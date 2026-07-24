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
