//! Constitutional amendments: changing adopted policy explicitly
//! (SPEC-CONSTITUTION §9).
//!
//! A waiver bends a rule once, briefly, for a named reason. An amendment
//! changes the rule. Keeping them separate is what stops the slow failure where
//! a rule is waived so routinely that the waivers *are* the policy, while the
//! written constitution still claims otherwise.
//!
//! Every amendment carries a rationale and an explicit diff, and produces a new
//! version rather than editing the old one. Prior conformance records keep
//! naming the version they were judged under, so a verdict never silently
//! re-reads under rules that did not exist when it was given.

use serde::{Deserialize, Serialize};

use crate::model::Goal;

/// How one clause changed between two constitutional versions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ClauseChange {
    Added,
    Removed,
    Changed,
}

impl ClauseChange {
    pub fn as_str(&self) -> &'static str {
        match self {
            ClauseChange::Added => "added",
            ClauseChange::Removed => "removed",
            ClauseChange::Changed => "changed",
        }
    }
}

/// One clause-level difference, carrying both sides so a reviewer can read the
/// change rather than infer it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ClauseDiff {
    /// The stable identity a clause keeps across versions.
    pub slug: String,
    pub change: ClauseChange,
    /// What the field-level difference actually is, for a `changed` clause.
    pub fields: Vec<String>,
    pub before: Option<Goal>,
    pub after: Option<Goal>,
}

/// A recorded, attributed change of adopted policy.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConstitutionAmendment {
    pub id: String,
    pub from_version: String,
    pub to_version: String,
    pub rationale: String,
    pub amended_by: String,
    pub created_at: i64,
    pub diff: Vec<ClauseDiff>,
}

/// The fields whose change makes a clause a *different rule* rather than a
/// tidier statement of the same one.
///
/// Deliberately includes the enforcement contract, not just the normative text.
/// A clause whose consequence moves from `review` to `block`, or whose scope
/// widens, governs differently even if every word of its statement is
/// unchanged — and that is precisely the amendment a reviewer must not miss.
fn changed_fields(before: &Goal, after: &Goal) -> Vec<String> {
    let mut fields = Vec::new();
    let mut note = |name: &str, differs: bool| {
        if differs {
            fields.push(name.to_string());
        }
    };
    note("kind", before.kind != after.kind);
    note("title", before.title != after.title);
    note("statement", before.statement != after.statement);
    note("rationale", before.rationale != after.rationale);
    note("scope", before.scope != after.scope);
    note(
        "evidence_contract",
        before.evidence_contract != after.evidence_contract,
    );
    note("consequence", before.consequence != after.consequence);
    note("waivable", before.waivable != after.waivable);
    note(
        "waiver_authority",
        before.waiver_authority != after.waiver_authority,
    );
    fields
}

/// Compute the clause-level diff between two constitutional versions.
///
/// Clauses are matched by `slug`, the identity a clause keeps across versions,
/// so re-stating a rule reads as `changed` rather than as a simultaneous
/// removal and addition. The result is sorted by slug: a diff a reviewer has to
/// re-sort mentally is a diff they will skim.
pub fn diff_clauses(before: &[Goal], after: &[Goal]) -> Vec<ClauseDiff> {
    let mut diffs = Vec::new();

    for old in before {
        match after.iter().find(|new| new.slug == old.slug) {
            None => diffs.push(ClauseDiff {
                slug: old.slug.clone(),
                change: ClauseChange::Removed,
                fields: Vec::new(),
                before: Some(old.clone()),
                after: None,
            }),
            Some(new) => {
                let fields = changed_fields(old, new);
                if !fields.is_empty() {
                    diffs.push(ClauseDiff {
                        slug: old.slug.clone(),
                        change: ClauseChange::Changed,
                        fields,
                        before: Some(old.clone()),
                        after: Some(new.clone()),
                    });
                }
            }
        }
    }

    for new in after {
        if !before.iter().any(|old| old.slug == new.slug) {
            diffs.push(ClauseDiff {
                slug: new.slug.clone(),
                change: ClauseChange::Added,
                fields: Vec::new(),
                before: None,
                after: Some(new.clone()),
            });
        }
    }

    diffs.sort_by(|left, right| left.slug.cmp(&right.slug));
    diffs
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{ClauseOrigin, Consequence};
    use crate::{GoalKind, GoalStatus};

    fn clause(slug: &str) -> Goal {
        Goal {
            id: format!("goal:{slug}"),
            slug: slug.into(),
            kind: GoalKind::Invariant,
            title: "A rule".into(),
            statement: "Something must hold.".into(),
            status: GoalStatus::Active,
            version: 1,
            parent_id: None,
            superseded_by: None,
            reason: None,
            created_at: 1,
            constitution_version: Some("constitution:v1".into()),
            rationale: None,
            scope: Some("artifact:crates/**".into()),
            evidence_contract: Some("tests".into()),
            consequence: Some(Consequence::Review),
            waivable: false,
            waiver_authority: None,
            origin: ClauseOrigin::Local,
        }
    }

    #[test]
    fn an_unchanged_clause_produces_no_diff_entry() {
        assert!(diff_clauses(&[clause("a")], &[clause("a")]).is_empty());
    }

    #[test]
    fn a_restated_clause_reads_as_changed_not_as_a_removal_and_an_addition() {
        // Matching on slug rather than id is what makes this true. Otherwise
        // every reworded rule would look like the old one being deleted and an
        // unrelated new one appearing.
        let mut after = clause("a");
        after.id = "goal:a-v2".into();
        after.statement = "Something stricter must hold.".into();

        let diff = diff_clauses(&[clause("a")], &[after]);
        assert_eq!(diff.len(), 1);
        assert_eq!(diff[0].change, ClauseChange::Changed);
        assert_eq!(diff[0].fields, vec!["statement"]);
    }

    #[test]
    fn a_clause_that_only_hardens_its_enforcement_still_reads_as_changed() {
        // The dangerous quiet amendment: identical words, different force. A
        // diff that only compared statements would report nothing at all.
        let mut after = clause("a");
        after.consequence = Some(Consequence::Block);
        after.scope = Some("artifact:**".into());

        let diff = diff_clauses(&[clause("a")], &[after]);
        assert_eq!(diff.len(), 1);
        assert_eq!(diff[0].change, ClauseChange::Changed);
        assert_eq!(diff[0].fields, vec!["scope", "consequence"]);
    }

    #[test]
    fn a_clause_becoming_waivable_is_reported() {
        // Making a rule excusable is a policy change, not an administrative one.
        let mut after = clause("a");
        after.waivable = true;
        after.waiver_authority = Some("security-team".into());

        let diff = diff_clauses(&[clause("a")], &[after]);
        assert_eq!(diff[0].fields, vec!["waivable", "waiver_authority"]);
    }

    #[test]
    fn additions_and_removals_are_reported_with_the_side_that_exists() {
        let diff = diff_clauses(&[clause("gone")], &[clause("fresh")]);
        assert_eq!(diff.len(), 2);
        // Sorted by slug, so "fresh" precedes "gone".
        assert_eq!(diff[0].change, ClauseChange::Added);
        assert!(diff[0].before.is_none());
        assert_eq!(diff[1].change, ClauseChange::Removed);
        assert!(diff[1].after.is_none());
    }
}
