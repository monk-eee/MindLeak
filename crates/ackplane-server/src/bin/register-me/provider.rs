use super::*;

pub(super) fn candidate(
    tenant: &str,
    repository: &str,
    node: &str,
    directory: &Path,
) -> Result<CredentialCandidate, String> {
    let candidate = match std::fs::symlink_metadata(directory.join("enrolment.json")) {
        Ok(_) => CredentialCandidate::recover(tenant, repository, directory),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            match std::fs::symlink_metadata(state_path(directory)) {
                Ok(_) => return Err("saved enrollment exists but the provider record is missing; restore the original provider".to_string()),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(format!("could not inspect enrollment state: {error}")),
            }
            CredentialCandidate::provision(tenant, repository, node, directory)
        }
        Err(error) => return Err(format!("could not inspect provider state: {error}")),
    }.map_err(|error| error.to_string())?;
    if candidate.identity().node_id != node {
        return Err(
            "the provider belongs to another node; existing identity is unchanged".to_string(),
        );
    }
    Ok(candidate)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn candidate_refuses_a_missing_provider_with_enrollment_state() {
        let directory = tempfile::tempdir().unwrap();
        let path = state_path(directory.path());
        std::fs::write(&path, b"saved enrollment").unwrap();
        assert!(candidate("tenant", "repo", "node", directory.path()).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), b"saved enrollment");
        assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 1);
    }

    #[test]
    fn candidate_preserves_a_corrupt_or_unreadable_provider_record() {
        for corrupt in [true, false] {
            let directory = tempfile::tempdir().unwrap();
            let path = directory.path().join("enrolment.json");
            if corrupt {
                std::fs::write(&path, b"damaged identity").unwrap();
            } else {
                std::fs::create_dir(&path).unwrap();
            }
            assert!(candidate("tenant", "repo", "node", directory.path()).is_err());
            if corrupt {
                assert_eq!(std::fs::read(&path).unwrap(), b"damaged identity");
            } else {
                assert!(path.is_dir());
            }
        }
    }
}
