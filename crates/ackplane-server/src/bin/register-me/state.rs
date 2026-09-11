use std::{fs, io::Write, path::Path};

use ackplane_protocol::v1;
use serde::{Deserialize, Serialize};
use tempfile::NamedTempFile;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub(super) struct SavedActivation {
    pub(super) signing_key_id: String,
    pub(super) enrolment_receipt_id: String,
}

#[derive(Deserialize, Serialize)]
pub(super) struct SavedRequest {
    pub(super) request_id: String,
    pub(super) tenant_id: String,
    pub(super) repository_id: String,
    pub(super) node_id: String,
    pub(super) public_key_fingerprint: String,
    pub(super) grpc_endpoint: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) activation_nonce: Option<Vec<u8>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) activation: Option<SavedActivation>,
}

impl SavedRequest {
    pub(super) fn load(path: &Path) -> Result<Self, String> {
        let bytes = fs::read(path).map_err(|error| format!("{}: {error}", path.display()))?;
        let saved: Self = serde_json::from_slice(&bytes)
            .map_err(|error| format!("{}: {error}", path.display()))?;
        if saved
            .activation_nonce
            .as_ref()
            .is_some_and(|nonce| nonce.len() != 32)
        {
            return Err(format!(
                "{}: recorded activation challenge is not a 32-byte nonce; restore the enrollment record",
                path.display()
            ));
        }
        if saved.activation.as_ref().is_some_and(|activation| {
            activation.signing_key_id.trim().is_empty()
                || activation.enrolment_receipt_id.trim().is_empty()
        }) {
            return Err(format!(
                "{}: recorded activation is incomplete; restore the enrollment record",
                path.display()
            ));
        }
        Ok(saved)
    }

    pub(super) fn save(&self, path: &Path) -> Result<(), String> {
        let parent = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        let write = || -> std::io::Result<()> {
            let bytes = serde_json::to_vec_pretty(self).map_err(std::io::Error::other)?;
            let mut temporary = NamedTempFile::new_in(parent)?;
            temporary.write_all(&bytes)?;
            temporary.as_file().sync_all()?;
            if self.activation.is_some() || self.activation_nonce.is_some() {
                temporary.persist(path).map_err(|error| error.error)?;
            } else {
                temporary
                    .persist_noclobber(path)
                    .map_err(|error| error.error)?;
            }
            Ok(())
        };
        write().map_err(|error| format!("could not save enrollment at {}: {error}", path.display()))
    }

    pub(super) fn record_activation(
        &mut self,
        path: &Path,
        result: &v1::EnrollmentActivationResult,
    ) -> Result<(), String> {
        if result.request_id != self.request_id
            || !matches!(
                result.state(),
                v1::EnrollmentState::Activating | v1::EnrollmentState::Active
            )
            || result.rejection_reason != 0
            || result.signing_key_id.trim().is_empty()
            || result.enrolment_receipt_id.trim().is_empty()
        {
            return Err(
                "activation did not return a matching accepted identity and receipt; saved enrollment is unchanged"
                    .to_string(),
            );
        }
        let activation = SavedActivation {
            signing_key_id: result.signing_key_id.clone(),
            enrolment_receipt_id: result.enrolment_receipt_id.clone(),
        };
        if let Some(recorded) = &self.activation {
            return if recorded == &activation {
                Ok(())
            } else {
                Err(
                    "activation differs from the recorded identity; saved enrollment is unchanged"
                        .to_string(),
                )
            };
        }
        let nonce = self.activation_nonce.take();
        self.activation = Some(activation);
        if let Err(error) = self.save(path) {
            self.activation_nonce = nonce;
            self.activation = None;
            return Err(error);
        }
        Ok(())
    }

    pub(super) fn record_activation_nonce(
        &mut self,
        path: &Path,
        nonce: Vec<u8>,
    ) -> Result<(), String> {
        if nonce.len() != 32 || self.activation.is_some() {
            return Err(
                "cannot record an invalid challenge or replace completed activation".to_string(),
            );
        }
        if self.activation_nonce.as_ref() == Some(&nonce) {
            return Ok(());
        }
        let previous = self.activation_nonce.replace(nonce);
        if let Err(error) = self.save(path) {
            self.activation_nonce = previous;
            return Err(error);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pending() -> SavedRequest {
        SavedRequest {
            request_id: "request-test".to_string(),
            tenant_id: "tenant-test".to_string(),
            repository_id: "repository-test".to_string(),
            node_id: "node-test".to_string(),
            public_key_fingerprint: "fingerprint-test".to_string(),
            grpc_endpoint: "http://127.0.0.1:8443".to_string(),
            activation_nonce: None,
            activation: None,
        }
    }

    fn response() -> v1::EnrollmentActivationResult {
        v1::EnrollmentActivationResult {
            request_id: "request-test".to_string(),
            state: v1::EnrollmentState::Activating as i32,
            signing_key_id: "key-test".to_string(),
            enrolment_receipt_id: "receipt-test".to_string(),
            ..Default::default()
        }
    }

    #[test]
    fn pending_and_activated_records_round_trip_without_private_key_material() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("enrollment.json");
        let mut saved = pending();
        saved.save(&path).unwrap();
        assert!(SavedRequest::load(&path).unwrap().activation.is_none());

        saved.record_activation(&path, &response()).unwrap();

        let loaded = SavedRequest::load(&path).unwrap();
        assert_eq!(loaded.activation, saved.activation);
        let json: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        assert_eq!(
            json,
            serde_json::json!({
                "request_id": "request-test",
                "tenant_id": "tenant-test",
                "repository_id": "repository-test",
                "node_id": "node-test",
                "public_key_fingerprint": "fingerprint-test",
                "grpc_endpoint": "http://127.0.0.1:8443",
                "activation": { "signing_key_id": "key-test", "enrolment_receipt_id": "receipt-test" },
            })
        );
        assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 1);
    }

    #[test]
    fn invalid_activation_never_replaces_the_pending_record() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("enrollment.json");
        let mut saved = pending();
        saved.save(&path).unwrap();
        let original = fs::read(&path).unwrap();
        for field in [
            "request",
            "state",
            "unknown-state",
            "key",
            "receipt",
            "rejection",
        ] {
            let mut result = response();
            match field {
                "request" => result.request_id = "another-request".to_string(),
                "state" => result.state = v1::EnrollmentState::Pending as i32,
                "unknown-state" => result.state = i32::MAX,
                "key" => result.signing_key_id = " ".to_string(),
                "receipt" => result.enrolment_receipt_id.clear(),
                "rejection" => result.rejection_reason = 1,
                _ => unreachable!(),
            }
            assert!(saved.record_activation(&path, &result).is_err());
            assert!(saved.activation.is_none());
            assert_eq!(fs::read(&path).unwrap(), original);
        }
    }

    #[test]
    fn a_new_request_or_different_activation_cannot_replace_recorded_enrollment() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("enrollment.json");
        let mut saved = pending();
        saved.save(&path).unwrap();
        saved.record_activation(&path, &response()).unwrap();
        let original = fs::read(&path).unwrap();

        assert!(pending().save(&path).is_err());
        saved.record_activation(&path, &response()).unwrap();
        let mut changed = response();
        changed.signing_key_id = "another-key".to_string();
        assert!(saved.record_activation(&path, &changed).is_err());
        assert_eq!(fs::read(&path).unwrap(), original);
    }

    #[test]
    fn a_failed_activation_write_does_not_claim_an_in_memory_success() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("enrollment.json");
        fs::create_dir(&path).unwrap();
        let mut saved = pending();

        assert!(saved.record_activation(&path, &response()).is_err());
        assert!(saved.activation.is_none());
        assert!(path.is_dir());
        assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 1);
    }

    #[test]
    fn incomplete_saved_activation_is_refused_instead_of_reported_as_enrolled() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("enrollment.json");
        for (key, receipt) in [("", "receipt-test"), ("key-test", " ")] {
            let mut saved = pending();
            saved.activation = Some(SavedActivation {
                signing_key_id: key.to_string(),
                enrolment_receipt_id: receipt.to_string(),
            });
            fs::write(&path, serde_json::to_vec(&saved).unwrap()).unwrap();

            let error = SavedRequest::load(&path)
                .err()
                .expect("must refuse incomplete activation");
            assert!(error.contains("recorded activation is incomplete"));
        }
    }

    #[test]
    fn activation_nonce_survives_restart_until_the_receipt_is_saved() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("enrollment.json");
        let mut saved = pending();
        saved.save(&path).unwrap();
        saved.record_activation_nonce(&path, vec![9; 32]).unwrap();
        let mut restarted = SavedRequest::load(&path).unwrap();
        assert_eq!(restarted.activation_nonce, Some(vec![9; 32]));
        assert!(restarted.activation.is_none());

        restarted
            .record_activation_nonce(&path, vec![10; 32])
            .unwrap();
        assert_eq!(
            SavedRequest::load(&path).unwrap().activation_nonce,
            Some(vec![10; 32])
        );
        restarted.record_activation(&path, &response()).unwrap();
        assert!(SavedRequest::load(&path)
            .unwrap()
            .activation_nonce
            .is_none());
        assert!(restarted
            .record_activation_nonce(&path, vec![11; 32])
            .is_err());
    }

    #[test]
    fn failed_challenge_and_receipt_writes_preserve_the_previous_attempt() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("enrollment.json");
        let mut saved = pending();
        saved.save(&path).unwrap();
        saved.record_activation_nonce(&path, vec![9; 32]).unwrap();
        let blocked = directory.path().join("directory-not-a-file");
        fs::create_dir(&blocked).unwrap();

        assert!(saved
            .record_activation_nonce(&blocked, vec![10; 32])
            .is_err());
        assert_eq!(saved.activation_nonce, Some(vec![9; 32]));
        assert!(saved.record_activation(&blocked, &response()).is_err());
        assert_eq!(saved.activation_nonce, Some(vec![9; 32]));
        assert!(saved.activation.is_none());
        assert_eq!(
            SavedRequest::load(&path).unwrap().activation_nonce,
            Some(vec![9; 32])
        );
        assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 2);
    }

    #[test]
    fn malformed_activation_nonces_are_refused_without_changing_the_record() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("enrollment.json");
        let mut saved = pending();
        for length in [0, 31, 33] {
            assert!(saved
                .record_activation_nonce(&path, vec![9; length])
                .is_err());
            assert!(saved.activation_nonce.is_none());
            let mut raw = serde_json::to_value(&saved).unwrap();
            raw["activation_nonce"] = serde_json::json!(vec![9; length]);
            let bytes = serde_json::to_vec(&raw).unwrap();
            fs::write(&path, &bytes).unwrap();
            assert!(SavedRequest::load(&path).is_err());
            assert_eq!(fs::read(&path).unwrap(), bytes);
        }
    }
}
