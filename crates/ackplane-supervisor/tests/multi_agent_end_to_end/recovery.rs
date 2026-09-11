use ackplane_client::companion::wire::{Claim, Operation};
use nix::{
    sys::signal::{kill, Signal},
    unistd::Pid,
};

use super::*;

#[path = "recovery/support.rs"]
mod support;
pub(super) use support::{Faults, Fixture};

#[derive(Clone, Copy)]
pub(super) enum Case {
    Stopped,
    Live,
    Rejected,
}

#[tokio::test]
async fn stopped_run_recovery_survives_supervisor_crash_lost_replies_and_completion_write_failure()
{
    exercise_two_workers(Scenario {
        recovery_case: Some(Case::Stopped),
        shutdown_receipt_fault: Some(ShutdownReceiptFault::LostAlways),
        ..Scenario::default()
    })
    .await;
}

#[tokio::test]
async fn recovery_refuses_unproven_live_workers_without_signalling_or_releasing_them() {
    exercise_two_workers(Scenario {
        recovery_case: Some(Case::Live),
        ..Scenario::default()
    })
    .await;
}

#[tokio::test]
async fn permanently_rejected_recovery_frames_keep_the_marker_and_original_lease() {
    exercise_two_workers(Scenario {
        recovery_case: Some(Case::Rejected),
        shutdown_receipt_fault: Some(ShutdownReceiptFault::Rejected),
        ..Scenario::default()
    })
    .await;
}

impl Fixture<'_> {
    pub(super) async fn run(&mut self, case: Case, daemon: &mut Daemon) {
        wait_for("both assignment frames acknowledged", || async {
            let first = self.inspect("first").await;
            let second = self.inspect("second").await;
            (first.acknowledged == 3 && second.acknowledged == 3).then_some(())
        })
        .await;
        let pids = self.worker_pids();
        assert!(pids.iter().all(|pid| kill(*pid, None).is_ok()));
        let preview = self.inspect("first").await;
        let busy = self.confirm(&preview).await;
        assert!(!busy.status.success());
        assert!(String::from_utf8_lossy(&busy.stderr).contains("already in use"));
        if matches!(case, Case::Live) {
            kill(Pid::from_raw(daemon.0.id() as i32), Signal::SIGKILL).unwrap();
            assert!(!daemon.wait().await.success());
            let refused = self.confirm(&preview).await;
            assert!(!refused.status.success());
            assert!(String::from_utf8_lossy(&refused.stderr).contains("no positive stop record"));
            assert!(
                pids.iter().all(|pid| kill(*pid, None).is_ok()),
                "recovery must not signal unproven workers"
            );
            assert_eq!(self.active_claims().await.len(), 2);
            self.assert_worker_starts(1, 1);
            fs::write(self.root.join("finish"), "finish fixture workers").unwrap();
            wait_for("orphaned fixture workers exit voluntarily", || async {
                pids.iter()
                    .all(|pid| kill(*pid, None).is_err())
                    .then_some(())
            })
            .await;
            assert!(!self.inspect("first").await.stopped);
            return;
        }
        self.release_failures.store(usize::MAX, Ordering::SeqCst);
        daemon.request_shutdown();
        wait_for(
            "both positive stop records and remote terminal receipts",
            || async {
                for (slot, session) in ["first", "second"].iter().zip(self.sessions) {
                    if !self.inspect(slot).await.stopped
                        || self
                            .supervisors
                            .lifecycle_history(self.tenant, self.repository, &session.session_id)
                            .await
                            .unwrap()
                            .len()
                            != 2
                    {
                        return None;
                    }
                }
                Some(())
            },
        )
        .await;
        assert!(
            pids.iter().all(|pid| kill(*pid, None).is_err()),
            "durable stop proof must follow actual worker exit"
        );
        if daemon.0.try_wait().unwrap().is_none() {
            kill(Pid::from_raw(daemon.0.id() as i32), Signal::SIGKILL).unwrap();
        }
        assert!(!daemon.wait().await.success());
        self.release_failures.store(0, Ordering::SeqCst);
        assert_eq!(self.active_claims().await.len(), 2);
        let first = self.inspect("first").await;
        let outbox = self.outbox(&first);
        let original: Vec<u8> = outbox
            .query_row(
                "SELECT frame FROM outbound_frames WHERE sequence = 4",
                [],
                |row| row.get(0),
            )
            .unwrap();
        for (fault, diagnostic) in [
            (1, "omitted its independent"),
            (2, "outside the recoverable interval"),
            (3, "outside the recoverable interval"),
        ] {
            self.faults.position.store(fault, Ordering::SeqCst);
            let refused = self.confirm(&first).await;
            assert!(!refused.status.success());
            assert!(
                String::from_utf8_lossy(&refused.stderr).contains(diagnostic),
                "{}",
                String::from_utf8_lossy(&refused.stderr)
            );
            assert_eq!(self.inspect("first").await.acknowledged, first.acknowledged);
            assert!(self.marker("first").exists());
            assert_eq!(self.active_claims().await.len(), 2);
        }
        self.faults.position.store(0, Ordering::SeqCst);
        let refused = self.confirm(&first).await;
        assert!(!refused.status.success());
        assert!(self.marker("first").exists());
        let retained: Vec<u8> = outbox
            .query_row(
                "SELECT frame FROM outbound_frames WHERE sequence = 4",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(
            original, retained,
            "failed recovery must preserve the original sequence and frame bytes"
        );
        assert_eq!(self.active_claims().await.len(), 2);
        if matches!(case, Case::Rejected) {
            assert!(
                String::from_utf8_lossy(&refused.stderr).contains("permanently rejected"),
                "{}",
                String::from_utf8_lossy(&refused.stderr)
            );
            return;
        }
        self.faults.hold_reply.store(true, Ordering::SeqCst);
        let mut interrupted = self
            .confirmation_command(&first)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let permit =
            tokio::time::timeout(Duration::from_secs(10), self.faults.reply_held.acquire())
                .await
                .unwrap()
                .unwrap();
        permit.forget();
        interrupted.kill().await.unwrap();
        self.faults.hold_reply.store(false, Ordering::SeqCst);
        let retained: Vec<u8> = outbox
            .query_row(
                "SELECT frame FROM outbound_frames WHERE sequence = 4",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(
            original, retained,
            "crash after remote acceptance must preserve exact local replay bytes"
        );
        assert!(self.marker("first").exists());
        assert_eq!(self.active_claims().await.len(), 2);
        self.fault_enabled.store(false, Ordering::SeqCst);
        outbox.execute_batch("CREATE TRIGGER reject_completion BEFORE INSERT ON worker_recovery_events WHEN NEW.event = 'completed' BEGIN SELECT RAISE(ABORT, 'injected recovery completion failure'); END;").unwrap();
        let failed_audit = self.confirm(&first).await;
        assert!(!failed_audit.status.success());
        assert!(String::from_utf8_lossy(&failed_audit.stderr)
            .contains("injected recovery completion failure"));
        assert!(self.marker("first").exists());
        assert_eq!(
            self.active_claims().await.len(),
            1,
            "release succeeded before the completion write failed"
        );
        let archived: Vec<u8> = outbox
            .query_row(
                "SELECT frame FROM acknowledged_lifecycle_receipts WHERE sequence = 4",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(original, archived);
        outbox
            .execute_batch("DROP TRIGGER reject_completion")
            .unwrap();
        let replacement: v1::ClaimLeaseResult = self
            .client()
            .protobuf(Operation::Claim(Claim::Delegate {
                task_id: first.task_id.clone(),
                owner_id: "replacement-owner".into(),
                branch: "agents/replacement".into(),
                lease_seconds: 300,
                paths: vec!["src/first.rs".into()],
                symbols: vec![],
            }))
            .await
            .unwrap();
        assert_eq!(replacement.owner_id, "replacement-owner");
        assert_eq!(
            replacement.outcome,
            ackplane_client::ClaimLeaseOutcome::Granted as i32
        );
        assert!(
            !self.confirm(&first).await.status.success(),
            "the stale preview must not be reused after acknowledgement progressed"
        );
        let fresh = self.inspect("first").await;
        let cleaned = self.confirm(&fresh).await;
        assert!(
            cleaned.status.success(),
            "{}",
            String::from_utf8_lossy(&cleaned.stderr)
        );
        assert!(!self.marker("first").exists());
        let retried = self.confirm(&fresh).await;
        assert!(
            retried.status.success(),
            "{}",
            String::from_utf8_lossy(&retried.stderr)
        );
        assert!(self
            .active_claims()
            .await
            .iter()
            .any(|claim| claim.task_id == first.task_id && claim.owner_id == "replacement-owner"));
        let second = self.inspect("second").await;
        let cleaned = self.confirm(&second).await;
        assert!(
            cleaned.status.success(),
            "{}",
            String::from_utf8_lossy(&cleaned.stderr)
        );
        for session in self.sessions {
            let history = self
                .supervisors
                .lifecycle_history(self.tenant, self.repository, &session.session_id)
                .await
                .unwrap();
            assert_eq!(
                history.len(),
                2,
                "recovery cannot invent an additional lifecycle transition"
            );
            assert_eq!(
                history
                    .iter()
                    .filter(|entry| entry.receipt.state == SupervisorWorkerState::Terminated)
                    .count(),
                1
            );
        }
        self.assert_worker_starts(1, 1);
        let release: v1::ClaimReleaseResult = self
            .client()
            .protobuf(Operation::Claim(Claim::Release {
                task_id: first.task_id,
                owner_id: "replacement-owner".into(),
            }))
            .await
            .unwrap();
        assert!(release.released);
        self.fresh_assignment().await;
    }
}
