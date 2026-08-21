//! Goal lifecycle: define, supersede, import external records, and read the
//! active constitution.

use crate::{
    now_unix, ConstitutionState, ConstitutionStatus, ExternalGoalImportResult, ExternalGoalRecord,
    Goal, GoalKind, GoalStatus, Lodestar, Result,
};

impl Lodestar {
    pub fn define_goal(
        &self,
        kind: GoalKind,
        title: &str,
        statement: &str,
        parent: Option<String>,
    ) -> Result<Goal> {
        self.store
            .define_goal(kind, title, statement, parent, now_unix())
    }

    pub fn supersede_goal(&self, old_id: &str, new_statement: &str, reason: &str) -> Result<Goal> {
        self.store
            .supersede_goal(old_id, new_statement, reason, now_unix())
    }

    /// Import structured records supplied by an external ADR system. This
    /// deliberately never reads or parses the caller's source documents.
    pub fn import_external_goals(
        &self,
        source_system: &str,
        records: &[ExternalGoalRecord],
    ) -> Result<ExternalGoalImportResult> {
        self.store
            .import_external_goals(source_system, records, now_unix())
    }

    /// The authoritative set an agent reads before acting.
    pub fn get_constitution(&self) -> Result<Vec<Goal>> {
        self.store.goals_by_status(GoalStatus::Active)
    }

    /// Report whether this project has an adopted constitution, a draft awaiting
    /// review, or none at all (SPEC-CONSTITUTION §11). Read-only and
    /// model-free: it never proposes, activates, or mutates anything. An active
    /// version wins over a draft, because only an activated version authorises
    /// verdicts — a project mid-bootstrap must not read as governed.
    pub fn constitution_status(&self) -> Result<ConstitutionStatus> {
        if let Some(version) = self.store.active_constitution_version()? {
            return Ok(ConstitutionStatus {
                state: ConstitutionState::Active,
                version: Some(version),
                clause_count: self.store.count_goals_by_status(GoalStatus::Active)?,
            });
        }
        if let Some(version) = self.store.draft_constitution_version()? {
            return Ok(ConstitutionStatus {
                state: ConstitutionState::Draft,
                version: Some(version),
                clause_count: self.store.count_goals_by_status(GoalStatus::Draft)?,
            });
        }
        Ok(ConstitutionStatus {
            state: ConstitutionState::Absent,
            version: None,
            clause_count: 0,
        })
    }
}
