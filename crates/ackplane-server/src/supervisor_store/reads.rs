use super::*;

impl SupervisorStore {
    pub async fn list_supervisors(
        &self,
        tenant_id: &str,
        repository_id: &str,
    ) -> Result<Vec<SupervisorStatus>, StoreError> {
        self.list_supervisors_at(
            tenant_id,
            repository_id,
            server_now_seconds()?,
            HEARTBEAT_STALE_AFTER_SECS,
        )
        .await
    }

    /// Lists supervisor freshness at a caller-supplied server observation
    /// instant. HTTP handlers supply their own server clock; browser callers
    /// never choose this value.
    pub async fn list_supervisors_at(
        &self,
        tenant_id: &str,
        repository_id: &str,
        now_seconds: i64,
        stale_after_secs: i64,
    ) -> Result<Vec<SupervisorStatus>, StoreError> {
        validate_scope(tenant_id, repository_id)?;
        let rows = self
            .connection()
            .await?
            .query(
                &format!(
                    "SELECT {REGISTRATION_COLUMNS} FROM supervisor_registrations \
                     WHERE tenant_id = $1 AND repository_id = $2 ORDER BY supervisor_id ASC"
                ),
                &[&tenant_id, &repository_id],
            )
            .await?;
        rows.into_iter()
            .map(|row| registration_status_from_row(&row, now_seconds, stale_after_secs))
            .collect()
    }

    pub async fn list_sessions(
        &self,
        tenant_id: &str,
        repository_id: &str,
        supervisor_id: &str,
    ) -> Result<Vec<SupervisorSessionProjection>, StoreError> {
        validate_scope(tenant_id, repository_id)?;
        let rows = self
            .connection()
            .await?
            .query(
                &format!(
                    "SELECT {SESSION_COLUMNS} FROM supervisor_sessions \
                     WHERE tenant_id = $1 AND repository_id = $2 AND supervisor_id = $3 \
                     ORDER BY started_at ASC, session_id ASC"
                ),
                &[&tenant_id, &repository_id, &supervisor_id],
            )
            .await?;
        rows.into_iter()
            .map(|row| session_projection_from_row(&row))
            .collect()
    }

    /// Resolves one immutable supervisor session inside its tenant and
    /// repository scope. Callers use the returned supervisor id only after
    /// they have independently authenticated the node that sent a frame.
    pub async fn session(
        &self,
        tenant_id: &str,
        repository_id: &str,
        session_id: &str,
    ) -> Result<Option<SupervisorSessionProjection>, StoreError> {
        validate_scope(tenant_id, repository_id)?;
        model::require_identifier("session_id", session_id)?;
        self.connection()
            .await?
            .query_opt(
                &format!(
                    "SELECT {SESSION_COLUMNS} FROM supervisor_sessions \
                     WHERE tenant_id = $1 AND repository_id = $2 AND session_id = $3"
                ),
                &[&tenant_id, &repository_id, &session_id],
            )
            .await?
            .map(|row| session_projection_from_row(&row))
            .transpose()
    }

    pub async fn lifecycle_history(
        &self,
        tenant_id: &str,
        repository_id: &str,
        session_id: &str,
    ) -> Result<Vec<SupervisorLifecycleReceiptRecord>, StoreError> {
        validate_scope(tenant_id, repository_id)?;
        let rows = self
            .connection()
            .await?
            .query(
                &format!(
                    "SELECT {RECEIPT_COLUMNS} FROM supervisor_lifecycle_receipts \
                     WHERE tenant_id = $1 AND repository_id = $2 AND session_id = $3 \
                     ORDER BY receipt_position ASC"
                ),
                &[&tenant_id, &repository_id, &session_id],
            )
            .await?;
        rows.into_iter()
            .map(|row| receipt_record_from_row(&row))
            .collect()
    }
}
