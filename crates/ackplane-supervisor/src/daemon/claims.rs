use ackplane_client::{
    companion::wire::{Claim, Operation},
    ClaimLeaseOutcome,
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
        let detail: v1::WorkTaskDetailResult = config
            .node
            .protobuf(Operation::WorkDetail {
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
        let result: v1::ClaimLeaseResult = config
            .node
            .protobuf(Operation::Claim(Claim::Delegate {
                task_id: task_id.into(),
                owner_id: self.session.session_id.clone(),
                branch: branch.clone(),
                lease_seconds: LEASE_SECONDS,
                paths: task.declared_paths.clone(),
                symbols: task.declared_symbols.clone(),
            }))
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
        let result: v1::ClaimLeaseResult = config
            .node
            .protobuf(Operation::Claim(Claim::Renew {
                task_id: task_id.into(),
                owner_id: self.session.session_id.clone(),
                lease_seconds: LEASE_SECONDS,
            }))
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
        config
            .node
            .protobuf::<v1::ClaimReleaseResult>(Operation::Claim(Claim::Release {
                task_id: task_id.into(),
                owner_id: self.session.session_id.clone(),
            }))
            .await
            .map_err(Box::new)?;
        if self
            .lease
            .as_ref()
            .is_some_and(|lease| lease.task_id == task_id)
        {
            self.lease = None;
        }
        Ok(())
    }
}
