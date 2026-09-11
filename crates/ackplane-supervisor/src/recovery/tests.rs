use std::{collections::BTreeMap, fs, time::Duration};

use ackplane_client::companion::NodeClient;
use ackplane_protocol::{supervisor::directive_payload_digest, v1};
use time::{format_description::well_known::Rfc3339, OffsetDateTime};

use super::{record::RunRecord, *};
use crate::{
    config::SupervisorConfig, daemon, storage::claim_state_directory, SupervisorInbox,
    SupervisorOutbox, WorkerCommand,
};

struct Fixture {
    config: SupervisorConfig,
    record: RunRecord,
    bytes: Vec<u8>,
    outbox: SupervisorOutbox,
    _root: tempfile::TempDir,
}

impl Fixture {
    fn new(stopped: bool) -> Self {
        let root = tempfile::tempdir().unwrap();
        let directory = root.path().canonicalize().unwrap();
        let workspace = directory.join("workspace");
        fs::create_dir(&workspace).unwrap();
        let config = SupervisorConfig {
            node: NodeClient::new(directory.join("node"), "tenant".into(), "repository".into()),
            supervisor_id: "supervisor".into(),
            state_dir: directory.join("state"),
            heartbeat_interval: Duration::from_secs(1),
            workers: BTreeMap::from([(
                "slot".into(),
                WorkerCommand {
                    command: "unused-worker".into(),
                    args: vec!["{prompt}".into()],
                    working_directory: workspace.clone(),
                    branch: "agents/slot".into(),
                },
            )]),
        };
        drop(claim_state_directory(&config.state_dir).unwrap());
        let run_config = SupervisorConfig {
            supervisor_id: "supervisor-slot-run".into(),
            ..config.clone()
        };
        let registration = daemon::registration(&run_config, "node");
        let now = OffsetDateTime::now_utc();
        let session = daemon::session(&run_config, now).unwrap();
        let inbox = SupervisorInbox::open(
            run_config.inbox_path(),
            registration.clone(),
            session.clone(),
        )
        .unwrap();
        let outbox = SupervisorOutbox::open(
            run_config.outbox_path(),
            registration.clone(),
            session.clone(),
        )
        .unwrap();
        let mut directive = v1::AgentDirective {
            directive_id: "directive".into(),
            tenant_id: "tenant".into(),
            repository_id: "repository".into(),
            target_node_id: "node".into(),
            target_agent_session_id: session.session_id.clone(),
            kind: v1::DirectiveKind::Assign as i32,
            schema_version: "v1".into(),
            task_id: "task".into(),
            goal_id: "goal".into(),
            context_packet_id: "packet".into(),
            created_at: now.format(&Rfc3339).unwrap(),
            expires_at: (now + time::Duration::hours(1)).format(&Rfc3339).unwrap(),
            sequence: 1,
            idempotency_key: "assign".into(),
            required_capability: "assign.v1".into(),
            payload: Some(v1::agent_directive::Payload::Assign(
                v1::AssignDirective::default(),
            )),
            ..Default::default()
        };
        directive.payload_digest = directive_payload_digest(&directive).unwrap();
        let effect = inbox
            .apply(&directive, now, vec!["packet".into()], || Ok(()))
            .unwrap();
        assert_eq!(effect.status, v1::DirectiveReceiptStatus::Applied as i32);
        outbox
            .enqueue_next(v1::NodeFrame {
                frame: Some(v1::node_frame::Frame::DirectiveReceipt(effect)),
            })
            .unwrap();
        let record = RunRecord {
            version: 1,
            slot: "slot".into(),
            registration,
            session,
            task_id: "task".into(),
            directive_id: directive.directive_id,
            directive_digest: directive.payload_digest,
            packet_id: "packet".into(),
            working_directory: workspace,
            branch: "agents/slot".into(),
            inbox: run_config.inbox_path(),
            outbox: run_config.outbox_path(),
        };
        let bytes = serde_json::to_vec(&record).unwrap();
        fs::write(config.worker_run_path("slot"), &bytes).unwrap();
        let fixture = Self {
            config,
            record,
            bytes,
            outbox,
            _root: root,
        };
        if stopped {
            fixture
                .outbox
                .enqueue_with_stop(fixture.terminal(), Some(&fixture.bytes))
                .unwrap();
        }
        fixture
    }

    fn terminal(&self) -> v1::NodeFrame {
        v1::NodeFrame {
            frame: Some(v1::node_frame::Frame::SupervisorLifecycleReceipt(
                v1::SupervisorLifecycleReceipt {
                    supervisor_id: self.record.session.supervisor_id.clone(),
                    session_id: self.record.session.session_id.clone(),
                    worker_id: self.record.session.worker_id.clone(),
                    occurred_at: OffsetDateTime::now_utc().format(&Rfc3339).unwrap(),
                    state: v1::SupervisorWorkerState::Terminated as i32,
                    reason: v1::SupervisorLifecycleReason::Unspecified as i32,
                    idempotency_key: "terminal".into(),
                    outbox_sequence: None,
                },
            )),
        }
    }

    async fn confirm(&self, preview: &RecoveryPreview) -> Result<RecoveryPreview, RecoveryError> {
        confirm(
            &self.config,
            "slot",
            &preview.run_id,
            &preview.confirmation_digest,
            "test operator recovery",
        )
        .await
    }
}

#[test]
fn inspection_reads_original_stop_proof_without_acknowledging_pending_frames() {
    let fixture = Fixture::new(true);
    let before = fixture.outbox.positions().unwrap();
    let preview = inspect(&fixture.config, "slot").unwrap();
    assert!(preview.stopped);
    assert!(!preview.cleanup_recorded);
    assert_eq!(preview.pending_frames, 2);
    assert_eq!(fixture.outbox.positions().unwrap(), before);
    assert_eq!(
        fs::read(fixture.config.worker_run_path("slot")).unwrap(),
        fixture.bytes
    );
}

// Terminal labels once outlived their process handles; only the adapter's bound stop record can authorize cleanup.
#[tokio::test]
async fn a_terminal_receipt_without_positive_stop_provenance_never_authorizes_cleanup() {
    let fixture = Fixture::new(false);
    fixture.outbox.enqueue_next(fixture.terminal()).unwrap();
    let preview = inspect(&fixture.config, "slot").unwrap();
    assert!(!preview.stopped);
    let error = fixture.confirm(&preview).await.unwrap_err();
    assert!(
        error.to_string().contains("no positive stop record"),
        "{error}"
    );
    assert!(fixture.config.worker_run_path("slot").exists());
}

#[tokio::test]
async fn confirmation_refuses_queue_changes_after_preview_even_when_marker_is_unchanged() {
    let fixture = Fixture::new(true);
    let preview = inspect(&fixture.config, "slot").unwrap();
    fixture.outbox.acknowledge_through(1).unwrap();
    let current = inspect(&fixture.config, "slot").unwrap();
    assert_eq!(preview.marker_digest, current.marker_digest);
    assert_ne!(preview.confirmation_digest, current.confirmation_digest);
    let error = fixture.confirm(&preview).await.unwrap_err();
    assert!(
        error.to_string().contains("changed since inspection"),
        "{error}"
    );
    assert!(fixture.config.worker_run_path("slot").exists());
}

#[test]
fn changed_marker_bytes_cannot_borrow_the_original_stop_record() {
    let fixture = Fixture::new(true);
    let mut changed = fixture.bytes.clone();
    changed.push(b'\n');
    fs::write(fixture.config.worker_run_path("slot"), changed).unwrap();
    let error = inspect(&fixture.config, "slot").unwrap_err();
    assert!(error.to_string().contains("exact run marker"), "{error}");
}

// Protobuf drops unknown fields when decoded; accepting changed stored bytes would defeat exact replay and preview binding.
#[test]
fn recovery_refuses_changed_wire_bytes_even_when_the_decoded_receipt_is_equal() {
    let fixture = Fixture::new(true);
    let connection = rusqlite::Connection::open(&fixture.record.outbox).unwrap();
    let mut frame: Vec<u8> = connection
        .query_row(
            "SELECT frame FROM outbound_frames WHERE sequence = 2",
            [],
            |row| row.get(0),
        )
        .unwrap();
    frame.extend_from_slice(&[0x98, 0x06, 0x01]);
    connection
        .execute(
            "UPDATE outbound_frames SET frame = ?1 WHERE sequence = 2",
            [frame],
        )
        .unwrap();
    let error = inspect(&fixture.config, "slot").unwrap_err();
    assert!(error.to_string().contains("wire bytes"), "{error}");
}

#[test]
fn oversized_original_effect_is_refused_before_inspection_decodes_it() {
    use prost::Message;
    let fixture = Fixture::new(true);
    let connection = rusqlite::Connection::open(&fixture.record.inbox).unwrap();
    let bytes: Vec<u8> = connection
        .query_row("SELECT receipt FROM directive_effects", [], |row| {
            row.get(0)
        })
        .unwrap();
    let mut receipt = v1::DirectiveReceipt::decode(bytes.as_slice()).unwrap();
    receipt.diagnostic = "x".repeat(1024 * 1024);
    connection
        .execute(
            "UPDATE directive_effects SET receipt = ?1",
            [receipt.encode_to_vec()],
        )
        .unwrap();
    let error = inspect(&fixture.config, "slot").unwrap_err();
    assert!(error.to_string().contains("effect exceeds"), "{error}");
}

#[tokio::test]
async fn recovery_cannot_run_while_another_process_owns_the_state_directory() {
    let fixture = Fixture::new(true);
    let preview = inspect(&fixture.config, "slot").unwrap();
    let _guard = claim_state_directory(&fixture.config.state_dir).unwrap();
    let error = fixture.confirm(&preview).await.unwrap_err();
    assert!(error.to_string().contains("already in use"), "{error}");
    assert!(fixture.config.worker_run_path("slot").exists());
}

#[test]
fn older_versions_and_paths_outside_the_derived_queue_location_are_refused() {
    let fixture = Fixture::new(true);
    let mut record = fixture.record.clone();
    record.version = 0;
    fs::write(
        fixture.config.worker_run_path("slot"),
        serde_json::to_vec(&record).unwrap(),
    )
    .unwrap();
    assert!(inspect(&fixture.config, "slot").is_err());
    record.version = 1;
    record.outbox = record.inbox.clone();
    fs::write(
        fixture.config.worker_run_path("slot"),
        serde_json::to_vec(&record).unwrap(),
    )
    .unwrap();
    let error = inspect(&fixture.config, "slot").unwrap_err();
    assert!(error.to_string().contains("derived location"), "{error}");
}

#[test]
fn absent_state_is_not_created_by_inspection() {
    let mut fixture = Fixture::new(true);
    fixture.config.state_dir = fixture._root.path().join("absent");
    assert!(inspect(&fixture.config, "slot").is_err());
    assert!(!fixture.config.state_dir.exists());
}

#[test]
fn a_changed_configured_branch_cannot_authorize_the_recorded_run() {
    let mut fixture = Fixture::new(true);
    fixture.config.workers.get_mut("slot").unwrap().branch = "agents/replacement".into();
    let error = inspect(&fixture.config, "slot").unwrap_err();
    assert!(
        error
            .to_string()
            .contains("configured worker identity or workspace"),
        "{error}"
    );
    assert!(fixture.config.worker_run_path("slot").exists());
}

// A lost command reply after marker removal must be distinguishable from an unproven missing run.
#[tokio::test]
async fn completed_cleanup_can_be_retried_before_and_after_matching_marker_removal() {
    use crate::outbox::{RecoveryAttempt, RecoveryResult};
    let fixture = Fixture::new(true);
    let preview = inspect(&fixture.config, "slot").unwrap();
    let attempt = RecoveryAttempt {
        marker_digest: &preview.marker_digest,
        confirmation_digest: &preview.confirmation_digest,
        reason: "test crash after durable completion",
    };
    fixture
        .outbox
        .record_recovery_event(&attempt, None)
        .unwrap();
    fixture
        .outbox
        .acknowledge_through(preview.last_enqueued)
        .unwrap();
    let settled = inspect(&fixture.config, "slot").unwrap();
    fixture
        .outbox
        .record_recovery_event(
            &attempt,
            Some(&RecoveryResult {
                result_digest: settled.confirmation_digest,
                server_position: settled.acknowledged,
                lease_released: false,
            }),
        )
        .unwrap();
    let first = fixture.confirm(&preview).await.unwrap();
    assert!(first.cleanup_recorded);
    assert!(!fixture.config.worker_run_path("slot").exists());
    let retry = fixture
        .confirm(&preview)
        .await
        .expect("a lost cleanup reply is resolved from its immutable completed audit");
    assert!(retry.cleanup_recorded);
    assert_eq!(retry.confirmation_digest, first.confirmation_digest);
    fs::write(fixture.config.worker_run_path("slot"), b"newer run").unwrap();
    assert!(fixture.confirm(&preview).await.is_err());
    assert_eq!(
        fs::read(fixture.config.worker_run_path("slot")).unwrap(),
        b"newer run"
    );
}

#[cfg(unix)]
#[test]
fn a_symlink_marker_is_not_an_owned_run_record() {
    let fixture = Fixture::new(true);
    let marker = fixture.config.worker_run_path("slot");
    let target = fixture._root.path().join("foreign-marker.json");
    fs::rename(&marker, &target).unwrap();
    std::os::unix::fs::symlink(&target, &marker).unwrap();
    assert!(inspect(&fixture.config, "slot").is_err());
    assert!(target.exists());
}
