//! Certification as a status over evidence that already exists (ADR-0090).

use super::*;
use crate::waiver::Waiver;
use crate::{CertificationState, CertificationStatus, ClauseCoverage};

impl Lodestar {
    /// Project the qualified certification status a task holds (ADR-0090).
    ///
    /// Verification is the capability; this is the status it produces. Nothing
    /// is judged here that was not judged already: the verdict is the
    /// deterministic conformance record the task closed on, and every other
    /// field qualifies it so the result cannot be read as a framework claim.
    /// That is why there is no state asserting external framework compliance,
    /// and no score — one number hides which obligation failed.
    ///
    /// `at_commit` is the commit the caller is asking about. The server never
    /// reads Git (ADR-0044), so staleness is judged against what the caller
    /// declares: a status whose evidence does not name that commit reports
    /// `stale` rather than quietly standing on evidence for another revision.
    pub fn certification_status(
        &self,
        task_id: &str,
        at_commit: Option<&str>,
    ) -> Result<CertificationStatus> {
        let now = now_unix();
        let task = self
            .store
            .get_task(task_id)?
            .ok_or_else(|| LodestarError::NotFound(task_id.to_string()))?;
        let active_clauses: Vec<String> = self
            .store
            .goals_by_status(GoalStatus::Active)?
            .into_iter()
            .map(|goal| goal.id)
            .collect();
        let policy = self.store.active_constitution_version()?.map(|it| it.id);

        let mut status = CertificationStatus {
            subject: task.id.clone(),
            commit: None,
            policy_version: policy.clone(),
            evidence_bundle: None,
            certified_at: None,
            state: CertificationState::Uncertifiable,
            reason: String::new(),
            covered_nodes: 0,
            coverage: ClauseCoverage {
                evaluated: Vec::new(),
                not_evaluated: sorted(active_clauses.clone()),
            },
            waiver: None,
        };

        if policy.is_none() {
            status.reason =
                "no constitution is adopted, so there is nothing to certify against".to_string();
            return Ok(status);
        }

        // A subject is uncertified until it is verified, which is a different
        // answer from a refusal and reads differently to whoever asked.
        let Some(record) = self
            .conformance_history(&task.id)?
            .into_iter()
            .max_by_key(|record| record.id)
        else {
            status.state = CertificationState::NotCertified;
            status.reason = "this subject has never been verified".to_string();
            return Ok(status);
        };

        let evidence: ConformanceEvidence = serde_json::from_str(&record.evidence)?;
        status.evidence_bundle = Some(record.id);
        status.certified_at = Some(record.checked_at);
        status.commit = evidence.commit_ids.first().cloned();
        status.covered_nodes = evidence.changed_node_ids.len();

        let resolved =
            self.resolve_governing_clauses(&evidence.changed_node_ids, Some(&task.goal_id))?;
        let evaluated: Vec<String> = sorted(
            resolved
                .clauses()
                .into_iter()
                .map(|clause| clause.goal.id)
                .collect(),
        );
        status.coverage.not_evaluated = sorted(
            active_clauses
                .into_iter()
                .filter(|id| !evaluated.contains(id))
                .collect(),
        );
        status.coverage.evaluated = evaluated;

        let (state, reason) = match record.verdict {
            // Agreement about nothing is not proof, and the existing receipt
            // (`TaskReceipt::affirms`) already draws this line — a status that
            // ignored it would certify the emptiest bundle in the ledger.
            Verdict::Aligned if status.covered_nodes == 0 => (
                CertificationState::NotCertified,
                "the verdict was aligned over evidence covering no node".to_string(),
            ),
            Verdict::Aligned => (
                CertificationState::Certified,
                format!(
                    "evidence affirmed this change against {} clause(s) of {}",
                    status.coverage.evaluated.len(),
                    policy.as_deref().unwrap_or("the adopted constitution"),
                ),
            ),
            Verdict::Drift => (
                CertificationState::NotCertified,
                "governed code changed without a covering task".to_string(),
            ),
            Verdict::Violation => (
                CertificationState::NotCertified,
                "the change contradicts a governing clause".to_string(),
            ),
            Verdict::NeedsHuman => (
                CertificationState::NeedsHuman,
                "the check could not decide; a person has to look".to_string(),
            ),
        };
        status.state = state;
        status.reason = reason;

        if record.verdict == Verdict::Violation {
            if let Some(waiver) = self.live_waiver_for(&evidence.changed_node_ids, now)? {
                status.reason = format!(
                    "excepted by waiver {} on clause {}, expiring at {}",
                    waiver.id, waiver.clause_id, waiver.expires_at
                );
                status.state = CertificationState::Waived;
                status.waiver = Some(waiver);
            }
        }

        // Staleness is about the evidence, so it only displaces a status that
        // was otherwise good; a refusal stays reported as the refusal it is.
        if status.state == CertificationState::Certified {
            if let Some(head) = at_commit.map(str::trim).filter(|head| !head.is_empty()) {
                if !evidence
                    .commit_ids
                    .iter()
                    .any(|recorded| same_commit(recorded, head))
                {
                    status.state = CertificationState::Stale;
                    status.reason =
                        format!("the evidence behind this status does not cover commit {head}");
                }
            }
        }

        Ok(status)
    }

    /// The first live waiver reaching any of these nodes.
    fn live_waiver_for(&self, node_ids: &[String], now: i64) -> Result<Option<Waiver>> {
        Ok(self
            .store
            .live_waivers(now)?
            .into_iter()
            .find(|waiver| node_ids.iter().any(|node| waiver.applies_to(node, now))))
    }
}

fn sorted(mut ids: Vec<String>) -> Vec<String> {
    ids.sort();
    ids.dedup();
    ids
}

/// Whether a recorded commit id and a declared one name the same commit.
///
/// The recorded side may carry the graph's `intent:` prefix, and either side may
/// be abbreviated, so this compares the shorter against the longer rather than
/// demanding an exact match a caller has no way to produce.
fn same_commit(recorded: &str, declared: &str) -> bool {
    let recorded = recorded.strip_prefix("intent:").unwrap_or(recorded);
    if recorded.is_empty() {
        return false;
    }
    recorded.starts_with(declared) || declared.starts_with(recorded)
}

#[cfg(test)]
mod tests {
    use super::super::{adopt_constitution, test_evidence};
    use super::*;
    use crate::facade::test_support::engine;
    use crate::GoalKind;

    const NODE: &str = "artifact:src/delivery.rs";

    /// Drive a task to an aligned completion and hand back its id.
    fn certified_task(e: &Lodestar, commits: &[&str]) -> String {
        let goal = e
            .define_goal(GoalKind::Objective, "Ship delivery", "deliver", None)
            .unwrap();
        e.link_goal_to_artifact(&goal.id, &[NODE.into()], ArtifactBindingMode::Governed)
            .unwrap();
        let task = e
            .create_task(&goal.id, "fix delivery", "is proven")
            .unwrap();
        e.claim_task(&task.id, "agent-a", 300).unwrap();
        let claimed = e.store.get_task(&task.id).unwrap().unwrap();
        let mut evidence = test_evidence(Some(task.id.clone()), "agent-a", NODE);
        evidence.started_at = claimed.claim_started_at.unwrap();
        evidence.ended_at = now_unix();
        evidence.commit_ids = commits.iter().map(|sha| sha.to_string()).collect();
        // Every event in a bundle has to be attributed, so a commit id needs the
        // observation edge that says which agent produced it.
        for commit in commits {
            evidence.provenance.push(crate::EvidenceProvenance {
                source_id: "agent:agent-a".into(),
                target_id: (*commit).into(),
                relation: "observed".into(),
            });
        }
        let checked = e.check_conformance(&evidence, Some(&task.id)).unwrap();
        assert_eq!(checked.verdict, Verdict::Aligned, "fixture must be aligned");
        e.complete_task(&task.id, "agent-a", &evidence, &checked, None)
            .unwrap();
        task.id
    }

    /// ADR-0090 §5. "No constitution adopted" and "adopted, and this passed"
    /// are different answers, and a product that renders them the same way is
    /// the badge this decision exists to refuse.
    #[test]
    fn a_subject_is_uncertifiable_until_a_constitution_is_adopted() {
        let e = engine();
        let task = certified_task(&e, &[]);

        let status = e.certification_status(&task, None).unwrap();

        assert_eq!(status.state, CertificationState::Uncertifiable);
        assert_eq!(status.policy_version, None);
        assert!(
            status.reason.contains("no constitution is adopted"),
            "the reason says which of the states this is: {}",
            status.reason
        );
    }

    /// ADR-0090 §6. A subject that has never been verified is uncertified, and
    /// that is a different sentence from "verified and refused".
    #[test]
    fn a_subject_is_not_certified_until_it_has_been_verified() {
        let e = engine();
        adopt_constitution(&e);
        let goal = e
            .define_goal(GoalKind::Objective, "Ship delivery", "deliver", None)
            .unwrap();
        let task = e.create_task(&goal.id, "fix delivery", "unproven").unwrap();

        let status = e.certification_status(&task.id, None).unwrap();

        assert_eq!(status.state, CertificationState::NotCertified);
        assert_eq!(status.evidence_bundle, None);
        assert_eq!(status.reason, "this subject has never been verified");
        assert!(
            !status.coverage.not_evaluated.is_empty(),
            "the adopted clauses it was never judged against are still named"
        );
    }

    /// ADR-0090 §2. The qualifiers are the product. A status that reached
    /// `certified` without naming the policy version it was judged against, the
    /// evidence behind it, and the clauses it covers is the bare badge §2 and
    /// §7 both refuse.
    #[test]
    fn a_certified_status_names_its_policy_version_evidence_and_clause_coverage() {
        let e = engine();
        let version = adopt_constitution(&e);
        let task = certified_task(&e, &["intent:3edc94c77773bec88d51fc777023076fe9ed0ed1"]);

        let status = e.certification_status(&task, None).unwrap();

        assert_eq!(status.state, CertificationState::Certified);
        assert_eq!(status.subject, task);
        assert_eq!(status.policy_version, Some(version.id));
        assert_eq!(
            status.commit.as_deref(),
            Some("intent:3edc94c77773bec88d51fc777023076fe9ed0ed1")
        );
        assert_eq!(status.covered_nodes, 1);
        assert!(status.evidence_bundle.is_some());
        assert!(status.certified_at.is_some());
        assert_eq!(status.coverage.evaluated.len(), 1);
        assert!(
            !status.coverage.not_evaluated.is_empty(),
            "the clauses this status did NOT cover travel with it, so it cannot \
             be read as 'fully compliant'"
        );
        assert!(
            !status
                .coverage
                .evaluated
                .iter()
                .any(|id| status.coverage.not_evaluated.contains(id)),
            "a clause is on exactly one side of the coverage split"
        );
    }

    /// ADR-0090 §6. Certification is bound to a commit and expires when the
    /// subject moves. Displaying staleness is the whole point: a green status
    /// that silently outlives its evidence is worse than no status.
    #[test]
    fn a_status_goes_stale_when_the_subject_moves_past_its_evidence() {
        let e = engine();
        adopt_constitution(&e);
        let task = certified_task(&e, &["intent:3edc94c77773bec88d51fc777023076fe9ed0ed1"]);

        // Abbreviated, which is how a caller actually has the sha to hand.
        let at_evidence = e.certification_status(&task, Some("3edc94c7")).unwrap();
        let moved_on = e.certification_status(&task, Some("0f60005a")).unwrap();

        assert_eq!(at_evidence.state, CertificationState::Certified);
        assert_eq!(moved_on.state, CertificationState::Stale);
        assert!(
            moved_on.reason.contains("0f60005a"),
            "the reason names the commit that is not covered: {}",
            moved_on.reason
        );
    }
}
