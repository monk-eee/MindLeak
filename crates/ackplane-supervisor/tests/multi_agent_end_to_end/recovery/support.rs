use std::{path::Path, process::Output};

use ackplane_client::companion::NodeClient;
use ackplane_protocol::supervisor::SupervisorSession;
use ackplane_supervisor::recovery::RecoveryPreview;
use nix::unistd::Pid;

use super::super::*;

pub(in super::super) struct Faults {
    pub position: AtomicUsize,
    pub hold_reply: AtomicBool,
    pub reply_held: tokio::sync::Semaphore,
}

impl Default for Faults {
    fn default() -> Self {
        Self {
            position: AtomicUsize::new(0),
            hold_reply: AtomicBool::new(false),
            reply_held: tokio::sync::Semaphore::new(0),
        }
    }
}

pub(in super::super) struct Fixture<'fixture> {
    pub root: &'fixture Path,
    pub pool: &'fixture PgPool,
    pub tenant: &'fixture str,
    pub repository: &'fixture str,
    pub node: &'fixture str,
    pub node_directory: &'fixture Path,
    pub supervisors: &'fixture SupervisorStore,
    pub work: &'fixture WorkStore,
    pub sessions: &'fixture [SupervisorSession],
    pub fault_enabled: &'fixture AtomicBool,
    pub release_failures: &'fixture AtomicUsize,
    pub faults: &'fixture Faults,
    pub daemon_command: &'fixture mut Command,
}

impl Fixture<'_> {
    fn command(&self, arguments: &[&str]) -> tokio::process::Command {
        let mut command = tokio::process::Command::new(self.daemon_command.get_program());
        command.args(self.daemon_command.get_args()).args(arguments);
        for (key, value) in self.daemon_command.get_envs() {
            if let Some(value) = value {
                command.env(key, value);
            } else {
                command.env_remove(key);
            }
        }
        command
            .env("RUST_LOG", "info")
            .stdin(Stdio::null())
            .kill_on_drop(true);
        command
    }

    pub(super) async fn inspect(&self, slot: &str) -> RecoveryPreview {
        let output = execute(self.command(&["recover", "inspect", slot])).await;
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        serde_json::from_slice(&output.stdout)
            .expect("recovery stdout is one JSON document, with logs on stderr")
    }

    pub(super) async fn confirm(&self, preview: &RecoveryPreview) -> Output {
        execute(self.confirmation_command(preview)).await
    }

    pub(super) fn confirmation_command(
        &self,
        preview: &RecoveryPreview,
    ) -> tokio::process::Command {
        self.command(&[
            "recover",
            "confirm",
            &preview.slot,
            &preview.run_id,
            &preview.confirmation_digest,
            "--reason",
            "fixture operator confirms stopped-run cleanup",
        ])
    }

    pub(super) fn marker(&self, slot: &str) -> std::path::PathBuf {
        self.root
            .join("state")
            .join(format!("{slot}.worker-run.json"))
    }

    pub(super) fn outbox(&self, preview: &RecoveryPreview) -> rusqlite::Connection {
        rusqlite::Connection::open(
            self.root
                .join("state")
                .join(format!("{}.outbox.db", preview.run_id)),
        )
        .unwrap()
    }

    pub(super) fn client(&self) -> NodeClient {
        NodeClient::new(
            self.node_directory.into(),
            self.tenant.into(),
            self.repository.into(),
        )
    }

    pub(super) fn worker_pids(&self) -> Vec<Pid> {
        ["first", "second"]
            .iter()
            .map(|slot| {
                let environment: serde_json::Value = serde_json::from_slice(
                    &fs::read(self.root.join(slot).join("environment.json")).unwrap(),
                )
                .unwrap();
                Pid::from_raw(
                    environment["process_id"]
                        .as_i64()
                        .unwrap()
                        .try_into()
                        .unwrap(),
                )
            })
            .collect()
    }

    pub(super) fn assert_worker_starts(&self, first: usize, second: usize) {
        for (slot, expected) in [("first", first), ("second", second)] {
            assert_eq!(
                fs::read_to_string(self.root.join(slot).join("worker-starts.txt"))
                    .unwrap()
                    .lines()
                    .count(),
                expected
            );
        }
    }

    pub(super) async fn active_claims(&self) -> Vec<ackplane_server::claim_store::ActiveClaim> {
        ClaimStore::connect(self.pool)
            .await
            .unwrap()
            .list_active(self.tenant, self.repository, SystemTime::now())
            .await
            .unwrap()
    }

    pub(super) async fn fresh_assignment(&mut self) {
        fs::write(
            self.root.join("finish"),
            "allow newly authorized worker completion",
        )
        .unwrap();
        let mut daemon = Daemon(self.daemon_command.group_spawn().unwrap());
        let session = wait_for("fresh first-slot session", || async {
            for supervisor in self
                .supervisors
                .list_supervisors(self.tenant, self.repository)
                .await
                .unwrap()
            {
                if supervisor
                    .registration
                    .supervisor_id
                    .starts_with("multi-agent-first-")
                {
                    for session in self
                        .supervisors
                        .list_sessions(
                            self.tenant,
                            self.repository,
                            &supervisor.registration.supervisor_id,
                        )
                        .await
                        .unwrap()
                    {
                        if !self
                            .sessions
                            .iter()
                            .any(|old| old.session_id == session.session.session_id)
                        {
                            return Some(session.session);
                        }
                    }
                }
            }
            None
        })
        .await;
        self.assert_worker_starts(1, 1);
        self.work
            .create_task(
                &NewWorkTask {
                    tenant_id: self.tenant.into(),
                    repository_id: self.repository.into(),
                    task_id: "task:fresh".into(),
                    title: "New authorization after cleanup".into(),
                    acceptance: "Exactly one new execution".into(),
                    goal_id: Some("goal:run".into()),
                    declared_paths: vec!["src/first.rs".into()],
                    declared_symbols: vec![],
                    published_by: self.node.into(),
                },
                "create:fresh",
                SystemTime::now(),
            )
            .await
            .unwrap();
        assign(
            self.pool,
            self.tenant,
            self.repository,
            self.node,
            &session.session_id,
            "task:fresh",
        )
        .await;
        wait_for("separately authorized fresh completion", || async {
            self.supervisors
                .lifecycle_history(self.tenant, self.repository, &session.session_id)
                .await
                .unwrap()
                .iter()
                .any(|entry| entry.receipt.state == SupervisorWorkerState::Completed)
                .then_some(())
        })
        .await;
        wait_for("fresh acknowledged marker removal", || async {
            (!self.marker("first").exists()).then_some(())
        })
        .await;
        self.assert_worker_starts(2, 1);
        let prompt: serde_json::Value =
            serde_json::from_slice(&fs::read(self.root.join("first/prompt.json")).unwrap())
                .unwrap();
        assert_eq!(prompt["scope"]["task_id"], "task:fresh");
        assert_eq!(prompt["scope"]["agent_session_id"], session.session_id);
        assert_eq!(
            self.work
                .task_detail(self.tenant, self.repository, "task:fresh")
                .await
                .unwrap()
                .unwrap()
                .task
                .state,
            WorkTaskState::Claimed
        );
        daemon.shutdown().await;
    }
}

async fn execute(mut command: tokio::process::Command) -> Output {
    tokio::time::timeout(Duration::from_secs(35), command.output())
        .await
        .expect("bounded recovery command")
        .unwrap()
}
