use std::time::SystemTime;

use super::{
    embeddings::read_candidates, rank, ProjectionError, Projector, RankedNode,
    STRUCTURAL_FACT_PAYLOAD_TYPE,
};

const CANDIDATE_LIMIT: i64 = 200;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecallState {
    Empty,
    NotYetProjected,
    Stale,
    NotYetEmbedded,
    PartiallyEmbedded,
    Current,
}

#[derive(Debug)]
pub struct RecallSnapshot {
    pub nodes: Vec<RankedNode>,
    pub searched: bool,
    pub projected_nodes: i64,
    pub embedded_nodes: i64,
    pub ledger_position: i64,
    pub projected_position: Option<i64>,
    pub projected_at: Option<SystemTime>,
}

impl RecallSnapshot {
    pub fn state(&self) -> RecallState {
        match self.projected_position {
            None if self.ledger_position != 0 || self.projected_nodes != 0 => {
                return RecallState::NotYetProjected;
            }
            Some(position) if position != self.ledger_position => return RecallState::Stale,
            None | Some(_) => {}
        }
        if self.projected_nodes == 0 {
            RecallState::Empty
        } else if self.embedded_nodes == 0 {
            RecallState::NotYetEmbedded
        } else if self.embedded_nodes < self.projected_nodes {
            RecallState::PartiallyEmbedded
        } else {
            RecallState::Current
        }
    }
}

impl Projector {
    pub async fn recall(
        &self,
        tenant_id: &str,
        repository_id: &str,
        model: &str,
        query: &[f32],
        floor: f32,
        limit: usize,
    ) -> Result<RecallSnapshot, ProjectionError> {
        let mut connection = self.connection().await?;
        let transaction = connection
            .build_transaction()
            .isolation_level(tokio_postgres::IsolationLevel::RepeatableRead)
            .read_only(true)
            .start()
            .await?;
        let row = transaction.query_one(
            "SELECT \
                 (SELECT count(*) FROM projected_nodes WHERE tenant_id = $1 AND repository_id = $2), \
                 (SELECT count(*) FROM projected_node_embeddings \
                  WHERE tenant_id = $1 AND repository_id = $2 AND model = $3), \
                 (SELECT COALESCE(max(stream_position), 0) FROM ledger_records \
                  WHERE tenant_id = $1 AND repository_id = $2 AND payload_type = $4), \
                 ps.stream_position, ps.projected_at \
             FROM (SELECT 1) AS snapshot \
             LEFT JOIN projection_state ps ON ps.tenant_id = $1 AND ps.repository_id = $2",
            &[&tenant_id, &repository_id, &model, &STRUCTURAL_FACT_PAYLOAD_TYPE],
        ).await?;
        let mut snapshot = RecallSnapshot {
            nodes: vec![],
            searched: false,
            projected_nodes: row.get(0),
            embedded_nodes: row.get(1),
            ledger_position: row.get(2),
            projected_position: row.get(3),
            projected_at: row.get(4),
        };
        if !query.is_empty()
            && matches!(
                snapshot.state(),
                RecallState::Current | RecallState::PartiallyEmbedded
            )
        {
            let candidates = read_candidates(
                &*transaction,
                tenant_id,
                repository_id,
                model,
                query,
                CANDIDATE_LIMIT,
            )
            .await?;
            snapshot.nodes = rank(candidates, floor, limit);
            snapshot.searched = true;
        }
        transaction.commit().await?;
        Ok(snapshot)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn freshness_and_coverage_distinguish_empty_results() {
        for (projected_nodes, embedded_nodes, ledger_position, projected_position, expected) in [
            (0, 0, 0, None, RecallState::Empty),
            (0, 0, 2, None, RecallState::NotYetProjected),
            (2, 2, 2, None, RecallState::NotYetProjected),
            (2, 2, 3, Some(2), RecallState::Stale),
            (2, 2, 1, Some(2), RecallState::Stale),
            (0, 0, 0, Some(0), RecallState::Empty),
            (2, 0, 2, Some(2), RecallState::NotYetEmbedded),
            (2, 1, 2, Some(2), RecallState::PartiallyEmbedded),
            (2, 2, 2, Some(2), RecallState::Current),
        ] {
            let snapshot = RecallSnapshot {
                nodes: vec![],
                searched: false,
                projected_nodes,
                embedded_nodes,
                ledger_position,
                projected_position,
                projected_at: None,
            };
            assert_eq!(snapshot.state(), expected);
        }
    }
}
