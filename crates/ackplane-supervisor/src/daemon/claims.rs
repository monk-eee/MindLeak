use ackplane_client::{
    authenticate, ClaimClient, ClaimLeaseOutcome, ClaimOperation, WorkQueryClient,
};
use ackplane_protocol::v1;
use time::{format_description::well_known::Rfc3339, OffsetDateTime};

use super::{runtime::WorkerRuntime, DaemonError};
use crate::config::SupervisorConfig;

pub(super) const LEASE_SECONDS: u64 = 300;

impl WorkerRuntime {
    pub(super) async fn acquire(
        &mut self,
        config: &SupervisorConfig,
        task_id: &str,
    ) -> Result<(v1::WorkTaskSummary, OffsetDateTime), DaemonError> {
        let identity = &config.identity;
        let mut work = WorkQueryClient::connect(&config.endpoint)
            .await
            .map_err(Box::new)?;
        let detail = work
            .get_work_task_detail(v1::WorkTaskDetailRequest {
                tenant_id: identity.tenant_id.clone(),
                repository_id: identity.repository_id.clone(),
                task_id: task_id.into(),
            })
            .await
            .map_err(Box::new)?;
        let task = detail
            .task
            .ok_or_else(|| DaemonError::Worker("assignment has no published task".into()))?;
        if !matches!(task.state.as_str(), "open" | "claimed") || task.goal_id.is_empty() {
            return Err(DaemonError::Worker(
                "task is not executable or has no declared goal".into(),
            ));
        }
        let branch = &config
            .workers
            .values()
            .next()
            .ok_or_else(|| DaemonError::Worker("worker configuration is missing".into()))?
            .branch;
        let operation = ClaimOperation::Delegate {
            branch,
            lease_seconds: LEASE_SECONDS,
            paths: &task.declared_paths,
            symbols: &task.declared_symbols,
        };
        let authentication = authenticate(
            self.signer.as_ref(),
            &identity.tenant_id,
            &identity.repository_id,
            task_id,
            &self.session.session_id,
            &operation,
        );
        let mut client = ClaimClient::connect(&config.endpoint)
            .await
            .map_err(Box::new)?;
        let result = client
            .delegate_claim(v1::ClaimLeaseRequest {
                tenant_id: identity.tenant_id.clone(),
                repository_id: identity.repository_id.clone(),
                task_id: task_id.into(),
                owner_id: self.session.session_id.clone(),
                branch: branch.clone(),
                lease_seconds: LEASE_SECONDS,
                paths: task.declared_paths.clone(),
                symbols: task.declared_symbols.clone(),
                authentication: Some(authentication),
            })
            .await
            .map_err(Box::new)?;
        if result.outcome != ClaimLeaseOutcome::Granted as i32
            || result.owner_id != self.session.session_id
        {
            return Err(DaemonError::Worker(
                "the task lease belongs to another agent".into(),
            ));
        }
        let expires = OffsetDateTime::parse(&result.lease_expires_at, &Rfc3339)
            .map_err(|_| DaemonError::Clock)?;
        Ok((task, expires))
    }

    pub(super) async fn renew(
        &mut self,
        config: &SupervisorConfig,
        task_id: &str,
    ) -> Result<OffsetDateTime, DaemonError> {
        let identity = &config.identity;
        let authentication = authenticate(
            self.signer.as_ref(),
            &identity.tenant_id,
            &identity.repository_id,
            task_id,
            &self.session.session_id,
            &ClaimOperation::Renew {
                lease_seconds: LEASE_SECONDS,
            },
        );
        let mut client = ClaimClient::connect(&config.endpoint)
            .await
            .map_err(Box::new)?;
        let result = client
            .renew_claim(v1::ClaimRenewRequest {
                tenant_id: identity.tenant_id.clone(),
                repository_id: identity.repository_id.clone(),
                task_id: task_id.into(),
                owner_id: self.session.session_id.clone(),
                lease_seconds: LEASE_SECONDS,
                authentication: Some(authentication),
            })
            .await
            .map_err(Box::new)?;
        if result.outcome != ClaimLeaseOutcome::Granted as i32
            || result.owner_id != self.session.session_id
        {
            return Err(DaemonError::Worker(
                "worker no longer owns the task lease".into(),
            ));
        }
        OffsetDateTime::parse(&result.lease_expires_at, &Rfc3339).map_err(|_| DaemonError::Clock)
    }

    pub(super) async fn release(
        &mut self,
        config: &SupervisorConfig,
        task_id: &str,
    ) -> Result<(), DaemonError> {
        let identity = &config.identity;
        let authentication = authenticate(
            self.signer.as_ref(),
            &identity.tenant_id,
            &identity.repository_id,
            task_id,
            &self.session.session_id,
            &ClaimOperation::Release,
        );
        let mut client = ClaimClient::connect(&config.endpoint)
            .await
            .map_err(Box::new)?;
        client
            .release_claim(v1::ClaimReleaseRequest {
                tenant_id: identity.tenant_id.clone(),
                repository_id: identity.repository_id.clone(),
                task_id: task_id.into(),
                owner_id: self.session.session_id.clone(),
                authentication: Some(authentication),
            })
            .await
            .map_err(Box::new)?;
        Ok(())
    }
}
