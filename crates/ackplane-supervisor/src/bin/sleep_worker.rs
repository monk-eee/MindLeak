//! Test fixture only: not part of the public crate surface. Sleeps for a
//! caller-specified duration in milliseconds (default 30 seconds) so
//! `worker_adapter` tests can observe a real, deterministic, cross-platform
//! child process without shell-specific plumbing (AGENTS.md toolchain
//! discipline).

fn main() -> Result<(), Box<dyn std::error::Error>> {
    use std::io::{Read, Write};

    let mut arguments = std::env::args().skip(1);
    let mode = arguments.next().unwrap_or_else(|| "30000".into());
    if mode == "--spawn-descendant" {
        let address = arguments.next().ok_or("missing fixture address")?;
        let child = std::process::Command::new(std::env::current_exe()?)
            .args(["--descendant", &address])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()?;
        drop(child);
        return Ok(());
    }
    if mode == "--descendant" {
        let address = arguments.next().ok_or("missing fixture address")?;
        let mut stream = std::net::TcpStream::connect(address)?;
        stream.set_read_timeout(Some(std::time::Duration::from_secs(10)))?;
        stream.write_all(b"R")?;
        let mut byte = [0_u8; 1];
        while stream.read(&mut byte)? > 0 {
            stream.write_all(&byte)?;
        }
        return Ok(());
    }
    let millis: u64 = mode.parse().unwrap_or(30_000);
    std::thread::sleep(std::time::Duration::from_millis(millis));
    Ok(())
}
