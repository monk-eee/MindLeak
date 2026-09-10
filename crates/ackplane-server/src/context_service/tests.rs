use std::time::{Duration, SystemTime};

use ackplane_protocol::supervisor::*;

use super::*;
use crate::{
    claim_store::{ClaimLeaseOutcome, ClaimLeaseRequest},
    constitution_store::{ClauseSnapshot, PublishConstitutionRequest},
    knowledge_store::RecordKnowledgeRequest,
    work_store::NewWorkTask,
};

struct Fixture {
    service: ContextService,
    tenant: String,
    repository: String,
    node: String,
}

impl Fixture {
    async fn new() -> Option<Self> {
        let pool = crate::test_support::test_pool()?;
        let fixture = Self {
            service: ContextService::connect(&pool).await.unwrap(),
            tenant: crate::test_support::unique_id("context-tenant"),
            repository: "context-repository".into(),
            node: "context-node".into(),
        };
        fixture
            .service
            .constitution
            .publish(PublishConstitutionRequest {
                tenant_id: fixture.tenant.clone(),
                repository_id: fixture.repository.clone(),
                version_id: "constitution:v1".into(),
                version: 1,
                status: "adopted".into(),
                clauses: vec![ClauseSnapshot {
                    id: "goal:context".into(),
                    slug: "context".into(),
                    kind: "invariant".into(),
                    title: "Keep evidence".into(),
                    statement: "Every completed task requires verified test evidence".into(),
                    status: "active".into(),
                    consequence: None,
                    scope: None,
                    rationale: None,
                }],
            })
            .await
            .unwrap();
        Some(fixture)
    }

    async fn assign(
        &self,
        session_id: &str,
        task_id: &str,
        claimed: bool,
    ) -> v1::ContextPacketRequest {
        let supervisor_id = format!("supervisor:{session_id}");
        self.service
            .supervisors
            .register(&SupervisorRegistration {
                supervisor_id: supervisor_id.clone(),
                identity: SupervisorIdentity {
                    tenant_id: self.tenant.clone(),
                    repository_id: self.repository.clone(),
                    node_id: self.node.clone(),
                },
                supervisor_version: "test".into(),
                protocol_version: "v1".into(),
                capabilities: SupervisorCapabilities {
                    supported_directives: vec![SupervisorDirectiveCapability::Assign],
                    supports_checkpoint: false,
                    supports_force_termination: false,
                    outbox_durability: SupervisorOutboxDurability::Persistent,
                    recoverable_outbox: true,
                },
            })
            .await
            .unwrap();
        self.service
            .supervisors
            .record_session(
                &self.tenant,
                &self.repository,
                &SupervisorSession {
                    supervisor_id,
                    session_id: session_id.into(),
                    worker_id: format!("worker:{session_id}"),
                    runtime: SupervisorRuntime::LocalMachine,
                    started_at: OffsetDateTime::now_utc().unix_timestamp(),
                    state: SupervisorWorkerState::Started,
                },
            )
            .await
            .unwrap();
        let paths = vec![format!("src/{task_id}.rs")];
        if self
            .service
            .work
            .task_detail(&self.tenant, &self.repository, task_id)
            .await
            .unwrap()
            .is_none()
        {
            self.service
                .work
                .create_task(
                    &NewWorkTask {
                        tenant_id: self.tenant.clone(),
                        repository_id: self.repository.clone(),
                        task_id: task_id.into(),
                        title: format!("Implement {task_id}"),
                        acceptance: "Tests pass and evidence is submitted".into(),
                        goal_id: Some("goal:context".into()),
                        declared_paths: paths.clone(),
                        declared_symbols: vec![],
                        published_by: self.node.clone(),
                    },
                    &format!("event:{task_id}"),
                    SystemTime::now(),
                )
                .await
                .unwrap();
        }
        if claimed {
            let result = self
                .service
                .claims
                .delegate(
                    &ClaimLeaseRequest {
                        tenant_id: self.tenant.clone(),
                        repository_id: self.repository.clone(),
                        task_id: task_id.into(),
                        owner_id: session_id.into(),
                        branch: "test".into(),
                        lease: Duration::from_secs(300),
                        paths,
                        symbols: vec![],
                    },
                    SystemTime::now(),
                )
                .await
                .unwrap();
            assert_eq!(result.outcome, ClaimLeaseOutcome::Granted);
        }
        let mut directive = v1::AgentDirective {
            directive_id: format!("directive:{session_id}:{task_id}"),
            tenant_id: self.tenant.clone(),
            project_id: "project:context".into(),
            repository_id: self.repository.clone(),
            target_node_id: self.node.clone(),
            target_agent_session_id: session_id.into(),
            kind: v1::DirectiveKind::Assign as i32,
            schema_version: "v1".into(),
            issuing_principal_id: "human:test".into(),
            rationale: "run the test task".into(),
            task_id: task_id.into(),
            goal_id: "goal:context".into(),
            context_packet_id: String::new(),
            created_at: String::new(),
            expires_at: (OffsetDateTime::now_utc() + time::Duration::minutes(5))
                .format(&Rfc3339)
                .unwrap(),
            sequence: 0,
            idempotency_key: format!("assign:{session_id}:{task_id}"),
            payload_digest: vec![],
            required_capability: "assign.v1".into(),
            policy_refs: vec!["constitution:v1".into()],
            knowledge_refs: vec![],
            evidence_refs: vec![],
            payload: Some(v1::agent_directive::Payload::Assign(v1::AssignDirective {})),
        };
        directive.payload_digest = directive_payload_digest(&directive).unwrap();
        self.service
            .directives
            .enqueue(directive.clone())
            .await
            .unwrap();
        v1::ContextPacketRequest {
            directive_id: directive.directive_id,
            agent_session_id: session_id.into(),
            token_budget: 8192,
        }
    }

    async fn request(
        &self,
        request: &v1::ContextPacketRequest,
    ) -> Result<ContextPacket, ContextServiceError> {
        self.service
            .request(&self.tenant, &self.repository, &self.node, request)
            .await
    }
}

#[tokio::test]
async fn two_sessions_get_separate_context_and_cannot_request_each_others_assignment() {
    let Some(fixture) = Fixture::new().await else {
        eprintln!("skipped: ACKPLANE_TEST_DATABASE_URL not set");
        return;
    };
    let first = fixture.assign("first", "task-first", true).await;
    let second = fixture.assign("second", "task-second", true).await;
    let (first_packet, second_packet) =
        tokio::join!(fixture.request(&first), fixture.request(&second));
    assert_eq!(first_packet.unwrap().scope.agent_session_id, "first");
    assert_eq!(second_packet.unwrap().scope.agent_session_id, "second");
    let stolen = v1::ContextPacketRequest {
        directive_id: first.directive_id.clone(),
        ..second.clone()
    };
    assert!(fixture.request(&stolen).await.is_err());
    assert!(fixture
        .service
        .request(&fixture.tenant, &fixture.repository, "another-node", &first)
        .await
        .is_err());
    assert!(fixture
        .service
        .request("another-tenant", &fixture.repository, &fixture.node, &first)
        .await
        .is_err());
}

#[tokio::test]
async fn missing_live_lease_refuses_a_real_assignment() {
    let Some(fixture) = Fixture::new().await else {
        eprintln!("skipped: ACKPLANE_TEST_DATABASE_URL not set");
        return;
    };
    let request = fixture.assign("first", "task-first", false).await;
    assert!(matches!(
        fixture.request(&request).await,
        Err(ContextServiceError::Refused(
            "this session has no live task lease"
        ))
    ));
}

#[tokio::test]
async fn projected_graph_relationships_reach_the_addressed_task_prompt() {
    use crate::{
        ledger::{DedupKey, LedgerStore},
        projection::{tests::structural_fact_envelope, StructuralEdgeFact, StructuralFact},
    };
    let Some(fixture) = Fixture::new().await else {
        eprintln!("skipped: ACKPLANE_TEST_DATABASE_URL not set");
        return;
    };
    let ledger = LedgerStore::connect(&crate::test_support::gated_test_pool())
        .await
        .unwrap();
    let facts = [
        StructuralFact {
            node_id: "artifact:src/task-first.rs".into(),
            node_type: "artifact".into(),
            label: "task-first.rs".into(),
            edges: vec![StructuralEdgeFact {
                target_id: "symbol:src/task-first.rs:check".into(),
                relation: "contains".into(),
                base_weight: 1.0,
                half_life_hours: 168.0,
            }],
        },
        StructuralFact {
            node_id: "symbol:src/task-first.rs:check".into(),
            node_type: "symbol".into(),
            label: "check".into(),
            edges: vec![],
        },
    ];
    for (index, fact) in facts.iter().enumerate() {
        ledger
            .append(&structural_fact_envelope(
                DedupKey {
                    tenant_id: fixture.tenant.clone(),
                    repository_id: fixture.repository.clone(),
                    producer_id: fixture.node.clone(),
                    producer_sequence: index as i64 + 1,
                },
                format!("digest:{index}").as_bytes(),
                fact,
            ))
            .await
            .unwrap();
    }
    fixture
        .service
        .projection
        .rebuild(&fixture.tenant, &fixture.repository)
        .await
        .unwrap();
    let request = fixture.assign("first", "task-first", true).await;
    let packet = fixture.request(&request).await.unwrap();
    let graph: Vec<_> = packet
        .selected
        .iter()
        .filter(|item| item.item_kind == ContextItemKind::Structural)
        .collect();
    assert_eq!(graph.len(), 2);
    let symbol = graph
        .iter()
        .find(|item| item.source_reference == "symbol:src/task-first.rs:check")
        .unwrap();
    let body: serde_json::Value = serde_json::from_str(&symbol.rendered).unwrap();
    assert_eq!(body["relationships"][0]["relation"], "contains");
    assert_eq!(packet.source.projection_position, 2);
}

#[tokio::test]
async fn a_prior_worker_failure_informs_the_next_session_without_becoming_policy() {
    let Some(fixture) = Fixture::new().await else {
        eprintln!("skipped: ACKPLANE_TEST_DATABASE_URL not set");
        return;
    };
    let first = fixture.assign("first", "task-retry", true).await;
    let before = fixture.request(&first).await.unwrap();
    fixture
        .service
        .supervisors
        .record_lifecycle(
            &crate::supervisor_store::SupervisorLifecycleReceiptRequest {
                tenant_id: fixture.tenant.clone(),
                repository_id: fixture.repository.clone(),
                idempotency_key: "observed-worker-failure".into(),
                receipt: SupervisorLifecycleReceipt {
                    supervisor_id: "supervisor:first".into(),
                    session_id: "first".into(),
                    worker_id: "worker:first".into(),
                    occurred_at: OffsetDateTime::now_utc().unix_timestamp(),
                    state: SupervisorWorkerState::Failed,
                    reason: Some(SupervisorLifecycleReason::WorkerLost),
                },
            },
        )
        .await
        .unwrap();
    fixture
        .service
        .claims
        .release(
            &fixture.tenant,
            &fixture.repository,
            "task-retry",
            "first",
            SystemTime::now(),
        )
        .await
        .unwrap();
    let retry = fixture.assign("second", "task-retry", true).await;
    let after = fixture.request(&retry).await.unwrap();
    let observation = after
        .selected
        .iter()
        .find(|item| item.item_kind == ContextItemKind::Outcome)
        .expect("retry should include the prior observed failure");
    assert!(!observation.mandatory);
    assert_eq!(observation.reason, ContextSelectionReason::PriorOutcome);
    assert_eq!(
        observation.provenance.evidence_reference.as_deref(),
        Some(before.packet_id.as_str())
    );
    let body: serde_json::Value = serde_json::from_str(&observation.rendered).unwrap();
    assert_eq!(body["worker_state"], "failed");
    assert_eq!(body["verified_task_completion"], false);
    assert!(after
        .selected
        .iter()
        .any(|item| item.mandatory && item.item_kind == ContextItemKind::Constitution));
}

#[tokio::test]
async fn activated_memory_changes_the_next_prompt_without_replacing_mandatory_policy() {
    let Some(fixture) = Fixture::new().await else {
        eprintln!("skipped: ACKPLANE_TEST_DATABASE_URL not set");
        return;
    };
    let request = fixture.assign("first", "task-first", true).await;
    let before = fixture.request(&request).await.unwrap();
    let learned = fixture
        .service
        .knowledge
        .record(RecordKnowledgeRequest {
            tenant_id: fixture.tenant.clone(),
            repository_id: fixture.repository.clone(),
            content:
                "The last run demonstrated that the fixture must allocate a unique database key"
                    .into(),
            source_ref: Some("execution:verified-fixture-failure".into()),
            recorded_by: Some(fixture.node.clone()),
            reach_node_ids: vec!["artifact:src/task-first.rs".into()],
            reach_goal_id: Some("goal:context".into()),
            half_life_hours: 168.0,
            embedding: None,
        })
        .await
        .unwrap();
    let candidate_packet = fixture.request(&request).await.unwrap();
    assert!(!candidate_packet
        .selected
        .iter()
        .any(|item| item.item_id == learned.knowledge_id));
    fixture
        .service
        .knowledge
        .activate(
            &fixture.tenant,
            &fixture.repository,
            &learned.knowledge_id,
            "human:test",
            Some("verified by the regression test"),
            SystemTime::now(),
        )
        .await
        .unwrap();
    let after = fixture.request(&request).await.unwrap();
    assert!(after
        .selected
        .iter()
        .any(|item| item.item_id == learned.knowledge_id && !item.mandatory));
    let before_policy: Vec<_> = before
        .selected
        .iter()
        .filter(|item| item.mandatory)
        .map(|item| (&item.item_id, &item.rendered))
        .collect();
    let after_policy: Vec<_> = after
        .selected
        .iter()
        .filter(|item| item.mandatory)
        .map(|item| (&item.item_id, &item.rendered))
        .collect();
    assert_eq!(before_policy, after_policy);
    let stored = fixture
        .service
        .packets
        .get_packet(&fixture.tenant, &fixture.repository, &after.packet_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(stored, after);
}
