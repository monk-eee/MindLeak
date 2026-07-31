//! Filesystem and git-process helpers shared across repository resolution.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;

pub(crate) const GIT_REPOSITORY_ENV: [&str; 6] = [
    "GIT_DIR",
    "GIT_WORK_TREE",
    "GIT_COMMON_DIR",
    "GIT_INDEX_FILE",
    "GIT_OBJECT_DIRECTORY",
    "GIT_ALTERNATE_OBJECT_DIRECTORIES",
];

pub(crate) fn nonblank_path(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|value| !value.is_empty())
}

pub(crate) fn git_command() -> Command {
    let mut command = Command::new("git");
    for variable in GIT_REPOSITORY_ENV {
        command.env_remove(variable);
    }
    command
}

pub(crate) fn sibling_path(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(suffix);
    path.with_file_name(name)
}

#[cfg(unix)]
pub(crate) fn create_private_directory(path: &Path) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;

    fs::create_dir_all(path)?;
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))
}

#[cfg(not(unix))]
pub(crate) fn create_private_directory(path: &Path) -> io::Result<()> {
    fs::create_dir_all(path)
}
