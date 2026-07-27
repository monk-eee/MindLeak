use std::collections::HashSet;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::model::Consequence;
use crate::{GoalKind, LodestarError, Result};

/// Immutable, versioned input to constitutional drafting
/// (SPEC-CONSTITUTION section 6).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConstitutionPack {
    pub id: String,
    pub version: String,
    pub digest: String,
    pub title: String,
    pub description: String,
    pub compatible_engine_versions: Vec<String>,
    pub preamble_fragments: Vec<String>,
    pub clauses: Vec<PackClause>,
    pub conflicts: Vec<PackConflict>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PackClause {
    pub key: String,
    pub kind: GoalKind,
    pub title: String,
    pub statement: String,
    pub rationale: String,
    pub default_scope: Option<String>,
    pub evidence_contract: Option<String>,
    pub default_consequence: Option<Consequence>,
    #[serde(default)]
    pub suggested_controls: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PackConflict {
    pub pack_id: String,
    pub reason: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PackClauseDisposition {
    Adopted,
    Tailored,
    Rejected,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PackClauseProposal {
    pub id: String,
    pub pack_id: String,
    pub pack_version: String,
    pub pack_digest: String,
    pub constitution_version: Option<String>,
    pub clause: PackClause,
    pub disposition: Option<PackClauseDisposition>,
    pub reviewed_by: Option<String>,
    pub review_reason: Option<String>,
    pub reviewed_at: Option<i64>,
    pub adopted_goal_id: Option<String>,
    pub created_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PackProposalBatch {
    pub proposals: Vec<PackClauseProposal>,
    pub conflicts: Vec<PackConflict>,
    pub needs_human: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PackClauseProvenance {
    pub goal_id: String,
    pub pack_id: String,
    pub pack_version: String,
    pub pack_digest: String,
    pub clause_key: String,
    pub source_clause: PackClause,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackReviewOutcome {
    pub proposal: PackClauseProposal,
    pub goal: Option<crate::Goal>,
}

impl PackClauseDisposition {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Adopted => "adopted",
            Self::Tailored => "tailored",
            Self::Rejected => "rejected",
        }
    }

    pub fn from_tag(tag: &str) -> Option<Self> {
        match tag {
            "adopted" => Some(Self::Adopted),
            "tailored" => Some(Self::Tailored),
            "rejected" => Some(Self::Rejected),
            _ => None,
        }
    }
}

impl ConstitutionPack {
    /// Validate the declared schema and require the supplied digest to match the
    /// canonical serialized content (all fields except the digest itself).
    pub fn validate(&self) -> Result<()> {
        if !valid_key(&self.id) {
            return Err(LodestarError::Invalid(
                "policy pack id must contain only lowercase ASCII letters, digits, '.', '_', or '-'"
                    .to_string(),
            ));
        }
        if self.version.trim().is_empty() || self.title.trim().is_empty() {
            return Err(LodestarError::Invalid(
                "policy pack version and title are required".to_string(),
            ));
        }
        if self.compatible_engine_versions.is_empty()
            || self
                .compatible_engine_versions
                .iter()
                .any(|version| version.trim().is_empty())
        {
            return Err(LodestarError::Invalid(
                "policy pack must declare at least one compatible engine version".to_string(),
            ));
        }
        if self.clauses.is_empty() {
            return Err(LodestarError::Invalid(
                "policy pack must contain at least one clause".to_string(),
            ));
        }
        let mut keys = HashSet::new();
        for clause in &self.clauses {
            if !valid_key(&clause.key)
                || clause.title.trim().is_empty()
                || clause.statement.trim().is_empty()
                || clause.rationale.trim().is_empty()
            {
                return Err(LodestarError::Invalid(format!(
                    "policy pack clause {} has an invalid key or missing title, statement, or rationale",
                    clause.key
                )));
            }
            if !keys.insert(&clause.key) {
                return Err(LodestarError::Invalid(format!(
                    "policy pack contains duplicate clause key {}",
                    clause.key
                )));
            }
        }
        for conflict in &self.conflicts {
            if !valid_key(&conflict.pack_id) || conflict.reason.trim().is_empty() {
                return Err(LodestarError::Invalid(
                    "policy pack conflicts require a valid pack id and reason".to_string(),
                ));
            }
        }
        let expected = self.computed_digest()?;
        if self.digest != expected {
            return Err(LodestarError::Invalid(format!(
                "policy pack digest mismatch: expected {expected}"
            )));
        }
        Ok(())
    }

    pub fn computed_digest(&self) -> Result<String> {
        #[derive(Serialize)]
        struct DigestContent<'a> {
            id: &'a str,
            version: &'a str,
            title: &'a str,
            description: &'a str,
            compatible_engine_versions: &'a [String],
            preamble_fragments: &'a [String],
            clauses: &'a [PackClause],
            conflicts: &'a [PackConflict],
        }

        let bytes = serde_json::to_vec(&DigestContent {
            id: &self.id,
            version: &self.version,
            title: &self.title,
            description: &self.description,
            compatible_engine_versions: &self.compatible_engine_versions,
            preamble_fragments: &self.preamble_fragments,
            clauses: &self.clauses,
            conflicts: &self.conflicts,
        })?;
        Ok(format!("{:x}", Sha256::digest(bytes)))
    }
}

pub fn common_core_pack() -> ConstitutionPack {
    let principles = [
        (
            "core.evidence",
            "Evidence before claims",
            "Do not claim success without relevant, fresh evidence.",
            "Tests, compile and lint results, benchmarks, or review evidence should match the risk of the claim.",
        ),
        (
            "core.intent",
            "Preserve project intent",
            "Preserve declared project intent and unrelated human work.",
            "Scoped diffs, task and goal linkage, and non-destructive handling of unrelated work keep intent attributable.",
        ),
        (
            "core.safety",
            "Protect the security boundary",
            "Protect secrets, sensitive data, and the project's security boundary.",
            "Security-specific checks and reviewed constraints should match the project's threat model.",
        ),
        (
            "core.proportionality",
            "Act proportionally",
            "Keep change and validation proportional to impact and reversibility.",
            "Use focused validation for narrow changes and broader proof for shared or irreversible contracts.",
        ),
        (
            "core.evolution",
            "Evolve policy explicitly",
            "Change policy through explicit amendment or bounded exception, never silent drift.",
            "Version chains, attributed rationale, and expiring waivers make policy evolution reviewable.",
        ),
    ];
    let clauses = principles
        .into_iter()
        .map(|(key, title, statement, rationale)| PackClause {
            key: key.to_string(),
            kind: GoalKind::Principle,
            title: title.to_string(),
            statement: statement.to_string(),
            rationale: rationale.to_string(),
            default_scope: None,
            evidence_contract: None,
            default_consequence: Some(Consequence::Review),
            suggested_controls: Vec::new(),
        })
        .collect();
    let mut pack = ConstitutionPack {
        id: "common-core".to_string(),
        version: "1".to_string(),
        digest: String::new(),
        title: "Lodestar Common Core".to_string(),
        description: "Five review-first principles proposed to every project, never imposed."
            .to_string(),
        compatible_engine_versions: vec!["*".to_string()],
        preamble_fragments: Vec::new(),
        clauses,
        conflicts: Vec::new(),
    };
    pack.digest = pack
        .computed_digest()
        .expect("Common Core serialization is infallible");
    pack
}

/// The optional `fleet-delivery` pack (ADR-0034): how a fleet of agents lands
/// work safely.
///
/// Unlike the Common Core these are mostly enforceable clauses rather than
/// principles, because each names a concrete scope and evidence contract. They
/// are still only *proposals*: shipping pack bytes is not enforcement, and every
/// clause needs an explicit adopt/tailor/reject before it governs anything.
///
/// `default_consequence` is what the clause asks for. What it actually gets is
/// bounded at resolution time by the power of whatever control backs it, so a
/// clause declaring `block` with only an advisory mechanism still resolves at
/// `review`.
pub fn fleet_delivery_pack() -> ConstitutionPack {
    let clauses = vec![
        PackClause {
            key: "fleet.protected_branch".to_string(),
            kind: GoalKind::Invariant,
            title: "A protected branch advances only by reviewed merge".to_string(),
            statement:
                "A protected branch advances only through a reviewed pull request whose required checks passed. No agent pushes directly to a protected branch."
                    .to_string(),
            rationale:
                "Review is where a fleet's work becomes one history someone actually read. A direct push skips the only step that catches an agent confidently doing the wrong thing."
                    .to_string(),
            default_scope: Some("workflow:git.publish".to_string()),
            evidence_contract: Some(
                "The push target ref, the pull request that merged it, and the conclusion of its required checks."
                    .to_string(),
            ),
            default_consequence: Some(Consequence::Block),
            suggested_controls: vec![
                "pre-push hook refusing a protected branch".to_string(),
                "server-side branch protection".to_string(),
                "history inspection for a non-merge advance".to_string(),
            ],
        },
        PackClause {
            key: "fleet.single_publisher".to_string(),
            kind: GoalKind::Constraint,
            title: "One designated publisher per fleet branch".to_string(),
            statement:
                "Exactly one designated integrator publishes a fleet branch, pushing the current branch's exact HEAD from the primary checkout."
                    .to_string(),
            rationale:
                "Concurrent publishers reconcile the same divergence differently and produce competing histories; one publisher makes integration a decision rather than a race."
                    .to_string(),
            default_scope: Some("workflow:git.publish".to_string()),
            evidence_contract: Some(
                "The publishing identity, the checkout it published from, and the exact HEAD pushed."
                    .to_string(),
            ),
            default_consequence: Some(Consequence::Block),
            suggested_controls: vec!["canonical publisher refusals".to_string()],
        },
        PackClause {
            key: "fleet.commit_identity".to_string(),
            kind: GoalKind::Invariant,
            title: "One logical change has one commit identity".to_string(),
            statement:
                "Work already published under one commit id is not republished under another. Cherry-pick is reserved for a declared backport or human-approved recovery."
                    .to_string(),
            rationale:
                "Intent and conformance evidence are addressed by commit id. Routine cherry-picking gives one logical change two identities and splits its provenance in half."
                    .to_string(),
            default_scope: Some("workflow:git.publish".to_string()),
            evidence_contract: Some(
                "The published commit ids and whether the remote tip is an ancestor of the pushed HEAD."
                    .to_string(),
            ),
            default_consequence: Some(Consequence::Block),
            suggested_controls: vec![
                "non-ancestor push refusal".to_string(),
                "patch-id comparison against remote ancestry".to_string(),
            ],
        },
        PackClause {
            key: "fleet.scoped_commit".to_string(),
            kind: GoalKind::Constraint,
            title: "A commit stays inside its declared scope".to_string(),
            statement:
                "An agent commits only paths within the scope it declared on its claim; the staged set may not escape that scope."
                    .to_string(),
            rationale:
                "Agents share one index. An unscoped commit silently carries another agent's work under the wrong attribution, which is unrecoverable once history moves on."
                    .to_string(),
            default_scope: Some("workflow:git.commit".to_string()),
            evidence_contract: Some(
                "The staged path set compared against the claim's declared scope.".to_string(),
            ),
            default_consequence: Some(Consequence::Block),
            suggested_controls: vec!["scoped-commit guard".to_string()],
        },
        PackClause {
            key: "fleet.branch_freshness".to_string(),
            kind: GoalKind::Constraint,
            title: "Work starts from, and lands close to, the current head".to_string(),
            statement:
                "Work is based on a recent head and reconciled before publication rather than after divergence has accumulated."
                    .to_string(),
            rationale:
                "Staleness is cheap to fix early and expensive later. Divergence discovered at the publisher or the merge has already cost the work it conflicts with."
                    .to_string(),
            default_scope: Some("workflow:git".to_string()),
            evidence_contract: Some(
                "Commits behind the declared base at claim time and at publication."
                    .to_string(),
            ),
            default_consequence: Some(Consequence::Review),
            suggested_controls: vec![
                "required up-to-date branch before merge".to_string(),
                "declared session context staleness (ADR-0035)".to_string(),
            ],
        },
        PackClause {
            key: "fleet.topology_honesty".to_string(),
            kind: GoalKind::Principle,
            title: "Declared working topology matches actual".to_string(),
            statement:
                "The checkout topology an agent declares matches the one it is working in."
                    .to_string(),
            rationale:
                "This does not prefer a topology. It exists so coordination advice is computed from what is true rather than from what was assumed."
                    .to_string(),
            default_scope: Some("workflow:topology".to_string()),
            evidence_contract: Some(
                "The declared branch and checkout compared with the observed ones.".to_string(),
            ),
            default_consequence: Some(Consequence::Review),
            suggested_controls: vec!["declared versus observed comparison".to_string()],
        },
    ];
    let mut pack = ConstitutionPack {
        id: "fleet-delivery".to_string(),
        version: "1".to_string(),
        digest: String::new(),
        title: "Fleet delivery".to_string(),
        description:
            "How a fleet of agents lands work safely: review, publication, commit identity, scope, and freshness. Proposed, never imposed."
                .to_string(),
        compatible_engine_versions: vec!["*".to_string()],
        preamble_fragments: Vec::new(),
        clauses,
        conflicts: Vec::new(),
    };
    pack.digest = pack
        .computed_digest()
        .expect("fleet-delivery serialization is infallible");
    pack
}

fn valid_key(value: &str) -> bool {
    !value.is_empty()
        && value.chars().all(|character| {
            character.is_ascii_lowercase()
                || character.is_ascii_digit()
                || matches!(character, '.' | '_' | '-')
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn common_core_is_a_valid_stable_five_principle_pack() {
        let pack = common_core_pack();
        pack.validate().unwrap();
        assert_eq!(pack.clauses.len(), 5);
        assert!(pack
            .clauses
            .iter()
            .all(|clause| clause.kind == GoalKind::Principle));
        assert_eq!(
            pack.clauses
                .iter()
                .map(|clause| clause.key.as_str())
                .collect::<Vec<_>>(),
            vec![
                "core.evidence",
                "core.intent",
                "core.safety",
                "core.proportionality",
                "core.evolution",
            ]
        );
        assert_eq!(pack.compatible_engine_versions, vec!["*"]);
    }

    #[test]
    fn validation_rejects_digest_mismatch_and_duplicate_clause_keys() {
        let mut pack = common_core_pack();
        pack.digest = "wrong".to_string();
        assert!(pack.validate().unwrap_err().to_string().contains("digest"));

        let mut pack = common_core_pack();
        pack.clauses.push(pack.clauses[0].clone());
        pack.digest = pack.computed_digest().unwrap();
        assert!(pack
            .validate()
            .unwrap_err()
            .to_string()
            .contains("duplicate clause key"));
    }
}
