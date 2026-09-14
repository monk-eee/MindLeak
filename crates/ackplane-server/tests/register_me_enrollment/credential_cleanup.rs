use std::{
    io::Read,
    path::Path,
    process::{Command, Stdio},
    time::{Duration, Instant},
};

const DIRECTORY_ENV: &str = "MINDLEAK_ENROLLMENT_CLEANUP_DIRECTORY";
const COMPLETE: &str = "mindleak-test-credential-cleanup-complete";

pub(super) fn remove(directory: &Path) -> Result<(), String> {
    let executable = std::env::current_exe()
        .map_err(|_| "could not locate the test credential cleanup executable".to_string())?;
    run(
        Command::new(executable)
            .args([
                "--exact",
                "support::credential_cleanup::exact_credential_cleanup_child",
                "--nocapture",
            ])
            .env(DIRECTORY_ENV, directory)
            .stdin(Stdio::null())
            .stderr(Stdio::inherit()),
        Duration::from_secs(10),
    )
}

fn run(command: &mut Command, timeout: Duration) -> Result<(), String> {
    let started = Instant::now();
    let mut child = command
        .stdout(Stdio::piped())
        .spawn()
        .map_err(|_| "could not start the test credential cleanup child".to_string())?;
    let failure = loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                if !status.success() {
                    return Err("the test credential cleanup child failed".to_string());
                }
                let mut output = String::new();
                child
                    .stdout
                    .take()
                    .ok_or("test credential cleanup confirmation pipe missing")?
                    .take(8192)
                    .read_to_string(&mut output)
                    .map_err(|_| "could not read test credential cleanup confirmation")?;
                return output
                    .lines()
                    .any(|line| line == COMPLETE)
                    .then_some(())
                    .ok_or_else(|| {
                        "the child did not confirm exact credential cleanup".to_string()
                    });
            }
            Ok(None) => {}
            Err(_) => break "could not inspect the test credential cleanup child".to_string(),
        }
        let remaining = timeout.saturating_sub(started.elapsed());
        if remaining.is_zero() {
            break format!("test credential cleanup timed out after {timeout:?}");
        }
        std::thread::sleep(remaining.min(Duration::from_millis(5)));
    };
    if child.kill().is_err() && child.try_wait().ok().flatten().is_none() {
        return Err(format!("{failure}; could not terminate the cleanup child"));
    }
    child
        .wait()
        .map_err(|_| format!("{failure}; could not reap the cleanup child"))?;
    Err(failure)
}

#[test]
fn exact_credential_cleanup_child() {
    let Some(directory) = std::env::var_os(DIRECTORY_ENV) else {
        return;
    };
    remove_in_process(Path::new(&directory)).unwrap();
    println!("\n{COMPLETE}");
}

fn remove_in_process(directory: &Path) -> Result<(), String> {
    let bytes = match std::fs::read(directory.join("enrolment.json")) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(_) => return Err("could not read test credential metadata for cleanup".to_string()),
    };
    let record: ackplane_node::EnrolmentRecord = serde_json::from_slice(&bytes)
        .map_err(|_| "invalid test credential metadata for cleanup".to_string())?;
    let handle = record
        .provider_handle
        .as_deref()
        .ok_or("test credential handle missing")?;
    if record.provider_scheme != "credential-facility-software"
        || handle.len() != 32
        || !handle.bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        return Err("refusing cleanup of an unexpected test credential".to_string());
    }
    let service = "mindleak-ackplane-node-software-v1";
    #[cfg(target_os = "macos")]
    {
        use security_framework::item::{ItemClass, ItemSearchOptions, Reference, SearchResult};

        let mut query = ItemSearchOptions::new();
        query
            .class(ItemClass::generic_password())
            .service(service)
            .account(handle)
            .load_refs(true);
        let items = match query.search() {
            Ok(items) => items,
            Err(error) if error.code() == -25300 => return Ok(()),
            Err(error) => {
                return Err(format!(
                    "test Keychain reference lookup failed ({})",
                    error.code()
                ))
            }
        };
        for item in items {
            match item {
                SearchResult::Ref(Reference::KeychainItem(item)) => item.delete(),
                _ => return Err("unexpected test Keychain reference type".to_string()),
            }
        }
        match query.search() {
            Err(error) if error.code() == -25300 => Ok(()),
            Err(error) => Err(format!(
                "test Keychain cleanup verification failed ({})",
                error.code()
            )),
            Ok(_) => Err("test Keychain entry remains after deletion".to_string()),
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        match keyring::Entry::new(service, handle).and_then(|entry| entry.delete_password()) {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(_) => Err("could not remove the exact test credential entry".to_string()),
        }
    }
}

#[path = "credential_cleanup_tests.rs"]
mod tests;
