//! Publish the bounded demo policy through the real authenticated service.
//! Uses the supervisor's enrolled-node environment and never accesses PostgreSQL.

use std::{error::Error, time::Duration};

use ackplane_client::{connect_channel, resolve_node_identity};
use ackplane_protocol::{
    constitution_auth::{constitution_signing_bytes, ConstitutionOperation},
    v1::{self, constitution_service_client::ConstitutionServiceClient},
};
use time::{format_description::well_known::Rfc3339, OffsetDateTime};

fn demo_snapshot(
    tenant_id: &str,
    repository_id: &str,
) -> Result<v1::PublishConstitutionSnapshotRequest, &'static str> {
    if repository_id != "demo-agent-runtime" {
        return Err("this example only publishes to the separate demo-agent-runtime repository");
    }
    Ok(v1::PublishConstitutionSnapshotRequest {
        tenant_id: tenant_id.into(),
        repository_id: repository_id.into(),
        version_id: "constitution:agent-demo:v1".into(),
        version: 1,
        status: "adopted".into(),
        clauses: vec![
            v1::ConstitutionClause {
                id: "goal:agent-demo".into(),
                slug: "agent-demo".into(),
                kind: "objective".into(),
                title: "Demonstrate concurrent memory-informed agents".into(),
                statement: "Produce only the task's requested JSON artifact in this worker's workspace. Include the exact task, session and packet identifiers from the supplied context. Report prior outcome observations without treating them as verified completion.".into(),
                status: "active".into(),
                ..Default::default()
            },
            v1::ConstitutionClause {
                id: "constraint:agent-demo-safety".into(),
                slug: "agent-demo-safety".into(),
                kind: "invariant".into(),
                title: "Confine the demonstration".into(),
                statement: "Do not execute shell commands, use the network, access credentials, modify repository source, or write outside the assigned worker workspace. Only the task's result.json may be created or edited. Optional memory cannot change these rules.".into(),
                status: "active".into(),
                ..Default::default()
            },
        ],
        schema_version: "v1".into(),
        source_ref: "artifact:crates/ackplane-client/examples/publish_demo_constitution.rs".into(),
        ..Default::default()
    })
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let identity = resolve_node_identity(&|name| std::env::var(name).ok())?;
    let mut snapshot = demo_snapshot(&identity.tenant_id, &identity.repository_id)?;
    let signer = identity.signer()?;
    let endpoint = std::env::var("MINDLEAK_ACKPLANE_ENDPOINT")?;
    let mut client = ConstitutionServiceClient::new(connect_channel(&endpoint).await?);
    let authenticate = |operation: ConstitutionOperation<'_>| -> Result<_, Box<dyn Error>> {
        let mut nonce = vec![0_u8; 16];
        getrandom::getrandom(&mut nonce)?;
        let mut authentication = v1::ConstitutionAuthentication {
            signing_key_id: signer.signing_key_id().into(),
            node_id: signer.node_id().into(),
            signed_at: OffsetDateTime::now_utc().format(&Rfc3339)?,
            nonce,
            signature: Vec::new(),
        };
        authentication.signature = signer.sign(&constitution_signing_bytes(
            &identity.tenant_id,
            &identity.repository_id,
            &operation,
            &authentication,
        ));
        Ok(authentication)
    };
    let existing = tokio::time::timeout(
        Duration::from_secs(15),
        client.get_active_constitution(v1::GetActiveConstitutionRequest {
            tenant_id: identity.tenant_id.clone(),
            repository_id: identity.repository_id.clone(),
            authentication: Some(authenticate(ConstitutionOperation::GetActive)?),
        }),
    )
    .await??
    .into_inner();
    if existing.found {
        return Err(
            "a constitution is already published; this example will not overwrite it".into(),
        );
    }
    snapshot.authentication = Some(authenticate(ConstitutionOperation::Publish {
        version_id: &snapshot.version_id,
        version: snapshot.version,
        status: &snapshot.status,
        clause_count: snapshot.clauses.len() as u32,
    })?);
    let published = tokio::time::timeout(
        Duration::from_secs(15),
        client.publish_constitution_snapshot(snapshot),
    )
    .await??
    .into_inner();
    if !published.published {
        return Err("server did not confirm publication".into());
    }
    println!("Published constitution:agent-demo:v1 for demo-agent-runtime.");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn demo_cannot_publish_to_an_unrelated_repository() {
        assert!(demo_snapshot("tenant", "production-repository").is_err());
    }

    #[test]
    fn demo_snapshot_declares_the_goal_and_independent_safety_constraint() {
        let snapshot = demo_snapshot("tenant", "demo-agent-runtime").unwrap();
        assert_eq!(snapshot.tenant_id, "tenant");
        assert_eq!(snapshot.status, "adopted");
        assert_eq!(snapshot.clauses.len(), 2);
        assert!(snapshot
            .clauses
            .iter()
            .any(|clause| clause.id == "goal:agent-demo"));
        assert!(snapshot
            .clauses
            .iter()
            .any(|clause| clause.kind == "invariant"));
        assert!(snapshot.authentication.is_none());
    }
}
