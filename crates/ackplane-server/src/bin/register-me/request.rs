use std::{fs, io::Write, path::Path};

use ackplane_node::CandidateIdentity;
use ackplane_protocol::{enrollment::public_key_fingerprint, v1};
use serde::{Deserialize, Serialize};
use time::{format_description::well_known::Rfc3339, Duration, OffsetDateTime};

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct SavedRequest {
    pub(super) request_id: String,
    pub(super) tenant_id: String,
    pub(super) repository_id: String,
    pub(super) node_id: String,
    pub(super) public_key: Vec<u8>,
    pub(super) public_key_fingerprint: String,
    pub(super) grpc_endpoint: String,
    pub(super) display_name: String,
    pub(super) requested_capabilities: Vec<String>,
    pub(super) created_at: String,
    pub(super) expires_at: String,
}

impl SavedRequest {
    pub(super) fn new(
        tenant_id: &str,
        repository_id: &str,
        identity: &CandidateIdentity,
        endpoint: &str,
        display_name: String,
        requested_capabilities: Vec<String>,
    ) -> Result<Self, String> {
        let now = OffsetDateTime::now_utc();
        Ok(Self {
            request_id: format!("request-{}", identity.fingerprint),
            tenant_id: tenant_id.to_string(),
            repository_id: repository_id.to_string(),
            node_id: identity.node_id.clone(),
            public_key: identity.public_key.to_vec(),
            public_key_fingerprint: identity.fingerprint.clone(),
            grpc_endpoint: endpoint.to_string(),
            display_name,
            requested_capabilities,
            created_at: now.format(&Rfc3339).map_err(|error| error.to_string())?,
            expires_at: (now + Duration::days(7))
                .format(&Rfc3339)
                .map_err(|error| error.to_string())?,
        })
    }

    pub(super) fn load(path: &Path) -> Result<Self, String> {
        let bytes = fs::read(path).map_err(|error| {
            format!(
                "could not read saved request at {}: {error}",
                path.display()
            )
        })?;
        let saved: Self = serde_json::from_slice(&bytes)
            .map_err(|error| format!("invalid saved request at {}: {error}", path.display()))?;
        validate_endpoint(&saved.grpc_endpoint)?;
        if [
            &saved.request_id,
            &saved.tenant_id,
            &saved.repository_id,
            &saved.node_id,
            &saved.display_name,
        ]
        .iter()
        .any(|value| value.trim().is_empty())
            || saved.public_key.len() != 32
            || public_key_fingerprint(&saved.public_key) != saved.public_key_fingerprint
            || saved
                .requested_capabilities
                .iter()
                .any(|value| value.trim().is_empty())
            || OffsetDateTime::parse(&saved.created_at, &Rfc3339).is_err()
            || OffsetDateTime::parse(&saved.expires_at, &Rfc3339).is_err()
        {
            return Err("saved enrollment request is incomplete or mismatched; restore it instead of replacing it".to_string());
        }
        Ok(saved)
    }

    pub(super) fn save(&self, path: &Path) -> Result<(), String> {
        let parent = path
            .parent()
            .ok_or("saved request must have a parent directory")?;
        let write = || -> std::io::Result<()> {
            let bytes = serde_json::to_vec_pretty(self).map_err(std::io::Error::other)?;
            let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
            temporary.write_all(&bytes)?;
            temporary.as_file().sync_all()?;
            temporary
                .persist_noclobber(path)
                .map_err(|error| error.error)?;
            Ok(())
        };
        write().map_err(|error| {
            format!(
                "could not save enrollment request at {}: {error}",
                path.display()
            )
        })
    }

    pub(super) fn ensure_identity(&self, identity: &CandidateIdentity) -> Result<(), String> {
        if self.node_id != identity.node_id
            || self.public_key != identity.public_key
            || self.public_key_fingerprint != identity.fingerprint
        {
            return Err("provider does not match the saved enrollment fingerprint and node; restore the approved identity".to_string());
        }
        Ok(())
    }

    pub(super) fn ensure_matches(&self, requested: &Self) -> Result<(), String> {
        let mut comparison = requested.clone();
        comparison.created_at = self.created_at.clone();
        comparison.expires_at = self.expires_at.clone();
        if self != &comparison {
            return Err("saved enrollment request differs from these parameters; existing identity and request are unchanged".to_string());
        }
        Ok(())
    }

    pub(super) fn request(&self) -> v1::EnrollmentRequest {
        v1::EnrollmentRequest {
            request_id: self.request_id.clone(),
            tenant_id: self.tenant_id.clone(),
            repository_id: self.repository_id.clone(),
            proposed_node_id: self.node_id.clone(),
            public_key: self.public_key.clone(),
            public_key_fingerprint: self.public_key_fingerprint.clone(),
            display_name: self.display_name.clone(),
            requested_capabilities: self.requested_capabilities.clone(),
            created_at: self.created_at.clone(),
            expires_at: self.expires_at.clone(),
        }
    }
}

pub(super) fn validate_endpoint(endpoint: &str) -> Result<(), String> {
    let uri: tonic::codegen::http::Uri = endpoint
        .parse()
        .map_err(|_| "invalid enrollment endpoint".to_string())?;
    if !matches!(uri.scheme_str(), Some("http" | "https"))
        || uri.host().is_none()
        || uri
            .authority()
            .is_some_and(|authority| authority.as_str().contains('@'))
        || uri.query().is_some()
        || !matches!(uri.path(), "" | "/")
    {
        return Err(
            "enrollment endpoint must be an http(s) authority without credentials, path or query"
                .to_string(),
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests;
