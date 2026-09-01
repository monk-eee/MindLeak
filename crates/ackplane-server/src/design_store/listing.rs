use super::*;

impl DesignStore {
    pub async fn get_design(
        &self,
        tenant_id: &str,
        repository_id: &str,
        design_id: &str,
    ) -> Result<Option<Design>, DesignStoreError> {
        let connection = self.connection().await?;
        let row = connection
            .query_opt(
                "SELECT title, summary, source_version, lifecycle_state, \
                        constitution_version_id, work_task_id, evidence_id, display_label, \
                        created_at, updated_at \
                 FROM industrial_designs \
                 WHERE tenant_id = $1 AND repository_id = $2 AND design_id = $3",
                &[&tenant_id, &repository_id, &design_id],
            )
            .await?;
        let Some(row) = row else {
            return Ok(None);
        };
        let lifecycle_state: i16 = row.get(3);
        Ok(Some(Design {
            tenant_id: tenant_id.to_string(),
            repository_id: repository_id.to_string(),
            design_id: design_id.to_string(),
            title: row.get(0),
            summary: row.get(1),
            source_version: row.get(2),
            lifecycle_state: DesignLifecycleState::try_from(lifecycle_state).map_err(|()| {
                DesignStoreError::CorruptLifecycleState {
                    design_id: design_id.to_string(),
                    value: lifecycle_state,
                }
            })?,
            constitution_version_id: row.get(4),
            work_task_id: row.get(5),
            evidence_id: row.get(6),
            display_label: row.get(7),
            created_at: row.get(8),
            updated_at: row.get(9),
        }))
    }

    pub async fn list_decisions(
        &self,
        tenant_id: &str,
        repository_id: &str,
        design_id: &str,
    ) -> Result<Vec<DesignDecision>, DesignStoreError> {
        let connection = self.connection().await?;
        let rows = connection
            .query(
                "SELECT sequence_number, decision_kind, actor, rationale, recorded_at \
                 FROM industrial_design_decisions \
                 WHERE tenant_id = $1 AND repository_id = $2 AND design_id = $3 \
                 ORDER BY sequence_number ASC",
                &[&tenant_id, &repository_id, &design_id],
            )
            .await?;
        rows.into_iter()
            .map(|row| {
                let decision_kind: i16 = row.get(1);
                Ok(DesignDecision {
                    sequence_number: row.get(0),
                    decision_kind: DesignLifecycleState::try_from(decision_kind).map_err(|()| {
                        DesignStoreError::CorruptLifecycleState {
                            design_id: design_id.to_string(),
                            value: decision_kind,
                        }
                    })?,
                    actor: row.get(2),
                    rationale: row.get(3),
                    recorded_at: row.get(4),
                })
            })
            .collect()
    }

    /// Bounded, paginated design list (ADR-0121 decision 6 read surface).
    /// Ordered most-recently-updated first, matching `WorkStore::list_tasks`.
    pub async fn list_designs(
        &self,
        tenant_id: &str,
        repository_id: &str,
        lifecycle_state: Option<DesignLifecycleState>,
        page: i64,
        page_size: i64,
    ) -> Result<DesignPage, DesignStoreError> {
        let offset = (page - 1) * page_size;
        let connection = self.connection().await?;
        let rows = match lifecycle_state {
            Some(state) => {
                connection
                    .query(
                        "SELECT design_id, title, summary, source_version, lifecycle_state, \
                                constitution_version_id, work_task_id, evidence_id, \
                                display_label, created_at, updated_at, \
                                COUNT(*) OVER()::BIGINT AS total_count \
                         FROM industrial_designs \
                         WHERE tenant_id = $1 AND repository_id = $2 AND lifecycle_state = $3 \
                         ORDER BY updated_at DESC, design_id ASC LIMIT $4 OFFSET $5",
                        &[
                            &tenant_id,
                            &repository_id,
                            &(state as i16),
                            &page_size,
                            &offset,
                        ],
                    )
                    .await?
            }
            None => {
                connection
                    .query(
                        "SELECT design_id, title, summary, source_version, lifecycle_state, \
                                constitution_version_id, work_task_id, evidence_id, \
                                display_label, created_at, updated_at, \
                                COUNT(*) OVER()::BIGINT AS total_count \
                         FROM industrial_designs \
                         WHERE tenant_id = $1 AND repository_id = $2 \
                         ORDER BY updated_at DESC, design_id ASC LIMIT $3 OFFSET $4",
                        &[&tenant_id, &repository_id, &page_size, &offset],
                    )
                    .await?
            }
        };
        let total = rows.first().map(|row| row.get("total_count")).unwrap_or(0);
        let mut items = Vec::with_capacity(rows.len());
        for row in &rows {
            let design_id: String = row.get("design_id");
            let lifecycle_state: i16 = row.get("lifecycle_state");
            items.push(Design {
                tenant_id: tenant_id.to_string(),
                repository_id: repository_id.to_string(),
                design_id: design_id.clone(),
                title: row.get("title"),
                summary: row.get("summary"),
                source_version: row.get("source_version"),
                lifecycle_state: DesignLifecycleState::try_from(lifecycle_state).map_err(|()| {
                    DesignStoreError::CorruptLifecycleState {
                        design_id: design_id.clone(),
                        value: lifecycle_state,
                    }
                })?,
                constitution_version_id: row.get("constitution_version_id"),
                work_task_id: row.get("work_task_id"),
                evidence_id: row.get("evidence_id"),
                display_label: row.get("display_label"),
                created_at: row.get("created_at"),
                updated_at: row.get("updated_at"),
            });
        }
        Ok(DesignPage { items, total })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unique_id(label: &str) -> String {
        format!("{label}-{}", crate::test_support::uuid_ish())
    }

    #[tokio::test]
    async fn listing_designs_paginates_and_orders_most_recently_updated_first() {
        let Some(pool) = crate::test_support::test_pool() else {
            println!("skipped: ACKPLANE_TEST_DATABASE_URL not set");
            return;
        };
        let tenant_id = unique_id("tenant");
        let repository_id = unique_id("repository");
        let store = DesignStore::connect(&pool).await.unwrap();

        for label in ["first", "second", "third"] {
            store
                .create_design(CreateDesignRequest {
                    tenant_id: tenant_id.clone(),
                    repository_id: repository_id.clone(),
                    design_id: unique_id(label),
                    title: format!("design {label}"),
                    summary: "a summary".to_string(),
                    source_version: "v1".to_string(),
                    constitution_version_id: None,
                    work_task_id: None,
                    evidence_id: None,
                    proposed_by: "agent:test".to_string(),
                    display_label: Some(format!("label for {label}")),
                })
                .await
                .expect("creating a design should succeed");
        }

        let page = store
            .list_designs(&tenant_id, &repository_id, None, 1, 2)
            .await
            .expect("listing designs should succeed");

        assert_eq!(page.total, 3);
        assert_eq!(page.items.len(), 2);
        assert!(page.items.iter().all(|item| item
            .display_label
            .as_deref()
            .is_some_and(|label| label.starts_with("label for "))));

        let second_page = store
            .list_designs(&tenant_id, &repository_id, None, 2, 2)
            .await
            .expect("listing the second page should succeed");
        assert_eq!(second_page.total, 3);
        assert_eq!(second_page.items.len(), 1);
    }

    #[tokio::test]
    async fn listing_designs_filters_by_lifecycle_state() {
        let Some(pool) = crate::test_support::test_pool() else {
            println!("skipped: ACKPLANE_TEST_DATABASE_URL not set");
            return;
        };
        let tenant_id = unique_id("tenant");
        let repository_id = unique_id("repository");
        let store = DesignStore::connect(&pool).await.unwrap();
        let design_id = unique_id("design");
        store
            .create_design(CreateDesignRequest {
                tenant_id: tenant_id.clone(),
                repository_id: repository_id.clone(),
                design_id: design_id.clone(),
                title: "a design".to_string(),
                summary: "a summary".to_string(),
                source_version: "v1".to_string(),
                constitution_version_id: None,
                work_task_id: None,
                evidence_id: None,
                proposed_by: "agent:test".to_string(),
                display_label: Some("Jordan (via Bridge)".to_string()),
            })
            .await
            .expect("creating a design should succeed");

        let accepted_only = store
            .list_designs(
                &tenant_id,
                &repository_id,
                Some(DesignLifecycleState::Accepted),
                1,
                10,
            )
            .await
            .expect("listing designs should succeed");
        assert_eq!(accepted_only.total, 0);

        let proposed_only = store
            .list_designs(
                &tenant_id,
                &repository_id,
                Some(DesignLifecycleState::Proposed),
                1,
                10,
            )
            .await
            .expect("listing designs should succeed");
        assert_eq!(proposed_only.total, 1);
        assert_eq!(proposed_only.items[0].design_id, design_id);
        assert_eq!(
            proposed_only.items[0].display_label,
            Some("Jordan (via Bridge)".to_string())
        );
    }
}
