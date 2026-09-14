use super::*;

#[cfg(test)]
mod concurrency;

impl Projector {
    /// Drop and replay one repository's projection from its committed
    /// [`STRUCTURAL_FACT_PAYLOAD_TYPE`] ledger records, in stream order, all
    /// inside one transaction — a caller never observes a half-rebuilt
    /// projection.
    ///
    /// Rebuilds for the same tenant/repository serialize under a transaction
    /// lock. This explicit operation always replays, including invalidating
    /// derived embeddings; background catch-up rechecks freshness instead.
    ///
    /// Retries a genuine PostgreSQL deadlock (SQLSTATE 40P01) a bounded number
    /// of times. No foreign key ties `projected_edges` to `projected_nodes`
    /// (see `migrations/0002_projection.sql`), so this is B-tree index-page
    /// lock contention between concurrent rebuilds of *unrelated* tenants
    /// under enough parallel load, not a logical schema bug — confirmed live
    /// once the Coverage CI gate started running these tests against a real
    /// Postgres (ADR-0118) instead of hollow-skipping them. PostgreSQL's own
    /// documentation recommends the client simply reissue a deadlocked
    /// transaction; rebuild is naturally idempotent (exactly what
    /// [`a_rebuild_reproduces_the_same_projection_from_the_same_ledger`]
    /// proves), so a clean retry from scratch is safe.
    pub async fn rebuild(
        &self,
        tenant_id: &str,
        repository_id: &str,
    ) -> Result<ProjectionSummary, ProjectionError> {
        self.apply_projection(tenant_id, repository_id, false)
            .await
            .map(|(summary, _)| summary)
    }

    async fn apply_projection(
        &self,
        tenant_id: &str,
        repository_id: &str,
        only_if_stale: bool,
    ) -> Result<(ProjectionSummary, bool), ProjectionError> {
        const MAX_DEADLOCK_RETRIES: u32 = 3;
        let mut attempt = 0;
        loop {
            match self
                .rebuild_once(tenant_id, repository_id, only_if_stale)
                .await
            {
                Err(ProjectionError::Database(error))
                    if attempt < MAX_DEADLOCK_RETRIES && error.code().is_some_and(is_deadlock) =>
                {
                    attempt += 1;
                }
                result => return result,
            }
        }
    }

    async fn rebuild_once(
        &self,
        tenant_id: &str,
        repository_id: &str,
        only_if_stale: bool,
    ) -> Result<(ProjectionSummary, bool), ProjectionError> {
        let mut connection = self.connection().await?;
        let transaction = connection.transaction().await?;

        let lock_scope = format!("mindleak.projection:{tenant_id}");
        transaction
            .query_one(
                "SELECT pg_advisory_xact_lock(hashtext($1), hashtext($2))",
                &[&lock_scope, &repository_id],
            )
            .await?;
        if only_if_stale {
            let current = transaction
                .query_opt(
                    "SELECT ps.stream_position, \
                         (SELECT count(*) FROM projected_nodes \
                          WHERE tenant_id = $1 AND repository_id = $2), \
                         (SELECT count(*) FROM projected_edges \
                          WHERE tenant_id = $1 AND repository_id = $2) \
                     FROM projection_state ps \
                     WHERE ps.tenant_id = $1 AND ps.repository_id = $2 \
                       AND ps.stream_position >= ( \
                           SELECT COALESCE(max(stream_position), 0) FROM ledger_records \
                           WHERE tenant_id = $1 AND repository_id = $2 AND payload_type = $3 \
                       )",
                    &[&tenant_id, &repository_id, &STRUCTURAL_FACT_PAYLOAD_TYPE],
                )
                .await?;
            if let Some(current) = current {
                let summary = ProjectionSummary {
                    stream_position: current.get(0),
                    nodes: current.get(1),
                    edges: current.get(2),
                };
                transaction.commit().await?;
                return Ok((summary, false));
            }
        }

        transaction
            .execute(
                "DELETE FROM projected_edges WHERE tenant_id = $1 AND repository_id = $2",
                &[&tenant_id, &repository_id],
            )
            .await?;
        transaction
            .execute(
                "DELETE FROM projected_nodes WHERE tenant_id = $1 AND repository_id = $2",
                &[&tenant_id, &repository_id],
            )
            .await?;

        let rows = transaction
            .query(
                "SELECT payload, stream_position FROM ledger_records \
                 WHERE tenant_id = $1 AND repository_id = $2 AND payload_type = $3 \
                 ORDER BY stream_position ASC",
                &[&tenant_id, &repository_id, &STRUCTURAL_FACT_PAYLOAD_TYPE],
            )
            .await?;

        let mut last_position: i64 = 0;
        for row in &rows {
            let payload: Vec<u8> = row.get(0);
            let position: i64 = row.get(1);
            let fact: StructuralFact = serde_json::from_slice(&payload)
                .map_err(|source| ProjectionError::MalformedFact { position, source })?;

            transaction
                .execute(
                    "INSERT INTO projected_nodes \
                         (tenant_id, repository_id, node_id, node_type, label, created_at, updated_at) \
                     VALUES ($1, $2, $3, $4, $5, now(), now()) \
                     ON CONFLICT (tenant_id, repository_id, node_id) DO UPDATE SET \
                         node_type = EXCLUDED.node_type, label = EXCLUDED.label, updated_at = now()",
                    &[
                        &tenant_id,
                        &repository_id,
                        &fact.node_id,
                        &fact.node_type,
                        &fact.label,
                    ],
                )
                .await?;

            for edge in &fact.edges {
                transaction
                    .execute(
                        "INSERT INTO projected_edges \
                             (tenant_id, repository_id, source_id, target_id, relation, \
                              base_weight, half_life_hours, updated_at) \
                         VALUES ($1, $2, $3, $4, $5, $6, $7, now()) \
                         ON CONFLICT (tenant_id, repository_id, source_id, target_id, relation) \
                         DO UPDATE SET \
                             base_weight = EXCLUDED.base_weight, \
                             half_life_hours = EXCLUDED.half_life_hours, \
                             updated_at = now()",
                        &[
                            &tenant_id,
                            &repository_id,
                            &fact.node_id,
                            &edge.target_id,
                            &edge.relation,
                            &edge.base_weight,
                            &edge.half_life_hours,
                        ],
                    )
                    .await?;
            }

            last_position = position;
        }

        transaction
            .execute(
                "INSERT INTO projection_state (tenant_id, repository_id, stream_position, projected_at) \
                 VALUES ($1, $2, $3, now()) \
                 ON CONFLICT (tenant_id, repository_id) DO UPDATE SET \
                     stream_position = EXCLUDED.stream_position, projected_at = now()",
                &[&tenant_id, &repository_id, &last_position],
            )
            .await?;

        let node_count: i64 = transaction
            .query_one(
                "SELECT count(*) FROM projected_nodes WHERE tenant_id = $1 AND repository_id = $2",
                &[&tenant_id, &repository_id],
            )
            .await?
            .get(0);
        let edge_count: i64 = transaction
            .query_one(
                "SELECT count(*) FROM projected_edges WHERE tenant_id = $1 AND repository_id = $2",
                &[&tenant_id, &repository_id],
            )
            .await?
            .get(0);

        transaction.commit().await?;
        Ok((
            ProjectionSummary {
                nodes: node_count,
                edges: edge_count,
                stream_position: last_position,
            },
            true,
        ))
    }

    /// Every repository whose committed structural facts are ahead of its
    /// projection checkpoint (ADR-0086 clause 9): a missing `projection_state`
    /// row reads as checkpoint zero, so a repository that has never been
    /// projected but has at least one structural fact is included too. A
    /// repository with zero structural-fact records never appears here —
    /// there is nothing for `rebuild` to give it.
    async fn stale_projections(&self) -> Result<Vec<StaleProjection>, ProjectionError> {
        let connection = self.connection().await?;
        let rows = connection
            .query(
                "SELECT lr.tenant_id, lr.repository_id \
                 FROM ledger_records lr \
                 LEFT JOIN projection_state ps \
                    ON ps.tenant_id = lr.tenant_id AND ps.repository_id = lr.repository_id \
                 WHERE lr.payload_type = $1 \
                 GROUP BY lr.tenant_id, lr.repository_id, ps.stream_position \
                 HAVING max(lr.stream_position) > COALESCE(ps.stream_position, 0)",
                &[&STRUCTURAL_FACT_PAYLOAD_TYPE],
            )
            .await?;
        Ok(rows
            .into_iter()
            .map(|row| StaleProjection {
                tenant_id: row.get(0),
                repository_id: row.get(1),
            })
            .collect())
    }

    /// One polling pass (ADR-0086 clause 9): rebuild every repository
    /// [`stale_projections`](Self::stale_projections) finds. One repository's
    /// rebuild failing is logged and does not stop the rest, or the caller's
    /// next tick — a projection worker's job is to catch a stream back up,
    /// not to guarantee every tick succeeds. The scan is only a work list:
    /// freshness is rechecked after locking each tenant/repository, so a
    /// delayed scan cannot discard vectors indexed after another worker
    /// caught up. Already-current repositories are not counted as rebuilt.
    pub async fn rebuild_stale(&self) -> Result<usize, ProjectionError> {
        let stale = self.stale_projections().await?;
        let mut rebuilt = 0;
        for repository in &stale {
            match self
                .apply_projection(&repository.tenant_id, &repository.repository_id, true)
                .await
            {
                Ok((summary, true)) => {
                    tracing::info!(
                        tenant_id = %repository.tenant_id,
                        repository_id = %repository.repository_id,
                        nodes = summary.nodes,
                        edges = summary.edges,
                        stream_position = summary.stream_position,
                        "rebuilt a repository's graph projection"
                    );
                    rebuilt += 1;
                }
                Ok((_, false)) => {}
                Err(error) => {
                    tracing::error!(
                        tenant_id = %repository.tenant_id,
                        repository_id = %repository.repository_id,
                        %error,
                        "projection rebuild failed for one repository; continuing with the rest"
                    );
                }
            }
        }
        Ok(rebuilt)
    }

    /// This repository's projection freshness, or `None` if it has never
    /// been projected (ADR-0087 clause 10).
    pub async fn freshness(
        &self,
        tenant_id: &str,
        repository_id: &str,
    ) -> Result<Option<ProjectionFreshness>, ProjectionError> {
        let connection = self.connection().await?;
        let row = connection
            .query_opt(
                "SELECT stream_position, projected_at FROM projection_state \
                 WHERE tenant_id = $1 AND repository_id = $2",
                &[&tenant_id, &repository_id],
            )
            .await?;
        Ok(row.map(|row| ProjectionFreshness {
            stream_position: row.get(0),
            projected_at: row.get(1),
        }))
    }
}

/// Run [`Projector::rebuild_stale`] on `interval` forever (ADR-0086 clause 9:
/// "Projection workers read the durable ledger through checkpoints").
/// Intended to run as its own background task (`tokio::spawn`) alongside the
/// gRPC server; a tick's database error is logged and the loop keeps polling
/// rather than exiting, since a missed tick is simply caught up by the next
/// one, not a fatal condition for the worker.
pub async fn run_projection_worker(projector: Projector, interval: std::time::Duration) {
    let mut ticker = tokio::time::interval(interval);
    loop {
        ticker.tick().await;
        if let Err(error) = projector.rebuild_stale().await {
            tracing::error!(%error, "could not check for stale projections this tick");
        }
    }
}

#[cfg(test)]
mod tests;
