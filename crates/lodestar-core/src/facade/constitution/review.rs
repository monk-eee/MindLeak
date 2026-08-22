//! Clause review and the enforcement contract: read/decide pack-clause
//! proposals, and complete a draft clause's scope/evidence/consequence so it
//! can drive a verdict.

use crate::model::Consequence;
use crate::{
    now_unix, Goal, GoalStatus, Lodestar, LodestarError, PackClause, PackClauseDisposition,
    PackClauseProposal, PackClauseProvenance, PackReviewOutcome, Result,
};

impl Lodestar {
    pub fn policy_pack_proposals(
        &self,
        pack_id: &str,
        version: &str,
        constitution_version: Option<&str>,
        include_decided: bool,
    ) -> Result<Vec<PackClauseProposal>> {
        let active;
        let context = match constitution_version {
            Some(version) => Some(version),
            None => {
                active = self.store.active_constitution_version()?;
                active.as_ref().map(|version| version.id.as_str())
            }
        };
        self.store
            .policy_pack_proposals(pack_id, version, context, include_decided)
    }

    pub fn review_pack_clause(
        &self,
        proposal_id: &str,
        disposition: PackClauseDisposition,
        tailored_clause: Option<&PackClause>,
        actor: &str,
        reason: Option<&str>,
    ) -> Result<PackReviewOutcome> {
        self.store.review_pack_clause(
            proposal_id,
            disposition,
            tailored_clause,
            actor,
            reason,
            now_unix(),
        )
    }

    pub fn pack_clause_provenance(&self, goal_id: &str) -> Result<Option<PackClauseProvenance>> {
        self.store.pack_clause_provenance(goal_id)
    }

    /// Give a clause the enforcement contract it needs to drive a verdict:
    /// a scope, an evidence contract, a consequence, and its waiver policy
    /// (SPEC-CONSTITUTION §10).
    ///
    /// Until this is done a clause is review-only, which is the correct default
    /// — migration deliberately invents none of these fields, so a rule never
    /// silently acquires the power to block. But the default is also sticky: a
    /// project can run for a long time with an active constitution that cannot
    /// reach a hard verdict about anything, and nothing about reading the clause
    /// list says so.
    ///
    /// **Refuses a clause belonging to an active version.** Moving a clause from
    /// review-only to `block` changes what governs everyone currently working
    /// under it, and ADR-0039 already fixed the shape of that act: draft an
    /// amendment, complete the contract there, and promote it with a rationale
    /// and a diff. Allowing a direct edit here would be exactly the quiet
    /// amendment that ADR-0039's diff exists to expose.
    pub fn complete_clause_contract(
        &self,
        clause_id: &str,
        scope: &str,
        evidence_contract: &str,
        consequence: Option<Consequence>,
        waivable: bool,
        waiver_authority: Option<&str>,
    ) -> Result<Goal> {
        let clause = self
            .store
            .get_goal(clause_id)?
            .ok_or_else(|| LodestarError::NotFound(clause_id.to_string()))?;
        if clause.status == GoalStatus::Active {
            return Err(LodestarError::Invalid(format!(
                "{clause_id} is active; changing what an active clause enforces is an amendment — \
                 propose_amendment, complete the contract on the draft, then amend_constitution"
            )));
        }
        self.store.complete_clause_contract(
            clause_id,
            scope,
            evidence_contract,
            consequence,
            waivable,
            waiver_authority,
        )
    }
}
