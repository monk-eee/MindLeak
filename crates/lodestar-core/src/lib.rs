//! Lodestar — the Intent Plane for MindLeak.
//!
//! The durable, authoritative counterpart to the decaying memory graph: a
//! versioned constitution (goals/constraints/invariants), an executive task
//! ledger with atomic claim/lease coordination for parallel local agents, a
//! conformance check that flags drift/violations, and consolidated learned
//! knowledge that is durable-but-revalidated (ADR-0004, ADR-0005, SPEC-INTENT).

pub mod db;
pub mod decay;
pub mod design;
pub mod error;
mod facade;
pub mod llm;
pub mod model;
pub mod store;
mod util;

use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

pub use design::{
    DesignConstraintDraft, DesignItem, DesignMaterializationMode, DesignMaterializationPlan,
    DesignMaterializationRecord, DesignPromotion, DesignPromotionStatus, DesignStatus,
    DesignTaskDraft,
};
pub use error::{LodestarError, Result};
pub use model::{
    Advice, AdviceDisposition, ClaimOverlap, CodeBinding, CodeBindingMode, ConformanceCheck,
    ConformanceEvidence, ConformanceRecord, ConformanceResult, EvidenceProvenance, Goal, GoalKind,
    GoalStatus, GoverningClause, Knowledge, SignalPromotion, Task, TaskQa, TaskScope, TaskStatus,
    Verdict,
};
pub use store::{LodestarStore, ResetOutcome, Stats};

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
    agent: Option<String>,
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
            agent: None,
            #[cfg(test)]
            test_judge: None,
        })
    }

    pub fn open_in_memory() -> Result<Self> {
        Ok(Lodestar {
            store: LodestarStore::new(db::open_in_memory()?),
            llm: LlmClient::default(),
            agent: None,
            #[cfg(test)]
            test_judge: None,
        })
    }

    /// Override the LLM client (dependency injection; used by tests to force the
    /// deterministic no-model fallback regardless of any local server).
    pub fn with_llm(mut self, llm: LlmClient) -> Self {
        self.llm = llm;
        self
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

    pub fn with_agent(mut self, agent: Option<String>) -> Self {
        self.agent = agent.filter(|value| !value.trim().is_empty());
        self
    }

    pub fn store(&self) -> &LodestarStore {
        &self.store
    }

    fn resolve_agent<'a>(&'a self, supplied: &'a str) -> Result<&'a str> {
        let supplied = supplied.trim();
        if supplied.is_empty() {
            return self.agent.as_deref().ok_or_else(|| {
                LodestarError::Invalid(
                    "agent is required when LODESTAR_AGENT is not configured".to_string(),
                )
            });
        }
        if let Some(configured) = self.agent.as_deref() {
            if configured != supplied {
                return Err(LodestarError::Invalid(format!(
                    "agent {supplied} does not match configured identity {configured}"
                )));
            }
            Ok(configured)
        } else {
            Ok(supplied)
        }
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
