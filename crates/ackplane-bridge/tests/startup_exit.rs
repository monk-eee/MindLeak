use std::{
    fs,
    path::PathBuf,
    process::{Output, Stdio},
    time::Duration,
};

use tokio::{
    io::{AsyncBufReadExt, BufReader},
    net::TcpListener,
    process::Command,
    time::timeout,
};

const PASSWORD_SENTINEL: &str = "startup-test-password-must-not-be-logged";

fn bridge_command() -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_ackplane-bridge"));
    command.env_clear().kill_on_drop(true);
    if let Some(system_root) = std::env::var_os("SYSTEMROOT") {
        command.env("SYSTEMROOT", system_root);
    }
    command
}

struct StartupFixture {
    directory: PathBuf,
}

impl StartupFixture {
    fn new() -> Self {
        let mut random = [0_u8; 16];
        getrandom::getrandom(&mut random).expect("generate a unique fixture id");
        let directory = std::env::temp_dir().join(format!(
            "bridge-startup-{:032x}",
            u128::from_le_bytes(random)
        ));
        fs::create_dir(&directory).expect("create a new test-owned directory");
        let fixture = Self { directory };
        ackplane_bridge::load_or_generate_salt(&fixture.directory.join("salt.bin"))
            .expect("create a test-only tenant salt");
        fixture
    }

    fn command(&self) -> Command {
        let mut command = bridge_command();
        command
            .env("ACKPLANE_BRIDGE_SALT_PATH", self.directory.join("salt.bin"))
            .env("ACKPLANE_BRIDGE_DEVELOPMENT_TENANT", "startup-test")
            .env("ACKPLANE_BRIDGE_LISTEN", "127.0.0.1:0")
            .env("ACKPLANE_DATABASE_URL", "not-a-postgres-url");
        command
    }
}

impl Drop for StartupFixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.directory).expect("remove the test-owned salt directory");
    }
}

async fn assert_startup_failure(mut command: Command, diagnostic: &str) -> Output {
    let output = timeout(Duration::from_secs(20), command.output())
        .await
        .expect("Bridge startup failure must finish within the test deadline")
        .expect("execute the Bridge subprocess");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains(diagnostic),
        "unexpected diagnostic: {stderr}"
    );
    assert!(!stderr.contains(PASSWORD_SENTINEL));
    assert!(output.stdout.is_empty());
    assert_eq!(
        output.status.code(),
        Some(1),
        "a startup refusal must not report a successful process exit"
    );
    output
}

// Startup errors printed a refusal but returned exit 0, so launchers saw success.
#[tokio::test]
async fn missing_salt_configuration_is_an_unsuccessful_process_exit() {
    let output =
        assert_startup_failure(bridge_command(), "ACKPLANE_BRIDGE_SALT_PATH must be set").await;

    assert_eq!(
        String::from_utf8(output.stderr).expect("UTF-8 startup diagnostic"),
        "ackplane-bridge: ACKPLANE_BRIDGE_SALT_PATH must be set for the loopback developer profile\n"
    );
}

#[tokio::test]
async fn invalid_configuration_reports_failure_without_contacting_a_database() {
    let fixture = StartupFixture::new();
    for (setting, value, diagnostic) in [
        (
            "ACKPLANE_BRIDGE_SALT_PATH",
            " ",
            "ACKPLANE_BRIDGE_SALT_PATH must be set",
        ),
        (
            "ACKPLANE_DATABASE_URL",
            " ",
            "ACKPLANE_DATABASE_URL must be set",
        ),
        (
            "ACKPLANE_BRIDGE_DEVELOPMENT_TENANT",
            " ",
            "ACKPLANE_BRIDGE_DEVELOPMENT_TENANT must be set",
        ),
        (
            "ACKPLANE_BRIDGE_LISTEN",
            "invalid",
            "ACKPLANE_BRIDGE_LISTEN must be a socket address",
        ),
        (
            "ACKPLANE_BRIDGE_LISTEN",
            "0.0.0.0:3000",
            "may bind only to loopback",
        ),
        (
            "ACKPLANE_DB_POOL_MAX_SIZE",
            "0",
            "ACKPLANE_DB_POOL_MAX_SIZE must be a positive integer",
        ),
        (
            "ACKPLANE_DB_POOL_TIMEOUT_MS",
            "0",
            "ACKPLANE_DB_POOL_TIMEOUT_MS must be a positive integer",
        ),
        (
            "ACKPLANE_DATABASE_URL",
            "not-a-postgres-url",
            "could not build the Ackplane database pool",
        ),
    ] {
        let mut command = fixture.command();
        command.env(setting, value);
        assert_startup_failure(command, diagnostic).await;
    }
    let mut command = fixture.command();
    command.env(
        "ACKPLANE_DATABASE_URL",
        format!("postgresql://startup:{PASSWORD_SENTINEL}@127.0.0.1:invalid/database"),
    );
    assert_startup_failure(command, "could not build the Ackplane database pool").await;
}

#[tokio::test]
async fn salt_file_failure_is_an_unsuccessful_process_exit() {
    let fixture = StartupFixture::new();
    let mut command = fixture.command();
    command.env("ACKPLANE_BRIDGE_SALT_PATH", &fixture.directory);
    assert_startup_failure(
        command,
        "could not load or generate the developer-tenant salt",
    )
    .await;
}

#[tokio::test]
async fn a_database_connection_failure_reports_failure_without_logging_its_password() {
    let fixture = StartupFixture::new();
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("reserve a test-owned database endpoint");
    let address = listener.local_addr().expect("read the test endpoint");
    let mut command = fixture.command();
    command.env(
        "ACKPLANE_DATABASE_URL",
        format!("postgresql://startup:{PASSWORD_SENTINEL}@{address}/database"),
    );
    timeout(Duration::from_secs(20), async {
        tokio::join!(
            assert_startup_failure(command, "could not connect to Ackplane read models"),
            async {
                let (connection, _) = listener.accept().await.expect("accept the test connection");
                drop(connection);
            }
        );
    })
    .await
    .expect("the controlled database failure must not hang");
}

#[tokio::test]
async fn an_occupied_listener_is_an_unsuccessful_process_exit() {
    let Ok(database_url) = std::env::var("ACKPLANE_TEST_DATABASE_URL") else {
        eprintln!("skipping: ACKPLANE_TEST_DATABASE_URL is not set");
        return;
    };
    let fixture = StartupFixture::new();
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("reserve a test-owned Bridge listener");
    let address = listener.local_addr().expect("read the occupied address");
    let mut command = fixture.command();
    command
        .env("ACKPLANE_DATABASE_URL", database_url)
        .env("ACKPLANE_BRIDGE_LISTEN", address.to_string());
    assert_startup_failure(command, &format!("could not listen on {address}")).await;
}

#[tokio::test]
async fn valid_configuration_reaches_the_serving_state() {
    let Ok(database_url) = std::env::var("ACKPLANE_TEST_DATABASE_URL") else {
        eprintln!("skipping: ACKPLANE_TEST_DATABASE_URL is not set");
        return;
    };
    let fixture = StartupFixture::new();
    let mut child = fixture
        .command()
        .env("ACKPLANE_DATABASE_URL", database_url)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("start the Bridge with valid test configuration");
    let mut stdout = BufReader::new(child.stdout.take().expect("capture Bridge stdout"));
    let mut line = String::new();
    timeout(Duration::from_secs(20), stdout.read_line(&mut line))
        .await
        .expect("the test Bridge must finish startup")
        .expect("read startup announcement");
    assert!(line.starts_with("ackplane-bridge: serving Fleet for development tenant on http://"));
    assert!(child.try_wait().expect("inspect the test Bridge").is_none());
    child.kill().await.expect("stop only the test Bridge");
    let output = child
        .wait_with_output()
        .await
        .expect("reap the test Bridge");
    assert!(output.stderr.is_empty());
}
