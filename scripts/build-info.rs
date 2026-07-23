use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;

pub fn emit_git_sha() {
    println!("cargo:rerun-if-env-changed=MINDLEAK_BUILD_SHA");

    let manifest_dir = PathBuf::from(
        env::var_os("CARGO_MANIFEST_DIR").expect("Cargo must set CARGO_MANIFEST_DIR"),
    );
    let repo_root = manifest_dir
        .parent()
        .and_then(Path::parent)
        .expect("MCP crates must live under the workspace crates directory");

    watch_git_path(repo_root, "HEAD");
    if let Some(reference) = git_output(repo_root, &["symbolic-ref", "-q", "HEAD"]) {
        watch_git_path(repo_root, &reference);
    }

    let sha = env::var("MINDLEAK_BUILD_SHA")
        .ok()
        .and_then(|value| normalize_sha(&value))
        .or_else(|| {
            git_output(repo_root, &["rev-parse", "HEAD"]).and_then(|value| normalize_sha(&value))
        })
        .unwrap_or_else(|| "unknown".to_string());

    println!("cargo:rustc-env=MINDLEAK_BUILD_SHA={sha}");
}

fn watch_git_path(repo_root: &Path, name: &str) {
    if let Some(path) = git_output(repo_root, &["rev-parse", "--git-path", name]) {
        let path = PathBuf::from(path);
        let path = if path.is_absolute() {
            path
        } else {
            repo_root.join(path)
        };
        println!("cargo:rerun-if-changed={}", path.display());
    }
}

fn git_output(repo_root: &Path, args: &[&str]) -> Option<String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(repo_root)
        .args(args)
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn normalize_sha(value: &str) -> Option<String> {
    let value = value.trim();
    (value.len() >= 7 && value.bytes().all(|byte| byte.is_ascii_hexdigit()))
        .then(|| value[..value.len().min(12)].to_ascii_lowercase())
}
