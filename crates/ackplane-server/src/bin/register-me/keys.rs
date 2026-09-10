use std::{
    fs,
    io::{self, Write},
    path::Path,
};

use ed25519_dalek::SigningKey;
use tempfile::NamedTempFile;

pub(super) fn load_key(path: &Path) -> io::Result<SigningKey> {
    let bytes = fs::read(path)?;
    let seed = <[u8; 32]>::try_from(bytes.as_slice()).map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            "existing key is not a 32-byte Ed25519 seed; restore the original key, which has not been replaced",
        )
    })?;
    Ok(SigningKey::from_bytes(&seed))
}

pub(super) fn load_or_generate_key(path: &Path) -> io::Result<SigningKey> {
    match load_key(path) {
        Ok(key) => return Ok(key),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    match fs::symlink_metadata(super::state_path(path)) {
        Ok(_) => {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                "enrollment state exists but its key is missing; restore the original key instead of generating a replacement",
            ));
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)?;
    let mut seed = [0_u8; 32];
    getrandom::getrandom(&mut seed)
        .map_err(|error| io::Error::other(format!("could not generate a key: {error}")))?;
    let mut temporary = NamedTempFile::new_in(parent)?;
    temporary.write_all(&seed)?;
    temporary.as_file().sync_all()?;
    match temporary.persist_noclobber(path) {
        Ok(_) => Ok(SigningKey::from_bytes(&seed)),
        Err(error) if error.error.kind() == io::ErrorKind::AlreadyExists => load_key(path),
        Err(error) => Err(error.error),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn load_or_generate_key_refuses_a_missing_key_with_enrollment_state() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("node.key");
        let state = super::super::state_path(&path);
        fs::write(&state, b"existing enrollment state").unwrap();

        let error = load_or_generate_key(&path).expect_err("must refuse");

        assert_eq!(error.kind(), io::ErrorKind::NotFound);
        assert!(!path.exists());
        assert_eq!(fs::read(&state).unwrap(), b"existing enrollment state");
    }

    #[test]
    fn load_or_generate_key_reuses_the_winner_of_concurrent_creation() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("nested/node.key");
        let barrier = std::sync::Barrier::new(8);
        let seeds = std::thread::scope(|scope| {
            let workers = (0..8)
                .map(|_| {
                    scope.spawn(|| {
                        barrier.wait();
                        load_or_generate_key(&path).unwrap().to_bytes()
                    })
                })
                .collect::<Vec<_>>();
            workers
                .into_iter()
                .map(|worker| worker.join().unwrap())
                .collect::<Vec<_>>()
        });

        let stored = load_key(&path).unwrap().to_bytes();
        assert!(seeds.iter().all(|seed| seed == &stored));
        assert_eq!(fs::read_dir(path.parent().unwrap()).unwrap().count(), 1);
    }

    #[test]
    fn load_or_generate_key_preserves_an_unreadable_directory() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("node.key");
        fs::create_dir(&path).unwrap();

        assert!(load_or_generate_key(&path).is_err());
        assert!(path.is_dir());
    }

    #[cfg(unix)]
    #[test]
    fn a_new_key_is_owner_only() {
        use std::os::unix::fs::PermissionsExt;

        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("node.key");
        load_or_generate_key(&path).unwrap();

        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
}
