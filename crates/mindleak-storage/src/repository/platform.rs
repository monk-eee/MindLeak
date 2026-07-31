//! Platform-specific resolution of the local, non-roaming state root.

use std::path::PathBuf;

use super::RepositoryStorageError;

#[derive(Debug, Clone, Copy)]
pub(crate) enum Platform {
    Windows,
    MacOs,
    Unix,
}

impl Platform {
    pub(crate) fn current() -> Self {
        if cfg!(target_os = "windows") {
            Self::Windows
        } else if cfg!(target_os = "macos") {
            Self::MacOs
        } else {
            Self::Unix
        }
    }
}

pub(crate) fn platform_state_root_from<F>(
    environment: F,
    platform: Platform,
) -> Result<PathBuf, RepositoryStorageError>
where
    F: Fn(&str) -> Option<String>,
{
    let nonblank = |name: &str| {
        environment(name)
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
    };
    match platform {
        Platform::Windows => nonblank("LOCALAPPDATA")
            .map(PathBuf::from)
            .map(|path| path.join("MindLeak")),
        Platform::MacOs => nonblank("HOME").map(PathBuf::from).map(|path| {
            path.join("Library")
                .join("Application Support")
                .join("MindLeak")
        }),
        Platform::Unix => nonblank("XDG_STATE_HOME")
            .map(PathBuf::from)
            .map(|path| path.join("mindleak"))
            .or_else(|| {
                nonblank("HOME")
                    .map(PathBuf::from)
                    .map(|path| path.join(".local").join("state").join("mindleak"))
            }),
    }
    .ok_or(RepositoryStorageError::MissingStateRoot)
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::path::PathBuf;

    use super::{platform_state_root_from, Platform};

    #[test]
    fn platform_roots_are_local_non_roaming_and_overrideable() {
        let values = HashMap::from([
            ("LOCALAPPDATA", "C:/Users/test/AppData/Local"),
            ("HOME", "/home/test"),
            ("XDG_STATE_HOME", "/state"),
        ]);
        let env = |name: &str| values.get(name).map(|value| value.to_string());

        assert_eq!(
            platform_state_root_from(env, Platform::Windows).unwrap(),
            PathBuf::from("C:/Users/test/AppData/Local").join("MindLeak")
        );
        assert_eq!(
            platform_state_root_from(env, Platform::MacOs).unwrap(),
            PathBuf::from("/home/test/Library/Application Support/MindLeak")
        );
        assert_eq!(
            platform_state_root_from(env, Platform::Unix).unwrap(),
            PathBuf::from("/state/mindleak")
        );
    }
}
