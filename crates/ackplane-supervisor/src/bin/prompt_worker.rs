use std::{
    env,
    error::Error,
    fs,
    path::Path,
    thread,
    time::{Duration, Instant},
};

fn main() -> Result<(), Box<dyn Error>> {
    let mut arguments = env::args().skip(1);
    let prompt = arguments.next().ok_or("missing prompt")?;
    let gate = arguments.next().ok_or("missing completion gate")?;
    let inherited_control_settings = env::vars_os().any(|(name, _)| {
        let name = name.to_string_lossy().to_ascii_uppercase();
        name.starts_with("ACKPLANE_") || name.starts_with("MINDLEAK_ACKPLANE_")
    });
    fs::write(
        "environment.json",
        serde_json::to_vec(
            &serde_json::json!({"inherited_control_settings":inherited_control_settings}),
        )?,
    )?;
    fs::write("prompt.json", prompt)?;
    let deadline = Instant::now() + Duration::from_secs(10);
    while !Path::new(&gate).exists() {
        if Instant::now() >= deadline {
            return Err("completion gate timed out".into());
        }
        thread::sleep(Duration::from_millis(5));
    }
    Ok(())
}
