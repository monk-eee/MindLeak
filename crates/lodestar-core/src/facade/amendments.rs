//! Facade surface for constitutional amendments and pack upgrades
//! (SPEC-CONSTITUTION §9).

use crate::amendment::{diff_clauses, ClauseDiff, ConstitutionAmendment};
use crate::error::LodestarError;
use crate::model::{ConstitutionVersion, Goal, GoalKind, GoalStatus};
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

    /// Author a new clause directly into an amendment draft.
    ///
    /// Policy has to be able to grow. `define_goal` states a rule that is live
    /// the moment it is written, and `complete_clause_contract` refuses to give
    /// a live rule a contract, because hardening what people are already working
    /// under is exactly what an amendment is for. Without this verb those two
    /// correct rules met in a corner: the clause that most needed an enforcement
    /// contract was the one clause that could never be given one, and belonging
    /// to no constitutional version it never appeared in a diff either. The only
    /// way in was to mint a policy pack, which records immutable *upstream*
    /// provenance — a fabricated source for a rule this project wrote itself.
    ///
    /// The clause enters as part of the draft, so it takes effect only if the
    /// amendment is promoted, and it shows up in `constitution_diff` as `added`
    /// for whoever reviews it. It carries the same id shape as a clause copied
    /// forward, so nothing downstream can tell an authored clause from an
    /// inherited one once the version is live.
    pub fn draft_clause(
        &self,
        draft_id: &str,
        kind: GoalKind,
        title: &str,
        statement: &str,
    ) -> Result<Goal> {
        let version = self
            .store
            .constitution_version(draft_id)?
            .ok_or_else(|| LodestarError::NotFound(draft_id.to_string()))?;
        if version.status != GoalStatus::Draft {
            return Err(LodestarError::Invalid(format!(
                "{draft_id} is {}, and a clause may only be authored into a draft; \
                 propose_amendment opens one",
                version.status.as_str()
            )));
        }
        self.store
            .define_clause_in_version(kind, title, statement, draft_id, now_unix())
    }

    /// Promote a reviewed amendment draft, retiring the version it replaces.
    ///
    /// `approved_by` is the separation of parties, not a humans-only gate: an
    /// agent may approve an amendment, just not its own. That is the whole
    /// point — the record could previously only ever name the calling agent, so
    /// an agent changing policy on its own initiative was indistinguishable
    /// from a reviewed adoption. Attributed, never authenticated (ADR-0071):
    /// this establishes what the audit history can say, and nothing here could
    /// be enforced against a determined caller on a local stdio server.
    ///
    /// The same shape `task_transition to="resolve"` already uses for a single
    /// task, applied to the larger act.
    pub fn amend_constitution(
        &self,
        draft_id: &str,
        amended_by: &str,
        approved_by: &str,
        rationale: &str,
    ) -> Result<ConstitutionAmendment> {
        let approver = approved_by.trim();
        if approver.is_empty() {
            return Err(LodestarError::Invalid(
                "an amendment requires an attributed approver; ADR-0043 makes adoption an \
                 attributed act, and an unnamed one records nothing"
                    .to_string(),
            ));
        }
        if approver.eq_ignore_ascii_case(amended_by.trim()) {
            return Err(LodestarError::Invalid(format!(
                "{approver} proposed this amendment and cannot also approve it; name the \
                 reviewer who accepted it, which may be another agent"
            )));
        }
        self.store
            .amend_constitution(draft_id, amended_by, approver, rationale, now_unix())
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
    use crate::controls::{Control, ControlKind, ControlStatus, EnforcementPower};
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
            .amend_constitution(
                &draft.id,
                "monk-eee",
                "reviewer",
                "Evidence rule needed teeth.",
            )
            .unwrap();

        assert_eq!(amendment.from_version, active);
        assert_eq!(amendment.amended_by, "monk-eee");
        assert_eq!(amendment.approved_by.as_deref(), Some("reviewer"));
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

    // The record could previously only ever name the calling agent, so an agent
    // amending policy on its own initiative was indistinguishable from a
    // reviewed adoption. The guard is separation of parties, not a humans-only
    // gate: another agent approving is fine, approving your own is not.
    #[test]
    fn an_amendment_cannot_be_approved_by_whoever_made_it() {
        let e = engine();
        let active = governed(&e);
        let draft = e.propose_amendment(Some("session:v1:alice")).unwrap();
        harden_clause(&e, &draft.id, 0);

        let error = e
            .amend_constitution(
                &draft.id,
                "session:v1:alice",
                "session:v1:alice",
                "Because I say so.",
            )
            .unwrap_err();
        assert!(
            format!("{error}").contains("cannot also approve it"),
            "{error}"
        );
        // Case is not a loophole: the same party under another spelling is the
        // same party.
        assert!(e
            .amend_constitution(
                &draft.id,
                "session:v1:alice",
                "SESSION:V1:ALICE",
                "Because I say so.",
            )
            .is_err());
        assert!(e
            .amend_constitution(&draft.id, "session:v1:alice", "   ", "No approver.")
            .is_err());

        // A different party may approve, and it need not be a human.
        let amendment = e
            .amend_constitution(
                &draft.id,
                "session:v1:alice",
                "session:v1:bob",
                "Reviewed by a peer.",
            )
            .unwrap();
        assert_eq!(amendment.from_version, active);
        assert_eq!(amendment.approved_by.as_deref(), Some("session:v1:bob"));
    }

    #[test]
    fn a_second_amendment_draft_is_refused_while_one_is_open() {
        let e = engine();
        governed(&e);
        e.propose_amendment(Some("monk-eee")).unwrap();
        assert!(e.propose_amendment(Some("monk-eee")).is_err());
    }

    /// Harden the `index`th carried clause on `draft`, returning its slug.
    fn harden_clause(e: &Lodestar, draft_id: &str, index: usize) -> String {
        let target = e
            .store
            .clauses_for_version(draft_id)
            .unwrap()
            .into_iter()
            .nth(index)
            .unwrap();
        e.store
            .complete_clause_contract(
                &target.id,
                "artifact:crates/**",
                "tests",
                Some(Consequence::Review),
                false,
                None,
            )
            .unwrap();
        target.slug
    }

    fn active_clause(e: &Lodestar, slug: &str) -> String {
        let version = e.constitution_status().unwrap().version.unwrap().id;
        e.store
            .clauses_for_version(&version)
            .unwrap()
            .into_iter()
            .find(|clause| clause.slug == slug)
            .expect("the clause is carried forward")
            .id
    }

    /// The incident: giving a clause an enforcement contract disarmed the
    /// control enforcing it. A clause copy takes a new id, controls store the
    /// id they were registered against, and nothing re-pointed them — so the
    /// ratchet went on accepting observations and went on answering pass and
    /// fail while serving no active clause, which collapses its consequence to
    /// `advise`. Enforcement stops without anything failing, at the exact
    /// moment someone was strengthening the rule.
    #[test]
    fn an_amendment_carries_the_controls_across_with_the_clause() {
        let e = engine();
        governed(&e);
        let draft = e.propose_amendment(Some("monk-eee")).unwrap();
        let slug = harden_clause(&e, &draft.id, 0);
        let before = active_clause(&e, &slug);

        e.register_control(&Control {
            id: "control:pre-push".into(),
            clause_id: before.clone(),
            kind: ControlKind::Check,
            power: EnforcementPower::Mechanical,
            version: 1,
            configuration: None,
            status: ControlStatus::Active,
            retired_by: None,
            retired_at: None,
        })
        .unwrap();

        e.amend_constitution(
            &draft.id,
            "monk-eee",
            "reviewer",
            "Evidence rule needed teeth.",
        )
        .unwrap();

        let after = active_clause(&e, &slug);
        assert_ne!(before, after, "the amendment gives the clause a new id");

        let controls = e.clause_controls(&after).unwrap();
        assert_eq!(
            controls.iter().map(|c| c.id.as_str()).collect::<Vec<_>>(),
            vec!["control:pre-push"],
            "the live clause is still guarded"
        );
        assert_eq!(controls[0].clause_id, after);
    }

    /// A control stranded by an earlier amendment cannot re-register itself:
    /// `register_ratchet` refuses a control whose version would move backwards,
    /// so the id is spent. Recovery therefore has to be something the next
    /// amendment does, or the orphan is permanent.
    #[test]
    fn the_next_amendment_recovers_a_control_stranded_by_an_earlier_one() {
        let e = engine();
        governed(&e);

        let first = e.propose_amendment(Some("monk-eee")).unwrap();
        let slug = harden_clause(&e, &first.id, 0);
        let stranded_against = active_clause(&e, &slug);
        e.amend_constitution(&first.id, "monk-eee", "reviewer", "First change.")
            .unwrap();

        // Registered against the clause id that the first amendment retired,
        // reproducing the state the fix has to clean up.
        e.register_control(&Control {
            id: "control:stranded".into(),
            clause_id: stranded_against.clone(),
            kind: ControlKind::Check,
            power: EnforcementPower::Mechanical,
            version: 1,
            configuration: None,
            status: ControlStatus::Active,
            retired_by: None,
            retired_at: None,
        })
        .unwrap();
        assert!(
            e.clause_controls(&active_clause(&e, &slug))
                .unwrap()
                .is_empty(),
            "the control starts out orphaned"
        );

        let second = e.propose_amendment(Some("monk-eee")).unwrap();
        harden_clause(&e, &second.id, 1);
        e.amend_constitution(&second.id, "monk-eee", "reviewer", "Second change.")
            .unwrap();

        let controls = e.clause_controls(&active_clause(&e, &slug)).unwrap();
        assert_eq!(
            controls.iter().map(|c| c.id.as_str()).collect::<Vec<_>>(),
            vec!["control:stranded"],
            "the orphan is adopted by the clause it was always meant to serve"
        );
    }

    /// A retired control is a record of what once enforced a rule. Re-pointing
    /// it would quietly rewrite that record onto a clause it never guarded.
    #[test]
    fn an_amendment_leaves_a_retired_control_naming_what_it_served() {
        let e = engine();
        governed(&e);
        let draft = e.propose_amendment(Some("monk-eee")).unwrap();
        let slug = harden_clause(&e, &draft.id, 0);
        let before = active_clause(&e, &slug);

        e.register_control(&Control {
            id: "control:retired".into(),
            clause_id: before.clone(),
            kind: ControlKind::Check,
            power: EnforcementPower::Mechanical,
            version: 1,
            configuration: None,
            status: ControlStatus::Active,
            retired_by: None,
            retired_at: None,
        })
        .unwrap();
        assert!(e.retire_control("control:retired", "monk-eee").unwrap());

        e.amend_constitution(
            &draft.id,
            "monk-eee",
            "reviewer",
            "Evidence rule needed teeth.",
        )
        .unwrap();

        let after = active_clause(&e, &slug);
        assert!(
            e.clause_controls(&after).unwrap().is_empty(),
            "a retired control does not follow the clause forward"
        );
        assert_eq!(e.clause_controls(&before).unwrap()[0].id, "control:retired");
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

    /// Regression, task:4cef8e361fc7: policy could not grow.
    ///
    /// `define_goal` writes a clause `active` with no constitution version, and
    /// `complete_clause_contract` refuses an active clause — correctly, since
    /// hardening a live rule mid-flight is an amendment. But `propose_amendment`
    /// only copied the clauses that already existed, and nothing could add one.
    /// So the clause that most needs a contract was the one clause that could
    /// never be given one, and belonging to no version it never appeared in
    /// `constitution_diff` either.
    ///
    /// Measured impact: this blocked registering a ratchet over the MCP tool
    /// surface. `register_ratchet` requires an active clause that authorises it,
    /// and none of the 25 clauses mentioned the tool surface. The only route
    /// into a version was `register_policy_pack`, which records immutable
    /// upstream provenance — minting a pack to carry a rule this project wrote
    /// itself would have put a fabricated source in the provenance record.
    #[test]
    fn a_locally_authored_clause_can_be_written_into_an_amendment() {
        let e = engine();
        governed(&e);

        let draft = e.propose_amendment(Some("monk-eee")).unwrap();
        let clause = e
            .draft_clause(
                &draft.id,
                GoalKind::Constraint,
                "The advertised tool surface stays small",
                "The MCP tool surface stays within the budget one session can read.",
            )
            .unwrap();

        // It enters as part of the draft, not as live policy: a new rule that
        // took effect the moment it was typed would bypass the review the
        // amendment exists to be.
        assert_eq!(clause.status, GoalStatus::Draft);
        assert_eq!(
            clause.constitution_version.as_deref(),
            Some(draft.id.as_str())
        );

        e.complete_clause_contract(
            &clause.id,
            "mcp:tools",
            "The advertised tool count and the tokens it costs a session.",
            Some(Consequence::Review),
            false,
            None,
        )
        .unwrap();

        e.amend_constitution(
            &draft.id,
            "monk-eee",
            "reviewer",
            "Bring the tool surface under a stated rule.",
        )
        .unwrap();

        let landed = e
            .get_constitution()
            .unwrap()
            .into_iter()
            .find(|g| g.slug == clause.slug)
            .expect("the promoted version must contain the new clause");

        assert_eq!(landed.status, GoalStatus::Active);
        assert_eq!(landed.scope.as_deref(), Some("mcp:tools"));
        assert_eq!(landed.consequence, Some(Consequence::Review));
    }

    /// A new clause is authored into the draft, so it must show up in the diff a
    /// reviewer reads before promoting. A rule that arrives invisibly is the
    /// laundering this verb exists to avoid.
    #[test]
    fn a_clause_authored_into_a_draft_appears_in_the_diff() {
        let e = engine();
        let active = governed(&e);
        let draft = e.propose_amendment(Some("monk-eee")).unwrap();

        let clause = e
            .draft_clause(
                &draft.id,
                GoalKind::Constraint,
                "The advertised tool surface stays small",
                "The MCP tool surface stays within the budget one session can read.",
            )
            .unwrap();

        let diff = e.constitution_diff(&active, &draft.id).unwrap();
        let added = diff
            .iter()
            .find(|d| d.change == ClauseChange::Added)
            .expect("a clause written into the draft must read as added");

        assert_eq!(added.slug, clause.slug);
    }

    /// The draft is the only place a clause may be authored. Writing into the
    /// live version would be the mid-flight change the amendment path exists to
    /// prevent, and writing into a promoted one would rewrite settled history.
    #[test]
    fn a_clause_cannot_be_authored_into_a_version_that_is_not_a_draft() {
        let e = engine();
        let active = governed(&e);

        let refused = e.draft_clause(
            &active,
            GoalKind::Constraint,
            "Sneak a rule into live policy",
            "This must not take effect without review.",
        );

        assert!(
            refused.is_err(),
            "authoring into the active version must be refused"
        );
    }

    /// Two clauses with one slug in a version would make "which rule governs
    /// this" ambiguous, and the carried-forward copy is already sitting there.
    #[test]
    fn a_clause_cannot_collide_with_one_the_draft_already_carries() {
        let e = engine();
        governed(&e);
        let draft = e.propose_amendment(Some("monk-eee")).unwrap();

        // The draft opened as a copy of live policy, so every active clause is
        // already in it under its own slug.
        let carried = e
            .get_constitution()
            .unwrap()
            .first()
            .map(|clause| clause.title.clone())
            .expect("a governed project has clauses to carry forward");

        let refused = e.draft_clause(
            &draft.id,
            GoalKind::Constraint,
            &carried,
            "A second rule under a slug the draft already holds.",
        );

        assert!(
            refused.is_err(),
            "a slug already in the draft must be refused"
        );
    }
}
