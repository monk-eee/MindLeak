//! Bounded exceptions to constitutional clauses (SPEC-CONSTITUTION §9).
//!
//! An exception is not a hidden bypass. `--no-verify` and a commented-out check
//! are exceptions too — they are just unattributed, unbounded, and invisible.
//! A waiver is the same act made reviewable: it names who allowed it, over what
//! scope, for how long, and what has to happen before it lapses.
//!
//! The load-bearing rule is that **a waiver always ends**. There is no
//! open-ended waiver, because an exception that never expires is not an
//! exception — it is the policy, and changing the policy is an amendment. That
//! single refusal is what stops the waiver table from silently becoming a
//! second constitution nobody reviewed.

use serde::{Deserialize, Serialize};

use crate::error::{LodestarError, Result};
use crate::model::Goal;
use crate::scope;

/// Whether a waiver was withdrawn before its expiry.
///
/// Expiry is deliberately *not* a status. A lapsed waiver stays `Active` in the
/// record and simply stops matching, so history reads as it was judged rather
/// than being rewritten by the passage of time (§9: expiry restores enforcement
/// without mutating history).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum WaiverStatus {
    Active,
    Revoked,
}

impl WaiverStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            WaiverStatus::Active => "active",
            WaiverStatus::Revoked => "revoked",
        }
    }

    pub fn from_tag(tag: &str) -> Option<Self> {
        match tag {
            "active" => Some(WaiverStatus::Active),
            "revoked" => Some(WaiverStatus::Revoked),
            _ => None,
        }
    }
}

/// A scoped, time-bounded, attributed exception to one clause.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Waiver {
    pub id: String,
    pub clause_id: String,
    /// The constitutional version in force when the waiver was granted. A
    /// waiver authorises an exception to *that* policy; an amendment leaves it
    /// naming a version that no longer governs.
    pub constitution_version: Option<String>,
    pub scope: String,
    pub reason: String,
    pub approved_by: String,
    pub created_at: i64,
    /// Always set. §9 has no unbounded waiver: a permanent exception is an
    /// amendment.
    pub expires_at: i64,
    /// The work that makes the exception unnecessary. Optional, because not
    /// every exception has a fix — but its absence is worth seeing.
    pub remediation_task_id: Option<String>,
    pub status: WaiverStatus,
    pub revoked_by: Option<String>,
    pub revoked_at: Option<i64>,
    pub revocation_reason: Option<String>,
}

impl Waiver {
    /// Whether this waiver excuses `target` at `now`.
    ///
    /// Three independent conditions, all computed rather than stored: it was
    /// not revoked, it has not lapsed, and its declared scope reaches the thing
    /// being judged.
    pub fn applies_to(&self, target: &str, now: i64) -> bool {
        self.status == WaiverStatus::Active
            && now < self.expires_at
            && scope::covers(&self.scope, target)
    }

    /// Whether this waiver is still capable of excusing anything at `now`.
    pub fn is_live(&self, now: i64) -> bool {
        self.status == WaiverStatus::Active && now < self.expires_at
    }
}

/// What a caller must supply to grant a waiver.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WaiverRequest {
    pub clause_id: String,
    pub scope: String,
    pub reason: String,
    pub approved_by: String,
    pub expires_at: i64,
    pub remediation_task_id: Option<String>,
}

/// Check a waiver request against the clause it would except.
///
/// Every refusal here is a case where granting would produce a waiver that
/// reads as legitimate but is not:
///
/// - a clause that **declares itself unwaivable** has already answered this;
///   granting anyway would make `waivable: false` decorative;
/// - a clause naming a **required authority** is naming who may decide, and an
///   exception approved by someone else is exactly the bypass §9 exists to
///   prevent;
/// - an **absent or past expiry** is a permanent exception wearing a waiver's
///   clothes, which §9 says must be an amendment instead;
/// - an **unattributed** waiver cannot be reviewed, and an unexplained one
///   cannot be judged when it comes up for renewal.
pub fn validate_request(clause: &Goal, request: &WaiverRequest, now: i64) -> Result<()> {
    let invalid = |message: String| Err(LodestarError::Invalid(message));

    if request.approved_by.trim().is_empty() {
        return invalid("a waiver requires an attributed approver".to_string());
    }
    if request.reason.trim().is_empty() {
        return invalid("a waiver requires a reason".to_string());
    }
    if request.scope.trim().is_empty() {
        return invalid("a waiver requires a scope; there is no blanket waiver".to_string());
    }
    if !clause.waivable {
        return invalid(format!(
            "clause {} declares itself unwaivable; changing that is an amendment, not an exception",
            clause.id
        ));
    }
    if let Some(authority) = clause.waiver_authority.as_deref() {
        if !authority.trim().is_empty() && authority.trim() != request.approved_by.trim() {
            return invalid(format!(
                "clause {} requires waiver authority {authority}; {} cannot approve this exception",
                clause.id, request.approved_by
            ));
        }
    }
    if request.expires_at <= now {
        return invalid(
            "a waiver must expire in the future; a permanent exception is an amendment".to_string(),
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::ClauseOrigin;
    use crate::{GoalKind, GoalStatus};

    const NOW: i64 = 1_000;

    fn clause(waivable: bool, authority: Option<&str>) -> Goal {
        Goal {
            id: "goal:secrets".into(),
            slug: "secrets".into(),
            kind: GoalKind::Invariant,
            title: "Protect the security boundary".into(),
            statement: "Never commit credentials.".into(),
            status: GoalStatus::Active,
            version: 1,
            parent_id: None,
            superseded_by: None,
            reason: None,
            created_at: 1,
            constitution_version: Some("constitution:v1".into()),
            rationale: None,
            scope: Some("artifact:crates/**".into()),
            evidence_contract: None,
            consequence: Some(crate::model::Consequence::Block),
            waivable,
            waiver_authority: authority.map(|a| a.to_string()),
            origin: ClauseOrigin::Local,
        }
    }

    fn request() -> WaiverRequest {
        WaiverRequest {
            clause_id: "goal:secrets".into(),
            scope: "artifact:crates/lodestar-core/src/lib.rs".into(),
            reason: "Hotfix for the release blocker; remediation tracked.".into(),
            approved_by: "monk-eee".into(),
            expires_at: NOW + 3_600,
            remediation_task_id: Some("task:fix".into()),
        }
    }

    fn waiver(scope: &str, expires_at: i64, status: WaiverStatus) -> Waiver {
        Waiver {
            id: "waiver:1".into(),
            clause_id: "goal:secrets".into(),
            constitution_version: Some("constitution:v1".into()),
            scope: scope.into(),
            reason: "Hotfix".into(),
            approved_by: "monk-eee".into(),
            created_at: NOW,
            expires_at,
            remediation_task_id: None,
            status,
            revoked_by: None,
            revoked_at: None,
            revocation_reason: None,
        }
    }

    #[test]
    fn an_unwaivable_clause_cannot_be_excepted() {
        // Otherwise `waivable: false` is decorative.
        let error = validate_request(&clause(false, None), &request(), NOW).unwrap_err();
        assert!(format!("{error}").contains("unwaivable"), "{error}");
    }

    #[test]
    fn only_the_declared_authority_may_approve() {
        // A clause naming an authority is naming who may decide; an exception
        // approved by anyone else is the bypass §9 exists to prevent.
        let clause = clause(true, Some("security-team"));
        let error = validate_request(&clause, &request(), NOW).unwrap_err();
        assert!(format!("{error}").contains("waiver authority"), "{error}");

        let mut allowed = request();
        allowed.approved_by = "security-team".into();
        assert!(validate_request(&clause, &allowed, NOW).is_ok());
    }

    #[test]
    fn a_waiver_must_expire_in_the_future() {
        // A permanent exception is an amendment. Without this refusal the
        // waiver table quietly becomes a second constitution nobody reviewed.
        for expiry in [NOW, NOW - 1, 0] {
            let mut permanent = request();
            permanent.expires_at = expiry;
            let error = validate_request(&clause(true, None), &permanent, NOW).unwrap_err();
            assert!(format!("{error}").contains("amendment"), "{error}");
        }
    }

    #[test]
    fn a_waiver_needs_an_approver_a_reason_and_a_scope() {
        let clause = clause(true, None);
        for mutate in [
            (|r: &mut WaiverRequest| r.approved_by = "  ".into()) as fn(&mut WaiverRequest),
            |r: &mut WaiverRequest| r.reason = String::new(),
            |r: &mut WaiverRequest| r.scope = String::new(),
        ] {
            let mut bad = request();
            mutate(&mut bad);
            assert!(validate_request(&clause, &bad, NOW).is_err());
        }
    }

    #[test]
    fn a_lapsed_waiver_stops_matching_without_changing_status() {
        // §9: expiry restores enforcement automatically and does not mutate
        // history, so the record still reads `active` after it stops applying.
        let lapsed = waiver("artifact:crates/**", NOW + 10, WaiverStatus::Active);
        assert!(lapsed.applies_to("artifact:crates/a.rs", NOW));
        assert!(!lapsed.applies_to("artifact:crates/a.rs", NOW + 10));
        assert_eq!(lapsed.status, WaiverStatus::Active);
    }

    #[test]
    fn a_revoked_waiver_stops_matching_immediately() {
        let revoked = waiver("artifact:crates/**", NOW + 10_000, WaiverStatus::Revoked);
        assert!(!revoked.applies_to("artifact:crates/a.rs", NOW));
    }

    #[test]
    fn a_waiver_excuses_only_what_its_scope_reaches() {
        let narrow = waiver(
            "artifact:crates/lodestar-core/src/lib.rs",
            NOW + 10_000,
            WaiverStatus::Active,
        );
        assert!(narrow.applies_to("artifact:crates/lodestar-core/src/lib.rs", NOW));
        assert!(!narrow.applies_to("artifact:crates/lodestar-core/src/db.rs", NOW));
    }
}
