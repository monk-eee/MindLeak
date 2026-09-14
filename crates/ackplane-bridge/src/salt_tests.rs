use std::{fs, io, sync::Barrier, thread};

use super::{development_tenant_token, load_or_generate_salt};

// An existing empty salt was overwritten, silently selecting a new tenant identity.
#[test]
fn an_empty_existing_salt_is_refused_without_replacement() {
    let directory = tempfile::tempdir().expect("create an owned salt fixture");
    let path = directory.path().join("salt.bin");
    fs::write(&path, []).expect("create an empty salt file");

    let result = load_or_generate_salt(&path);

    assert!(
        matches!(result, Err(error) if error.kind() == io::ErrorKind::InvalidData),
        "an existing empty salt must be refused, not regenerated"
    );
    assert!(fs::read(&path).expect("read unchanged fixture").is_empty());
}

// A read failure was treated as absence and could overwrite writable identity state.
#[cfg(unix)]
#[test]
fn an_unreadable_existing_salt_is_refused_without_replacement() {
    use std::os::unix::fs::PermissionsExt;

    let directory = tempfile::tempdir().expect("create an owned salt fixture");
    let path = directory.path().join("salt.bin");
    let original = b"existing-test-salt-do-not-replace";
    fs::write(&path, original).expect("create the original salt");
    fs::set_permissions(&path, fs::Permissions::from_mode(0o200))
        .expect("make only the test salt unreadable but writable");
    match fs::read(&path) {
        Err(error) if error.kind() == io::ErrorKind::PermissionDenied => {}
        Ok(_) => {
            eprintln!("skipping: this user or filesystem bypasses read permission checks");
            return;
        }
        Err(error) => panic!("unexpected fixture read error: {error}"),
    }

    let result = load_or_generate_salt(&path);
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600))
        .expect("restore test salt read permission");
    let after = fs::read(&path).expect("read the original salt after refusal");

    assert!(
        matches!(result, Err(error) if error.kind() == io::ErrorKind::PermissionDenied),
        "an unreadable existing salt must be refused, not regenerated"
    );
    assert_eq!(after, original);
}

#[test]
fn a_valid_existing_salt_and_its_tenant_identity_are_preserved() {
    let directory = tempfile::tempdir().expect("create an owned salt fixture");
    let path = directory.path().join("salt.bin");
    let original = b"a-valid-existing-salt-of-a-different-length";
    fs::write(&path, original).expect("create the original salt");
    let original_identity = development_tenant_token(original, "tenant");

    let loaded = load_or_generate_salt(&path).expect("reuse the original salt");

    assert_eq!(loaded, original);
    assert_eq!(
        development_tenant_token(&loaded, "tenant"),
        original_identity
    );
    assert_eq!(fs::read(&path).expect("read persisted salt"), original);
    assert_eq!(
        fs::read_dir(directory.path())
            .expect("list fixture")
            .count(),
        1
    );
}

#[test]
fn first_creation_publishes_a_complete_salt_in_a_missing_directory() {
    let directory = tempfile::tempdir().expect("create an owned salt fixture");
    let parent = directory.path().join("nested").join("configuration");
    let path = parent.join("salt.bin");

    let created = load_or_generate_salt(&path).expect("create the first salt");

    assert_eq!(created.len(), 32);
    assert_eq!(fs::read(&path).expect("read complete salt"), created);
    assert_eq!(load_or_generate_salt(&path).expect("reload salt"), created);
    assert_eq!(fs::read_dir(parent).expect("list fixture").count(), 1);
}

// Concurrent first starts could each return different salts and overwrite the winner.
#[test]
fn concurrent_first_starts_share_one_complete_persisted_salt() {
    let directory = tempfile::tempdir().expect("create an owned salt fixture");
    let path = directory.path().join("salt.bin");
    let barrier = Barrier::new(16);

    let salts = thread::scope(|scope| {
        let readers = (0..16)
            .map(|_| {
                scope.spawn(|| {
                    barrier.wait();
                    load_or_generate_salt(&path).expect("load the shared first salt")
                })
            })
            .collect::<Vec<_>>();
        readers
            .into_iter()
            .map(|reader| reader.join().expect("salt caller completes"))
            .collect::<Vec<_>>()
    });

    let persisted = fs::read(&path).expect("read the winning salt");
    assert_eq!(persisted.len(), 32);
    assert!(
        salts.iter().all(|salt| salt == &persisted),
        "every caller must use the same complete salt that remains on disk"
    );
    assert_eq!(
        fs::read_dir(directory.path())
            .expect("list fixture")
            .count(),
        1
    );
}

// Windows reports a file-parent read as NotFound; directory creation must not
// replace that original error with AlreadyExists or attempt to create an identity.
#[test]
fn non_file_targets_propagate_read_errors_without_creating_salt_files() {
    let directory = tempfile::tempdir().expect("create an owned salt fixture");
    let file = directory.path().join("regular-file");
    fs::write(&file, b"unchanged fixture").expect("create a non-directory parent");
    for path in [
        directory.path().to_path_buf(),
        file.join("salt.bin"),
        file.join("nested").join("salt.bin"),
        file.join("nested").join("deeper").join("salt.bin"),
    ] {
        let original_error = fs::read(&path).expect_err("the fixture cannot be read as a salt");
        let error = load_or_generate_salt(&path).expect_err("invalid target must refuse");
        assert_eq!(error.kind(), original_error.kind());
        assert_eq!(error.raw_os_error(), original_error.raw_os_error());
    }
    assert_eq!(
        fs::read(&file).expect("read unchanged parent"),
        b"unchanged fixture"
    );
    assert_eq!(
        fs::read_dir(directory.path())
            .expect("list fixture")
            .count(),
        1
    );
}

#[cfg(unix)]
#[test]
fn a_dangling_symlink_is_not_permission_to_create_a_replacement_identity() {
    use std::os::unix::fs::symlink;

    let directory = tempfile::tempdir().expect("create an owned salt fixture");
    let target = directory.path().join("missing-original-salt");
    let path = directory.path().join("salt.bin");
    symlink(&target, &path).expect("create a link to a missing salt");

    assert!(load_or_generate_salt(&path).is_err());
    assert!(!target.exists());
    assert_eq!(fs::read_link(&path).expect("original link remains"), target);
    assert_eq!(
        fs::read_dir(directory.path())
            .expect("list fixture")
            .count(),
        1
    );
}

#[cfg(unix)]
#[test]
fn newly_created_salts_are_not_group_or_world_accessible() {
    use std::os::unix::fs::PermissionsExt;

    let directory = tempfile::tempdir().expect("create an owned salt fixture");
    let path = directory.path().join("salt.bin");

    load_or_generate_salt(&path).expect("create a private salt");

    let mode = fs::metadata(&path)
        .expect("read salt permissions")
        .permissions()
        .mode();
    assert_eq!(mode & 0o077, 0);
}
