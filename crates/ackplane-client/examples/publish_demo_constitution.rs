//! Publish the bounded demo policy through the real authenticated service.
//! Uses the supervisor's enrolled-node environment and never accesses PostgreSQL.

use std::error::Error;

use ackplane_client::{companion::wire::Operation, resolve_node_client};
use ackplane_protocol::v1;
use prost::Message;

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
    let node = resolve_node_client(&|name| std::env::var(name).ok())?;
    let snapshot = demo_snapshot(&node.tenant_id, &node.repository_id)?;
    let existing: v1::GetActiveConstitutionResult =
        node.protobuf(Operation::ConstitutionActive).await?;
    if existing.found {
        return Err(
            "a constitution is already published; this example will not overwrite it".into(),
        );
    }
    let published: v1::PublishConstitutionSnapshotResult = node
        .protobuf(Operation::ConstitutionPublish {
            snapshot: snapshot.encode_to_vec(),
        })
        .await?;
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
