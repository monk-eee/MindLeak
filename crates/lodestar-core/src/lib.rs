//! Lodestar — the Intent Plane for MindLeak.
//!
//! The durable, authoritative counterpart to the decaying memory graph: a
//! versioned constitution (goals/constraints/invariants), an executive task
//! ledger with atomic claim/lease coordination for parallel local agents, a
//! conformance check that flags drift/violations, and consolidated learned
//! knowledge that is durable-but-revalidated (ADR-0004, ADR-0005, SPEC-INTENT).

pub mod amendment;
pub mod controls;
pub mod db;
pub mod decay;
pub mod design;
pub mod dialogue;
pub mod discovery;
pub mod embed;
pub mod error;
mod facade;
pub mod fleet;
pub mod llm;
pub mod merge;
#[cfg(test)]
mod merge_tests;
pub mod model;
pub mod policy;
pub mod scope;
pub mod stalls;
pub mod store;
mod util;
pub mod waiver;

use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

pub use design::{
    DesignAction, DesignActionKind, DesignConstraintDraft, DesignItem, DesignMaterializationMode,
    DesignMaterializationPlan, DesignMaterializationRecord, DesignPromotion, DesignPromotionStatus,
    DesignStatus, DesignTaskDraft,
};
pub use embed::{cosine, Embedder, KnowledgeMatches};
pub use error::{LodestarError, Result};
pub use facade::{DecomposedTask, PlannedDesignMaterialization};
pub use model::{
    Advice, AdviceDisposition, AdviceReason, ArtifactBinding, ArtifactBindingMode, BoardAilment,
    BoardFinding, CertificationState, CertificationStatus, ClaimOverlap, ClaimOverlapReport,
    ClaimWindow, ClauseCoverage, ConformanceCheck, ConformanceCheckReference, ConformanceEvidence,
    ConformanceRecord, ConformanceResult, ConstitutionProposal, ConstitutionState,
    ConstitutionStatus, ConstitutionVersion, EvidenceProvenance, ExternalGoalImportDisposition,
    ExternalGoalImportOutcome, ExternalGoalImportResult, ExternalGoalRecord, Goal, GoalKind,
    GoalStatus, GoverningClause, HumanQuestion, Knowledge, KnowledgeAdvisory, KnowledgeReach,
    OverlapSignal, RepeatedTitle, ReworkReport, SignalPromotion, Task, TaskEvent, TaskEventKind,
    TaskQa, TaskReceipt, TaskScope, TaskStatus, Verdict,
};
pub use policy::{
    common_core_pack, fleet_delivery_pack, ConstitutionPack, PackClause, PackClauseDisposition,
    PackClauseProposal, PackClauseProvenance, PackConflict, PackProposalBatch, PackReviewOutcome,
};
pub use store::{ClaimTransfer, LodestarStore, ResetOutcome, Stats, TransferSource};

use llm::LlmClient;
/// Current unix time in whole seconds.
pub fn now_unix() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// High-level facade over the Intent Plane store and the optional LLM.
pub struct Lodestar {
    store: LodestarStore,
    llm: LlmClient,
    /// The checkout this process serves, when it knows one.
    ///
    /// Optional because the plane is useful without it — every existing verb
    /// works against the ledger alone, and an in-memory store has no repository
    /// at all. Merge verification is the one thing that needs a repository, and
    /// it says so rather than guessing at the current directory: verifying
    /// against whatever directory the server happened to start in is how you
    /// prove a commit landed in somebody else's checkout.
    workspace_root: Option<String>,
    #[cfg(test)]
    test_judge: Option<Box<TestJudge>>,
}

#[cfg(test)]
type TestJudge = dyn Fn(&str, &str) -> Result<(String, String)> + Send + Sync;

impl Lodestar {
    pub fn open(path: &str) -> Result<Self> {
        Ok(Lodestar {
            store: LodestarStore::new(db::open(path)?),
            llm: LlmClient::default(),
            workspace_root: None,
            #[cfg(test)]
            test_judge: None,
        })
    }

    pub fn open_in_memory() -> Result<Self> {
        Ok(Lodestar {
            store: LodestarStore::new(db::open_in_memory()?),
            llm: LlmClient::default(),
            workspace_root: None,
            #[cfg(test)]
            test_judge: None,
        })
    }

    /// Declare the checkout this process serves (ADR-0058 merge verification).
    pub fn with_workspace_root(mut self, root: impl Into<String>) -> Self {
        let root = root.into();
        self.workspace_root = (!root.trim().is_empty()).then_some(root);
        self
    }

    /// Override the LLM client (dependency injection; used by tests to force the
    /// deterministic no-model fallback regardless of any local server).
    pub fn with_llm(mut self, llm: LlmClient) -> Self {
        self.llm = llm;
        self
    }

    /// Probe the optional model only when an explicit status call asks for it.
    pub fn model_health(&self) -> llm::ModelHealth {
        self.llm.model_health()
    }

    #[cfg(test)]
    pub(crate) fn with_test_judge(
        mut self,
        judge: impl Fn(&str, &str) -> Result<(String, String)> + Send + Sync + 'static,
    ) -> Self {
        self.test_judge = Some(Box::new(judge));
        self
    }

    fn judge_conformance(&self, constraint: &str, summary: &str) -> Result<(String, String)> {
        #[cfg(test)]
        if let Some(judge) = self.test_judge.as_ref() {
            return judge(constraint, summary);
        }
        self.llm.judge(constraint, summary)
    }

    pub fn store(&self) -> &LodestarStore {
        &self.store
    }

    fn resolve_agent<'a>(&'a self, supplied: &'a str) -> Result<&'a str> {
        let supplied = supplied.trim();
        if supplied.is_empty() {
            return Err(LodestarError::Invalid(
                "a registered session agent is required".to_string(),
            ));
        }
        Ok(supplied)
    }

    pub fn stats(&self) -> Result<Stats> {
        self.store.stats(now_unix())
    }

    /// Create a verified online SQLite backup without stopping this server.
    pub fn backup_database(&self, destination: &str) -> Result<()> {
        self.store.backup_database(Path::new(destination))
    }

    /// Clear durable intent only after the exact Lodestar confirmation token.
    pub fn reset_database(&self, confirmation: &str) -> Result<ResetOutcome> {
        self.store.reset_database(confirmation)
    }
}
