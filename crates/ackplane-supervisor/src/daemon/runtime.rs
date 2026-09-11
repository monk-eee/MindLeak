use std::{
    fs::OpenOptions,
    io::Write,
    path::PathBuf,
    time::{Duration, Instant},
};

use ackplane_client::NodeSyncConnection;
use ackplane_protocol::{context_packet::*, supervisor::*, v1};
use time::{format_description::well_known::Rfc3339, OffsetDateTime};

use super::{registration, session, DaemonError};
use crate::{
    config::SupervisorConfig, AdapterError, ProcessWorkerAdapter, SupervisorInbox,
    SupervisorOutbox, WorkerAdapter, WorkerCommand,
};

pub(super) struct WorkerRuntime {
    pub session: SupervisorSession,
    pub started_at: OffsetDateTime,
    pub registration: SupervisorRegistration,
    pub inbox: SupervisorInbox,
    pub outbox: SupervisorOutbox,
    adapter: ProcessWorkerAdapter,
    command: Option<WorkerCommand>,
    pub(super) lease: Option<WorkerLease>,
    active: Option<ContextPacket>,
    run_path: Option<PathBuf>,
    pub finished: bool,
}

pub(super) struct WorkerLease {
    pub(super) task_id: String,
    expires_at: OffsetDateTime,
    next_renewal: Instant,
}

impl WorkerRuntime {
    pub fn new(config: &SupervisorConfig, node_id: &str) -> Result<Self, DaemonError> {
        let started_at = OffsetDateTime::now_utc();
        let registration = registration(config, node_id);
        let session = session(config, started_at)?;
        Ok(Self {
            inbox: SupervisorInbox::open(
                config.inbox_path(),
                registration.clone(),
                session.clone(),
            )?,
            outbox: SupervisorOutbox::open(
                config.outbox_path(),
                registration.clone(),
                session.clone(),
            )?,
            session,
            registration,
            started_at,
            adapter: ProcessWorkerAdapter::new(),
            command: config.workers.values().next().cloned(),
            lease: None,
            active: None,
            run_path: None,
            finished: false,
        })
    }

    pub async fn dispatch(
        &mut self,
        config: &SupervisorConfig,
        connection: &mut NodeSyncConnection,
        directive: &v1::AgentDirective,
    ) -> Result<v1::DirectiveReceipt, DaemonError> {
        let receipt = self.inbox.receive(directive, OffsetDateTime::now_utc())?;
        if receipt.status != v1::DirectiveReceiptStatus::Accepted as i32
            || matches!(
                directive.payload,
                Some(v1::agent_directive::Payload::Notify(_))
            )
        {
            return Ok(receipt);
        }
        if directive.schema_version != "v1" {
            return self.refuse(directive, "unsupported directive protocol");
        }
        match &directive.payload {
            Some(v1::agent_directive::Payload::Assign(_)) => {
                self.assign(config, connection, directive).await
            }
            Some(v1::agent_directive::Payload::Terminate(termination))
                if termination.mode == v1::TerminationMode::Force as i32 =>
            {
                if self
                    .active
                    .as_ref()
                    .is_none_or(|packet| packet.scope.task_id != directive.task_id)
                {
                    return self.refuse(directive, "termination does not address the active task");
                }
                let receipt =
                    self.inbox
                        .apply(directive, OffsetDateTime::now_utc(), vec![], || {
                            self.adapter.terminate(&self.session.worker_id)
                        })?;
                if receipt.status == v1::DirectiveReceiptStatus::Applied as i32 {
                    self.finish(config, SupervisorWorkerState::Terminated)
                        .await?;
                }
                Ok(receipt)
            }
            _ => self.refuse(directive, "runtime cannot enforce this directive"),
        }
    }

    fn refuse(
        &self,
        directive: &v1::AgentDirective,
        reason: &str,
    ) -> Result<v1::DirectiveReceipt, DaemonError> {
        tracing::warn!(directive_id = %directive.directive_id, worker_id = %self.session.worker_id, %reason, "worker directive refused");
        Ok(self
            .inbox
            .apply(directive, OffsetDateTime::now_utc(), vec![], || {
                Err(AdapterError::InvalidAssignment(reason.into()))
            })?)
    }

    async fn assign(
        &mut self,
        config: &SupervisorConfig,
        connection: &mut NodeSyncConnection,
        directive: &v1::AgentDirective,
    ) -> Result<v1::DirectiveReceipt, DaemonError> {
        if self.lease.is_some() || self.active.is_some() || self.finished {
            return self.refuse(directive, "this session already has an assignment");
        }
        let Some(command) = self.command.clone() else {
            return self.refuse(directive, "no executable is configured for this worker");
        };
        let (task, expires_at) = match tokio::time::timeout(
            Duration::from_secs(15),
            self.acquire(config, &directive.task_id),
        )
        .await
        {
            Ok(Ok(grant)) => grant,
            Ok(Err(error)) => return self.refuse(directive, &error.to_string()),
            Err(_) => {
                return self.refuse(directive, "claim request timed out; no worker was started")
            }
        };
        self.lease = Some(WorkerLease {
            task_id: directive.task_id.clone(),
            expires_at,
            next_renewal: Instant::now() + Duration::from_secs(60),
        });
        let packet = match tokio::time::timeout(
            Duration::from_secs(15),
            connection.request_context_packet(v1::ContextPacketRequest {
                directive_id: directive.directive_id.clone(),
                agent_session_id: self.session.session_id.clone(),
                token_budget: 8192,
            }),
        )
        .await
        {
            Ok(Ok(packet)) => packet,
            other => {
                let _ = tokio::time::timeout(
                    Duration::from_secs(10),
                    self.release(config, &directive.task_id),
                )
                .await;
                return self.refuse(
                    directive,
                    &format!(
                        "context request failed: {}",
                        match other {
                            Ok(Err(error)) => error.to_string(),
                            Err(_) => "timed out".into(),
                            _ => unreachable!(),
                        }
                    ),
                );
            }
        };
        let scope = ContextPacketScope {
            tenant_id: config.node.tenant_id.clone(),
            repository_id: config.node.repository_id.clone(),
            task_id: directive.task_id.clone(),
            goal_id: task.goal_id,
            agent_session_id: self.session.session_id.clone(),
        };
        let assignment = command.assignment(
            &self.session.worker_id,
            &scope,
            &packet,
            OffsetDateTime::now_utc().unix_timestamp(),
        );
        let run_path = config
            .workers
            .keys()
            .next()
            .map(|name| config.worker_run_path(name))
            .ok_or_else(|| DaemonError::Worker("worker run path is missing".into()))?;
        let marker = serde_json::to_vec(&serde_json::json!({
            "supervisor_id": self.session.supervisor_id,
            "session_id": self.session.session_id,
            "worker_id": self.session.worker_id,
            "task_id": directive.task_id,
            "packet_id": packet.packet_id,
            "working_directory": command.working_directory,
            "inbox": config.inbox_path(),
            "outbox": config.outbox_path(),
        }))
        .map_err(|error| DaemonError::Worker(error.to_string()))?;
        let receipt = self.inbox.apply(
            directive,
            OffsetDateTime::now_utc(),
            vec![packet.packet_id.clone()],
            || {
                let assignment = assignment?;
                let mut file = OpenOptions::new()
                    .create_new(true)
                    .write(true)
                    .open(&run_path)
                    .map_err(|_| {
                        AdapterError::InvalidAssignment(format!(
                            "worker state requires recovery: {}",
                            run_path.display()
                        ))
                    })?;
                file.write_all(&marker)
                    .and_then(|_| file.sync_all())
                    .map_err(|error| AdapterError::InvalidAssignment(error.to_string()))?;
                match self.adapter.start(assignment) {
                    Ok(()) => {
                        self.run_path = Some(run_path.clone());
                        self.active = Some(packet.clone());
                        Ok(())
                    }
                    Err(error) => {
                        let _ = std::fs::remove_file(&run_path);
                        Err(error)
                    }
                }
            },
        )?;
        if receipt.status == v1::DirectiveReceiptStatus::Applied as i32 {
            self.queue_use(&packet, ContextPacketUseStatus::Accepted)?;
            self.queue_lifecycle(SupervisorWorkerState::Started)?;
            tracing::info!(worker_id = %self.session.worker_id, task_id = %directive.task_id, packet_id = %packet.packet_id, "worker started with scoped context");
        } else {
            let _ = tokio::time::timeout(
                Duration::from_secs(10),
                self.release(config, &directive.task_id),
            )
            .await;
        }
        Ok(receipt)
    }

    fn queue_use(
        &self,
        packet: &ContextPacket,
        status: ContextPacketUseStatus,
    ) -> Result<(), DaemonError> {
        let receipt = ContextPacketUseReceipt {
            packet_id: packet.packet_id.clone(),
            scope: packet.scope.clone(),
            occurred_at: OffsetDateTime::now_utc().unix_timestamp(),
            status,
            reason: None,
        };
        let receipt_json =
            serde_json::to_vec(&receipt).map_err(|error| DaemonError::Worker(error.to_string()))?;
        self.outbox.enqueue_next(v1::NodeFrame {
            frame: Some(v1::node_frame::Frame::ContextPacketUseReport(
                v1::ContextPacketUseReport {
                    receipt_json,
                    outbox_sequence: None,
                },
            )),
        })?;
        Ok(())
    }

    fn queue_lifecycle(&self, state: SupervisorWorkerState) -> Result<(), DaemonError> {
        let (state, reason) = match state {
            SupervisorWorkerState::Started => (
                v1::SupervisorWorkerState::Started,
                v1::SupervisorLifecycleReason::Unspecified,
            ),
            SupervisorWorkerState::Completed => (
                v1::SupervisorWorkerState::Completed,
                v1::SupervisorLifecycleReason::Unspecified,
            ),
            SupervisorWorkerState::Failed => (
                v1::SupervisorWorkerState::Failed,
                v1::SupervisorLifecycleReason::WorkerLost,
            ),
            SupervisorWorkerState::Terminated => (
                v1::SupervisorWorkerState::Terminated,
                v1::SupervisorLifecycleReason::Unspecified,
            ),
            _ => {
                return Err(DaemonError::Worker(
                    "unexpected process lifecycle state".into(),
                ))
            }
        };
        self.outbox.enqueue_next(v1::NodeFrame {
            frame: Some(v1::node_frame::Frame::SupervisorLifecycleReceipt(
                v1::SupervisorLifecycleReceipt {
                    supervisor_id: self.session.supervisor_id.clone(),
                    session_id: self.session.session_id.clone(),
                    worker_id: self.session.worker_id.clone(),
                    occurred_at: OffsetDateTime::now_utc()
                        .format(&Rfc3339)
                        .map_err(|_| DaemonError::Clock)?,
                    state: state as i32,
                    reason: reason as i32,
                    idempotency_key: format!("{}:{state:?}", self.session.session_id),
                    outbox_sequence: None,
                },
            )),
        })?;
        Ok(())
    }

    async fn finish(
        &mut self,
        config: &SupervisorConfig,
        state: SupervisorWorkerState,
    ) -> Result<(), DaemonError> {
        if self.active.is_some() {
            if let Err(error) = self.adapter.terminate(&self.session.worker_id) {
                if !matches!(error, AdapterError::UnknownWorker(_)) {
                    return Err(DaemonError::Worker(error.to_string()));
                }
            }
        }
        if let Some(packet) = &self.active {
            self.queue_lifecycle(state)?;
            self.finished = true;
            tracing::info!(worker_id = %self.session.worker_id, task_id = %packet.scope.task_id, packet_id = %packet.packet_id, ?state, "worker ended; task completion still requires evidence review");
            self.active = None;
        }
        if let Some(lease) = &self.lease {
            let task_id = lease.task_id.clone();
            match tokio::time::timeout(Duration::from_secs(10), self.release(config, &task_id))
                .await
            {
                Ok(Ok(())) => {}
                _ => {
                    tracing::warn!(%task_id, "lease release could not be confirmed; retaining cleanup state for retry")
                }
            }
        }
        Ok(())
    }

    pub async fn shutdown(&mut self, config: &SupervisorConfig) -> Result<(), DaemonError> {
        self.finish(config, SupervisorWorkerState::Terminated)
            .await?;
        self.finished = true;
        Ok(())
    }

    pub async fn observe(&mut self, config: &SupervisorConfig) -> Result<(), DaemonError> {
        if self.active.is_none() {
            return self.finish(config, SupervisorWorkerState::Failed).await;
        }
        let state = self
            .adapter
            .observe(&self.session.worker_id)
            .map_err(|error| DaemonError::Worker(error.to_string()))?;
        if state != SupervisorWorkerState::Started {
            return self.finish(config, state).await;
        }
        let Some(lease) = &self.lease else {
            return self.finish(config, SupervisorWorkerState::Failed).await;
        };
        if lease.expires_at <= OffsetDateTime::now_utc() + time::Duration::seconds(15) {
            return self.finish(config, SupervisorWorkerState::Failed).await;
        }
        if lease.next_renewal <= Instant::now() {
            let task_id = lease.task_id.clone();
            match tokio::time::timeout(Duration::from_secs(10), self.renew(config, &task_id)).await
            {
                Ok(Ok(expiry)) => {
                    if let Some(lease) = &mut self.lease {
                        lease.expires_at = expiry;
                        lease.next_renewal = Instant::now() + Duration::from_secs(60);
                    }
                }
                _ => return self.finish(config, SupervisorWorkerState::Failed).await,
            }
        }
        Ok(())
    }

    pub fn acknowledge_finished(&self) -> Result<bool, DaemonError> {
        if !self.finished || self.lease.is_some() || !self.outbox.pending(1)?.is_empty() {
            return Ok(false);
        }
        if let Some(path) = &self.run_path {
            std::fs::remove_file(path).map_err(|error| DaemonError::Worker(error.to_string()))?;
        }
        Ok(true)
    }
}
