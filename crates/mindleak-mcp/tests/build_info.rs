use std::path::{Path, PathBuf};
use std::process::Command;

#[test]
fn build_identity_and_watch_paths_ignore_foreign_git_pointers() {
    let root = tempfile::tempdir().unwrap();
    let candidate = root.path().join("candidate");
    let foreign = root.path().join("foreign");
    let pointers = [
        "GIT_DIR",
        "GIT_WORK_TREE",
        "GIT_COMMON_DIR",
        "GIT_INDEX_FILE",
        "GIT_OBJECT_DIRECTORY",
        "GIT_ALTERNATE_OBJECT_DIRECTORIES",
    ];
    let git = |directory: &Path, args: &[&str]| {
        let mut command = Command::new("git");
        command.arg("-C").arg(directory).args(args);
        for name in pointers {
            command.env_remove(name);
        }
        let output = command.output().unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).unwrap().trim().to_string()
    };
    for directory in [&candidate, &foreign] {
        std::fs::create_dir_all(directory.join("crates/mindleak-mcp")).unwrap();
        std::fs::write(
            directory.join("input.txt"),
            directory.to_string_lossy().as_bytes(),
        )
        .unwrap();
        git(directory, &["init", "--quiet", "--initial-branch=main"]);
        git(directory, &["add", "input.txt"]);
        git(
            directory,
            &[
                "-c",
                "user.name=Build Test",
                "-c",
                "user.email=build@example.invalid",
                "-c",
                "commit.gpgsign=false",
                "commit",
                "--quiet",
                "-m",
                "fixture",
            ],
        );
    }
    let revision = git(&candidate, &["rev-parse", "HEAD"]);
    assert_ne!(revision, git(&foreign, &["rev-parse", "HEAD"]));
    let executable = root.path().join(if cfg!(windows) {
        "build-info-probe.exe"
    } else {
        "build-info-probe"
    });
    let compiler = std::env::var_os("RUSTC").unwrap_or_else(|| "rustc".into());
    let compiled = Command::new(compiler)
        .arg("--edition=2021")
        .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("build.rs"))
        .arg("-o")
        .arg(&executable)
        .output()
        .unwrap();
    assert!(
        compiled.status.success(),
        "{}",
        String::from_utf8_lossy(&compiled.stderr)
    );
    let metadata = foreign.join(".git");
    let invoke = |overridden: Option<&str>| {
        let mut command = Command::new(&executable);
        command
            .current_dir(root.path())
            .env("CARGO_MANIFEST_DIR", candidate.join("crates/mindleak-mcp"))
            .env_remove("MINDLEAK_BUILD_SHA")
            .env("GIT_DIR", &metadata)
            .env("GIT_WORK_TREE", &foreign)
            .env("GIT_COMMON_DIR", &metadata)
            .env("GIT_INDEX_FILE", metadata.join("index"))
            .env("GIT_OBJECT_DIRECTORY", metadata.join("objects"))
            .env("GIT_ALTERNATE_OBJECT_DIRECTORIES", metadata.join("objects"));
        if let Some(value) = overridden {
            command.env("MINDLEAK_BUILD_SHA", value);
        }
        let output = command.output().unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let output = String::from_utf8(output.stdout).unwrap();
        let identity = output
            .lines()
            .find_map(|line| line.strip_prefix("cargo:rustc-env=MINDLEAK_BUILD_SHA="))
            .unwrap()
            .to_string();
        let watched: Vec<PathBuf> = output
            .lines()
            .filter_map(|line| line.strip_prefix("cargo:rerun-if-changed="))
            .map(PathBuf::from)
            .collect();
        (identity, watched)
    };

    // Foreign Git pointers changed both the embedded revision and the files Cargo watched.
    for (overridden, expected) in [
        (None, &revision[..12]),
        (Some("ABCDEF0123456789"), "abcdef012345"),
        (Some("not-a-revision"), &revision[..12]),
    ] {
        let (identity, watched) = invoke(overridden);
        assert_eq!(identity, expected);
        assert_eq!(watched.len(), 2);
        for file in watched {
            assert!(file
                .canonicalize()
                .unwrap()
                .starts_with(candidate.canonicalize().unwrap()));
        }
    }
    std::fs::remove_dir_all(candidate.join(".git")).unwrap();
    let (identity, watched) = invoke(None);
    assert_eq!(identity, "unknown");
    assert!(watched.is_empty());
}
