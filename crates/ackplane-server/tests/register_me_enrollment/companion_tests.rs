use std::{
    process::Stdio,
    time::{Duration, SystemTime},
};

use ackplane_client::companion::{wire::SupervisorScope, NodeClient};
use ackplane_protocol::v1;
use ackplane_server::{
    db_pool::PgPool,
    signing_keys::{self, KeyRevocation},
};
use tokio::{
    io::{AsyncBufReadExt, BufReader},
    process::{Child, Command},
};

use super::support::{run_cli, TestIdentity};

async fn start(directory: &TestIdentity, endpoint: &str) -> Child {
    let mut child = Command::new(env!("CARGO_BIN_EXE_register-me"))
        .args(["serve", "--grpc-endpoint", endpoint, "--state-dir"])
        .arg(directory.path())
        .env_remove("MINDLEAK_ACKPLANE_TLS_CA_PATH")
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .stdin(Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    let ready = tokio::time::timeout(
        Duration::from_secs(10),
        BufReader::new(child.stdout.take().unwrap())
            .lines()
            .next_line(),
    )
    .await
    .unwrap()
    .unwrap();
    assert!(
        ready.is_some_and(|line| line.starts_with("node companion ready:")),
        "companion never reported readiness"
    );
    child
}

pub(super) async fn exercise(
    directory: &TestIdentity,
    endpoint: &str,
    tenant: &str,
    pool: &PgPool,
    revoke: bool,
) {
    let metadata = std::fs::read(directory.path().join("enrolment.json")).unwrap();
    let mut child = start(directory, endpoint).await;
    let node = NodeClient::new(directory.path().into(), tenant.into(), "repo-test".into());
    let original = node.identity().await.unwrap();
    assert_eq!(original.node_id, "node-test");
    let mut misdirected = node.clone();
    misdirected.expected_endpoint = Some("http://127.0.0.1:1".into());
    assert!(matches!(
        misdirected.identity().await,
        Err(ackplane_client::ClientError::ConnectionRefused {
            retryable: false,
            ..
        })
    ));
    let mut streams = Vec::new();
    for _index in 0..32 {
        streams.push(node.open_sync(0, None).await.unwrap());
    }
    assert!(
        node.open_sync(0, None).await.is_err(),
        "extra streams must be refused"
    );
    assert_eq!(
        node.identity().await.unwrap(),
        original,
        "stream capacity must leave room for short-lived authority calls"
    );
    drop(streams);
    let status = node.status().await.unwrap();
    assert!(status.verified);
    assert_eq!(status.state, v1::EnrollmentState::Active as i32);
    let duplicate = run_cli(directory.path(), &["serve", "--grpc-endpoint", endpoint]).await;
    assert!(
        !duplicate.status.success(),
        "a second provider owner must be refused"
    );

    child.kill().await.unwrap();
    child.wait().await.unwrap();
    assert!(node.identity().await.is_err());
    let mut child = start(directory, endpoint).await;
    assert_eq!(node.identity().await.unwrap(), original);
    let scope = SupervisorScope {
        supervisor_id: "enrolled-runtime".into(),
        session_id: "enrolled-runtime:session".into(),
        worker_id: "enrolled-runtime:worker".into(),
    };
    let mut connection = node.open_sync(0, Some(scope.clone())).await.unwrap();
    connection
        .exchange_supervisor_frame(v1::NodeFrame {
            frame: Some(v1::node_frame::Frame::SupervisorRegistration(
                v1::SupervisorRegistration {
                    supervisor_id: scope.supervisor_id.clone(),
                    node_id: original.node_id.clone(),
                    supervisor_version: "test".into(),
                    protocol_version: "v1".into(),
                    supported_directives: vec![v1::SupervisorDirectiveCapability::Notify as i32],
                    supports_checkpoint: false,
                    supports_force_termination: false,
                    outbox_durability: v1::SupervisorOutboxDurability::Persistent as i32,
                    recoverable_outbox: true,
                },
            )),
        })
        .await
        .unwrap();
    connection
        .exchange_supervisor_frame(v1::NodeFrame {
            frame: Some(v1::node_frame::Frame::SupervisorSession(
                v1::SupervisorSession {
                    supervisor_id: scope.supervisor_id,
                    session_id: scope.session_id,
                    worker_id: scope.worker_id,
                    runtime: v1::SupervisorRuntime::LocalMachine as i32,
                    started_at: time::OffsetDateTime::now_utc()
                        .format(&time::format_description::well_known::Rfc3339)
                        .unwrap(),
                    state: v1::SupervisorWorkerState::Started as i32,
                },
            )),
        })
        .await
        .unwrap();

    if revoke {
        let mut database = pool.get().await.unwrap();
        let transaction = database.transaction().await.unwrap();
        assert!(signing_keys::revoke(
            &transaction,
            &KeyRevocation {
                signing_key_id: original.signing_key_id,
                reason: "companion revocation test".into()
            },
            SystemTime::now()
        )
        .await
        .unwrap());
        transaction.commit().await.unwrap();
    } else {
        directory.remove_credential().unwrap();
    }
    let exit = tokio::time::timeout(Duration::from_secs(8), child.wait())
        .await
        .expect("loss of provider authority must stop the companion")
        .unwrap();
    assert!(!exit.success());
    assert!(
        !matches!(
            tokio::time::timeout(Duration::from_secs(2), connection.recv())
                .await
                .unwrap(),
            Ok(Some(_))
        ),
        "an established stream survived authority loss"
    );
    assert!(node.open_sync(0, None).await.is_err());
    let refused = run_cli(directory.path(), &["serve", "--grpc-endpoint", endpoint]).await;
    assert!(
        !refused.status.success(),
        "authority loss must not provision a replacement"
    );
    assert_eq!(
        std::fs::read(directory.path().join("enrolment.json")).unwrap(),
        metadata
    );
}
