//! Facade surface for constitutional amendments and pack upgrades
//! (SPEC-CONSTITUTION §9).

use crate::amendment::{diff_clauses, ClauseDiff, ConstitutionAmendment};
use crate::error::LodestarError;
use crate::model::{ConstitutionVersion, GoalStatus};
use crate::policy::{PackClause, PackClauseDisposition};
use crate::{now_unix, Lodestar, Result};
use serde::{Deserialize, Serialize};

/// How one pack clause differs from what this project adopted from it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PackUpgradeClause {
    pub key: String,
    /// `added` · `removed` · `changed` relative to the adopted version.
    pub change: String,
    /// What this project adopted from, if anything.
    pub adopted_from: Option<PackClause>,
    /// What the newer pack version says.
    pub incoming: Option<PackClause>,
    pub local_goal_id: Option<String>,
    /// Whether the local clause was tailored rather than adopted verbatim.
    ///
    /// The load-bearing flag. Accepting an upstream change to a clause someone
    /// deliberately edited would silently discard that edit, which is the one
    /// way a pack upgrade can quietly undo a local decision.
    pub locally_tailored: bool,
}

/// The reviewable difference between an adopted pack version and a newer one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PackUpgrade {
    pub pack_id: String,
    pub from_version: String,
    pub to_version: String,
    pub clauses: Vec<PackUpgradeClause>,
}

impl Lodestar {
    /// Begin an amendment: draft the next constitutional version, carrying every
    /// active clause forward so the draft starts as the current policy.
    ///
    /// Starting from a copy rather than an empty version is what makes the later
    /// diff readable — only what the author actually changes shows up in it.
    pub fn propose_amendment(&self, created_by: Option<&str>) -> Result<ConstitutionVersion> {
        let active = self.store.active_constitution_version()?.ok_or_else(|| {
            LodestarError::Invalid(
                "no constitution is active; propose_constitution adopts a first one".to_string(),
            )
        })?;
        if let Some(draft) = self.store.draft_constitution_version()? {
            return Err(LodestarError::Invalid(format!(
                "{} is already drafted and awaiting review; resolve it rather than drafting over it",
                draft.id
            )));
        }

        let number = self.store.next_constitution_version_number()?;
        let id = format!("constitution:v{number}");
        let version = self.store.create_constitution_version(
            &id,
            number,
            GoalStatus::Draft,
            created_by,
            now_unix(),
        )?;
        self.store.copy_clauses_to_version(&active.id, &id)?;
        Ok(version)
    }

    /// Promote a reviewed amendment draft, retiring the version it replaces.
    pub fn amend_constitution(
        &self,
        draft_id: &str,
        amended_by: &str,
        rationale: &str,
    ) -> Result<ConstitutionAmendment> {
        self.store
            .amend_constitution(draft_id, amended_by, rationale, now_unix())
    }

    /// The amendment history, newest first.
    pub fn amendments(&self) -> Result<Vec<ConstitutionAmendment>> {
        self.store.amendments()
    }

    /// The clause diff between two constitutional versions, without changing
    /// anything — what an amendment *would* do.
    pub fn constitution_diff(
        &self,
        from_version: &str,
        to_version: &str,
    ) -> Result<Vec<ClauseDiff>> {
        Ok(diff_clauses(
            &self.store.clauses_for_version(from_version)?,
            &self.store.clauses_for_version(to_version)?,
        ))
    }

    /// Compare a newer pack version against what this project actually adopted
    /// from it (SPEC-CONSTITUTION §9, ADR-0026).
    ///
    /// This is a **proposal**, not an upgrade. An upstream version change can
    /// never alter active local policy: adoption copied self-contained local
    /// clauses precisely so that a pack author cannot reach into a governed
    /// project. What this returns is the argument for amending, which a human
    /// still has to accept clause by clause.
    ///
    /// The comparison is against the recorded provenance — the exact pack clause
    /// each local clause was adopted from — rather than against the local clause
    /// itself, so a locally tailored clause does not read as an upstream change.
    pub fn plan_pack_upgrade(&self, pack_id: &str, to_version: &str) -> Result<PackUpgrade> {
        let incoming = self
            .store
            .get_policy_pack(pack_id, to_version)?
            .ok_or_else(|| LodestarError::NotFound(format!("{pack_id}@{to_version}")))?;

        let mut adopted_version: Option<String> = None;
        let mut clauses: Vec<PackUpgradeClause> = Vec::new();
        let mut seen: Vec<String> = Vec::new();

        for goal in self.get_constitution()? {
            let Some(provenance) = self.store.pack_clause_provenance(&goal.id)? else {
                continue;
            };
            if provenance.pack_id != pack_id {
                continue;
            }
            adopted_version.get_or_insert(provenance.pack_version.clone());
            seen.push(provenance.clause_key.clone());

            let locally_tailored = self
                .store
                .pack_clause_disposition(&goal.id)?
                .is_some_and(|disposition| disposition == PackClauseDisposition::Tailored);

            match incoming
                .clauses
                .iter()
                .find(|clause| clause.key == provenance.clause_key)
            {
                None => clauses.push(PackUpgradeClause {
                    key: provenance.clause_key.clone(),
                    change: "removed".to_string(),
                    adopted_from: Some(provenance.source_clause.clone()),
                    incoming: None,
                    local_goal_id: Some(goal.id.clone()),
                    locally_tailored,
                }),
                Some(clause) if *clause != provenance.source_clause => {
                    clauses.push(PackUpgradeClause {
                        key: provenance.clause_key.clone(),
                        change: "changed".to_string(),
                        adopted_from: Some(provenance.source_clause.clone()),
                        incoming: Some(clause.clone()),
                        local_goal_id: Some(goal.id.clone()),
                        locally_tailored,
                    })
                }
                Some(_) => {}
            }
        }

        for clause in &incoming.clauses {
            if !seen.contains(&clause.key) {
                clauses.push(PackUpgradeClause {
                    key: clause.key.clone(),
                    change: "added".to_string(),
                    adopted_from: None,
                    incoming: Some(clause.clone()),
                    local_goal_id: None,
                    locally_tailored: false,
                });
            }
        }

        clauses.sort_by(|left, right| left.key.cmp(&right.key));
        Ok(PackUpgrade {
            pack_id: pack_id.to_string(),
            from_version: adopted_version.unwrap_or_default(),
            to_version: to_version.to_string(),
            clauses,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::amendment::ClauseChange;
    use crate::facade::test_support::engine;
    use crate::model::Consequence;
    use crate::policy::common_core_pack;

    /// A governed project with the Common Core adopted.
    fn governed(e: &Lodestar) -> String {
        let proposal = e
            .propose_constitution(&["README.md".to_string()], Some("monk-eee"))
            .unwrap();
        for clause in proposal.common_core.proposals {
            e.review_pack_clause(
                &clause.id,
                PackClauseDisposition::Adopted,
                None,
                "monk-eee",
                Some("Adopted as proposed"),
            )
            .unwrap();
        }
        e.activate_constitution(&proposal.version.id, "monk-eee")
            .unwrap();
        proposal.version.id
    }

    #[test]
    fn an_amendment_draft_starts_as_the_current_policy() {
        // Anything else would make the author re-adopt every rule, and the diff
        // would drown the one real change in noise.
        let e = engine();
        let active = governed(&e);
        let draft = e.propose_amendment(Some("monk-eee")).unwrap();

        assert_eq!(draft.status, GoalStatus::Draft);
        assert!(e.constitution_diff(&active, &draft.id).unwrap().is_empty());
    }

    #[test]
    fn amending_records_an_attributed_diff_and_moves_which_version_governs() {
        let e = engine();
        let active = governed(&e);
        let draft = e.propose_amendment(Some("monk-eee")).unwrap();

        // Harden one carried clause.
        let target = e
            .store
            .clauses_for_version(&draft.id)
            .unwrap()
            .into_iter()
            .next()
            .unwrap();
        e.store
            .complete_clause_contract(
                &target.id,
                "artifact:crates/**",
                "tests",
                Some(Consequence::Block),
                false,
                None,
            )
            .unwrap();

        let amendment = e
            .amend_constitution(&draft.id, "monk-eee", "Evidence rule needed teeth.")
            .unwrap();

        assert_eq!(amendment.from_version, active);
        assert_eq!(amendment.amended_by, "monk-eee");
        assert!(amendment
            .diff
            .iter()
            .any(|d| d.change == ClauseChange::Changed));
        assert_eq!(
            e.constitution_status().unwrap().version.unwrap().id,
            draft.id
        );
        assert_eq!(e.amendments().unwrap().len(), 1);
    }

    #[test]
    fn a_second_amendment_draft_is_refused_while_one_is_open() {
        let e = engine();
        governed(&e);
        e.propose_amendment(Some("monk-eee")).unwrap();
        assert!(e.propose_amendment(Some("monk-eee")).is_err());
    }

    #[test]
    fn a_pack_upgrade_is_a_proposal_that_changes_nothing_by_itself() {
        // ADR-0026: an upstream version change can never alter active local
        // policy. Planning the upgrade must therefore be a pure read.
        let e = engine();
        governed(&e);

        let mut newer = common_core_pack();
        newer.version = "2".to_string();
        newer.clauses[0].statement = "Do not claim success without fresh evidence.".to_string();
        newer.clauses.push(PackClause {
            key: "core.reversibility".to_string(),
            kind: crate::GoalKind::Principle,
            title: "Prefer reversible change".to_string(),
            statement: "Prefer changes that can be undone.".to_string(),
            rationale: "Reversibility bounds the cost of being wrong.".to_string(),
            default_scope: None,
            evidence_contract: None,
            default_consequence: Some(Consequence::Review),
            suggested_controls: Vec::new(),
        });
        newer.clauses.remove(1);
        newer.digest = newer.computed_digest().unwrap();
        e.register_policy_pack(&newer).unwrap();

        let before = e.get_constitution().unwrap();
        let plan = e.plan_pack_upgrade("common-core", "2").unwrap();
        assert_eq!(
            e.get_constitution().unwrap(),
            before,
            "planning an upgrade must not touch active policy"
        );

        let change = |key: &str| {
            plan.clauses
                .iter()
                .find(|c| c.key == key)
                .map(|c| c.change.as_str())
        };
        assert_eq!(change("core.evidence"), Some("changed"));
        assert_eq!(change("core.reversibility"), Some("added"));
        assert_eq!(change("core.intent"), Some("removed"));
        // Untouched clauses stay out of the plan.
        assert_eq!(change("core.safety"), None);
    }

    #[test]
    fn a_tailored_clause_is_flagged_so_an_upgrade_cannot_silently_undo_it() {
        // The one way a pack upgrade can quietly discard a local decision.
        let e = engine();
        let proposal = e
            .propose_constitution(&["README.md".to_string()], Some("monk-eee"))
            .unwrap();
        for clause in proposal.common_core.proposals {
            if clause.clause.key == "core.evidence" {
                let mut tailored = clause.clause.clone();
                tailored.statement = "Evidence must include a benchmark.".to_string();
                e.review_pack_clause(
                    &clause.id,
                    PackClauseDisposition::Tailored,
                    Some(&tailored),
                    "monk-eee",
                    Some("Local bar is higher"),
                )
                .unwrap();
            } else {
                e.review_pack_clause(
                    &clause.id,
                    PackClauseDisposition::Adopted,
                    None,
                    "monk-eee",
                    Some("Reviewed"),
                )
                .unwrap();
            }
        }
        e.activate_constitution(&proposal.version.id, "monk-eee")
            .unwrap();

        let mut newer = common_core_pack();
        newer.version = "2".to_string();
        newer.clauses[0].statement = "Upstream rewrote this.".to_string();
        newer.digest = newer.computed_digest().unwrap();
        e.register_policy_pack(&newer).unwrap();

        let plan = e.plan_pack_upgrade("common-core", "2").unwrap();
        let evidence = plan
            .clauses
            .iter()
            .find(|c| c.key == "core.evidence")
            .unwrap();
        assert_eq!(evidence.change, "changed");
        assert!(
            evidence.locally_tailored,
            "a tailored clause must be flagged before an upstream change overwrites it"
        );
    }
}
