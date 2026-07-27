//! Deterministic, read-only discovery of cited project facts
//! (SPEC-CONSTITUTION §7.2).
//!
//! Bootstrap needs to ground a constitutional draft in what a repository
//! actually does, without inventing authority it has not been given. Two rules
//! shape everything here:
//!
//! 1. **Facts are cited, never clauses.** Discovery reports "this repository
//!    runs CI at `.github/workflows/ci.yml`". It never concludes "therefore CI
//!    must pass". Turning a fact into policy is a human decision (§7.4).
//! 2. **Ambiguity stays a question.** An existing gate is evidence that the
//!    project uses a mechanism; it is not proof of the *reason*, scope, or
//!    desired consequence. Every mechanism fact therefore carries the question
//!    a maintainer still has to answer.
//!
//! Classification is by path alone. Parsing manifests, workflow YAML, or lint
//! configuration to extract thresholds would mean guessing intent from
//! configuration, which is exactly what rule 2 forbids — and it would make
//! discovery depend on a parser per ecosystem. The caller supplies the paths, so
//! this module performs no filesystem scan of its own and stays a pure function
//! that fixtures can drive.

use serde::{Deserialize, Serialize};

/// The repository surface a fact came from (SPEC-CONSTITUTION §7.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProjectFactKind {
    /// Stated intent: README, AGENTS.md, contributing guidance.
    Documentation,
    /// An architecture decision record.
    DecisionRecord,
    /// A dependency/build manifest.
    Manifest,
    /// A continuous integration definition.
    ContinuousIntegration,
    /// Formatting or lint configuration.
    Linter,
    /// Test runner configuration.
    TestConfiguration,
    /// Declared ownership or review routing.
    Ownership,
}

impl ProjectFactKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            ProjectFactKind::Documentation => "documentation",
            ProjectFactKind::DecisionRecord => "decision_record",
            ProjectFactKind::Manifest => "manifest",
            ProjectFactKind::ContinuousIntegration => "continuous_integration",
            ProjectFactKind::Linter => "linter",
            ProjectFactKind::TestConfiguration => "test_configuration",
            ProjectFactKind::Ownership => "ownership",
        }
    }

    pub fn from_tag(tag: &str) -> Option<Self> {
        match tag {
            "documentation" => Some(ProjectFactKind::Documentation),
            "decision_record" => Some(ProjectFactKind::DecisionRecord),
            "manifest" => Some(ProjectFactKind::Manifest),
            "continuous_integration" => Some(ProjectFactKind::ContinuousIntegration),
            "linter" => Some(ProjectFactKind::Linter),
            "test_configuration" => Some(ProjectFactKind::TestConfiguration),
            "ownership" => Some(ProjectFactKind::Ownership),
            _ => None,
        }
    }
}

/// One cited observation about a repository. `question` is present whenever the
/// fact evidences a *mechanism* whose intent the repository does not state, so a
/// draft can surface it for deliberation instead of assuming a reason.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectFact {
    pub kind: ProjectFactKind,
    pub source_path: String,
    pub detail: String,
    pub question: Option<String>,
}

fn basename(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

fn is_adr(path: &str) -> bool {
    path.contains("docs/adr/") && path.ends_with(".md") && !path.ends_with("/README.md")
}

fn is_workflow(path: &str) -> bool {
    path.contains(".github/workflows/") && (path.ends_with(".yml") || path.ends_with(".yaml"))
}

fn classify(path: &str) -> Option<(ProjectFactKind, String, Option<String>)> {
    let name = basename(path);
    let lower = name.to_ascii_lowercase();

    if is_adr(path) {
        return Some((
            ProjectFactKind::DecisionRecord,
            "records an architecture decision".to_string(),
            None,
        ));
    }
    if is_workflow(path) {
        return Some((
            ProjectFactKind::ContinuousIntegration,
            "defines automated checks that run on change".to_string(),
            Some(
                "which of these checks are required, and what should happen when one fails?"
                    .to_string(),
            ),
        ));
    }

    let documentation = match lower.as_str() {
        // Only the repository-root README states project purpose; a nested one
        // (an ADR index, a crate readme) is navigation, not stated intent.
        "readme.md" | "readme" if !path.contains('/') => Some("states what the project is for"),
        "agents.md" => Some("states constraints the project places on automated contributors"),
        "contributing.md" => Some("states how a change is expected to be made"),
        "security.md" => Some("states how security concerns are reported and handled"),
        "rationale.md" => Some("states why the project is shaped the way it is"),
        _ => None,
    };
    if let Some(detail) = documentation {
        return Some((ProjectFactKind::Documentation, detail.to_string(), None));
    }

    let manifest = matches!(
        lower.as_str(),
        "cargo.toml" | "package.json" | "go.mod" | "pyproject.toml" | "setup.cfg"
    ) || (lower.starts_with("requirements") && lower.ends_with(".txt"));
    if manifest {
        return Some((
            ProjectFactKind::Manifest,
            "declares dependencies and build metadata".to_string(),
            None,
        ));
    }

    let linter = matches!(
        lower.as_str(),
        ".pre-commit-config.yaml"
            | ".editorconfig"
            | "rustfmt.toml"
            | "clippy.toml"
            | ".prettierrc"
            | ".prettierrc.json"
            | ".eslintrc"
            | ".eslintrc.json"
            | ".eslintrc.cjs"
            | "eslint.config.js"
            | "eslint.config.mjs"
    );
    if linter {
        return Some((
            ProjectFactKind::Linter,
            "configures formatting or static analysis".to_string(),
            Some("is this advisory, or must it pass before a change lands?".to_string()),
        ));
    }

    let test_config = matches!(lower.as_str(), "pytest.ini" | "tox.ini")
        || lower.starts_with("vitest.config.")
        || lower.starts_with("jest.config.");
    if test_config {
        return Some((
            ProjectFactKind::TestConfiguration,
            "configures the test runner".to_string(),
            Some(
                "what level of evidence does this project expect before a change is called done?"
                    .to_string(),
            ),
        ));
    }

    if lower == "codeowners" {
        return Some((
            ProjectFactKind::Ownership,
            "declares who owns or reviews parts of the tree".to_string(),
            Some("who holds amendment and exception authority?".to_string()),
        ));
    }

    None
}

/// Classify explicit repository paths into cited project facts.
///
/// Deterministic and model-free: the same input always yields the same output,
/// ordered by kind then path. Unrecognised paths are ignored rather than guessed
/// at, so discovery never manufactures a fact it cannot cite.
pub fn discover_project_facts(paths: &[String]) -> Vec<ProjectFact> {
    let mut facts: Vec<ProjectFact> = paths
        .iter()
        .map(|path| path.replace('\\', "/"))
        .filter_map(|path| {
            classify(&path).map(|(kind, detail, question)| ProjectFact {
                kind,
                source_path: path,
                detail,
                question,
            })
        })
        .collect();
    facts.sort_by(|left, right| {
        left.kind
            .cmp(&right.kind)
            .then_with(|| left.source_path.cmp(&right.source_path))
    });
    facts.dedup();
    facts
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paths(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| value.to_string()).collect()
    }

    #[test]
    fn every_documented_surface_is_cited_to_its_own_path() {
        let facts = discover_project_facts(&paths(&[
            "README.md",
            "AGENTS.md",
            "docs/CONTRIBUTING.md",
            "docs/adr/0026-constitution.md",
            "Cargo.toml",
            ".github/workflows/ci.yml",
            ".pre-commit-config.yaml",
            "editors/vscode/vitest.config.ts",
            "CODEOWNERS",
        ]));

        assert_eq!(facts.len(), 9);
        for fact in &facts {
            assert!(
                !fact.source_path.is_empty(),
                "every fact must cite its source"
            );
        }
        let kinds: Vec<_> = facts.iter().map(|fact| fact.kind).collect();
        assert!(kinds.contains(&ProjectFactKind::Documentation));
        assert!(kinds.contains(&ProjectFactKind::DecisionRecord));
        assert!(kinds.contains(&ProjectFactKind::Manifest));
        assert!(kinds.contains(&ProjectFactKind::ContinuousIntegration));
        assert!(kinds.contains(&ProjectFactKind::Linter));
        assert!(kinds.contains(&ProjectFactKind::TestConfiguration));
        assert!(kinds.contains(&ProjectFactKind::Ownership));
    }

    #[test]
    fn a_mechanism_carries_the_question_its_configuration_cannot_answer() {
        // SPEC-CONSTITUTION §7.2: a gate proves a mechanism exists, never the
        // reason, scope, or desired consequence.
        let facts = discover_project_facts(&paths(&[
            ".github/workflows/ci.yml",
            ".pre-commit-config.yaml",
            "pytest.ini",
            "CODEOWNERS",
        ]));
        assert!(facts.iter().all(|fact| fact.question.is_some()));
    }

    #[test]
    fn stated_intent_needs_no_question_because_the_project_already_says_it() {
        let facts = discover_project_facts(&paths(&["README.md", "AGENTS.md"]));
        assert!(facts.iter().all(|fact| fact.question.is_none()));
    }

    #[test]
    fn unrecognised_paths_are_ignored_rather_than_guessed_at() {
        let facts = discover_project_facts(&paths(&[
            "src/main.rs",
            "target/debug/build.log",
            "docs/adr/README.md",
            "assets/logo.png",
        ]));
        assert!(facts.is_empty());
    }

    #[test]
    fn discovery_is_deterministic_and_order_independent() {
        let forward = discover_project_facts(&paths(&[
            "README.md",
            "Cargo.toml",
            "docs/adr/0001-record.md",
        ]));
        let reversed = discover_project_facts(&paths(&[
            "docs/adr/0001-record.md",
            "Cargo.toml",
            "README.md",
        ]));
        assert_eq!(forward, reversed);
    }

    #[test]
    fn windows_separators_are_normalised_before_classification() {
        let facts = discover_project_facts(&paths(&["docs\\adr\\0026-constitution.md"]));
        assert_eq!(facts.len(), 1);
        assert_eq!(facts[0].source_path, "docs/adr/0026-constitution.md");
        assert_eq!(facts[0].kind, ProjectFactKind::DecisionRecord);
    }

    #[test]
    fn a_repeated_path_is_reported_once() {
        let facts = discover_project_facts(&paths(&["README.md", "README.md"]));
        assert_eq!(facts.len(), 1);
    }
}
