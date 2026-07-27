//! Proposing the question one agent should put to another (ADR-0055).
//!
//! ADR-0046 gave agents a way to address a question at a peer, and then nothing
//! ever used it: in an eight-hour session `pending_questions` stayed empty while
//! five tasks sat waiting on a human. The verb existed; nothing in the loop
//! surfaced that there *was* a question to ask.
//!
//! This module closes that gap without deciding anything. It turns a scope
//! collision — a fact the ledger already holds — into a concrete, addressed
//! draft the owning agent can send, edit, or discard. It records nothing, parks
//! nothing, and addresses nothing by itself: `ask_question` remains the only
//! thing that changes task state, and a human or agent remains the only thing
//! that decides.

use serde::{Deserialize, Serialize};

use crate::model::ClaimOverlap;

/// Who wrote the question text.
///
/// Recorded because a drafted sentence is not evidence and must never read like
/// it. A reader can always tell whether a local model phrased this or whether it
/// came from the deterministic template.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DraftedBy {
    /// Built from the overlap alone. Always available, model or no model.
    Template,
    /// Phrased by the optional local model from the same overlap.
    Model,
}

impl DraftedBy {
    pub fn as_str(self) -> &'static str {
        match self {
            DraftedBy::Template => "template",
            DraftedBy::Model => "model",
        }
    }
}

/// A question a task's owner could put to a peer whose live claim collides.
///
/// A proposal, never a message. Nothing is sent until the caller decides to
/// call `ask_question`, which is what keeps the durable thread the only record
/// of what was actually asked.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QuestionDraft {
    /// The peer to address it at — their agent id, ready for `ask_question`.
    pub audience: String,
    /// The peer's task whose scope collides.
    pub their_task_id: String,
    pub their_title: String,
    /// Concrete paths and symbols claimed by both.
    pub matching_paths: Vec<String>,
    pub matching_symbols: Vec<String>,
    pub question: String,
    pub drafted_by: DraftedBy,
}

/// The question implied by a scope collision, with no model involved.
///
/// Deliberately narrow and concrete: it names what is shared and asks about
/// ordering, because that is the one thing the ledger cannot answer for itself.
/// Who holds what is readable; what a peer intends to do next is not, and a
/// question about a readable fact is a worse version of looking it up.
pub fn template_question(my_title: &str, overlap: &ClaimOverlap) -> String {
    let shared = shared_scope_phrase(overlap);
    format!(
        "I am working on \"{my_title}\" and we both hold a live claim on {shared}. \
         Are you changing it, or shall I? If you are, I will wait for yours to land first."
    )
}

/// The shared scope as one readable clause, capped so a wide claim cannot
/// produce a question nobody will read.
fn shared_scope_phrase(overlap: &ClaimOverlap) -> String {
    const MAX_NAMED: usize = 3;
    let mut named: Vec<&str> = overlap
        .matching_paths
        .iter()
        .chain(overlap.matching_symbols.iter())
        .map(String::as_str)
        .collect();
    if named.is_empty() {
        return "overlapping scope".to_string();
    }
    let extra = named.len().saturating_sub(MAX_NAMED);
    named.truncate(MAX_NAMED);
    let listed = named.join(", ");
    if extra == 0 {
        listed
    } else {
        format!("{listed} (and {extra} more)")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::TaskScope;

    fn overlap(paths: &[&str], symbols: &[&str]) -> ClaimOverlap {
        ClaimOverlap {
            task_id: "task:theirs".to_string(),
            owner: "session:v1:abc".to_string(),
            lease_expires_at: 100,
            scope: TaskScope::default(),
            matching_paths: paths.iter().map(|p| p.to_string()).collect(),
            matching_symbols: symbols.iter().map(|s| s.to_string()).collect(),
        }
    }

    #[test]
    fn the_question_names_the_shared_scope_and_asks_about_ordering() {
        let question =
            template_question("Split the ingest module", &overlap(&["src/ingest.rs"], &[]));
        assert!(question.contains("Split the ingest module"));
        assert!(question.contains("src/ingest.rs"));
        // Ordering is the point: who holds what is already readable.
        assert!(question.contains("shall I"));
        assert!(question.contains("wait"));
    }

    #[test]
    fn a_wide_overlap_is_summarised_rather_than_listed_in_full() {
        let question = template_question(
            "Rename the facade",
            &overlap(&["a.rs", "b.rs", "c.rs", "d.rs"], &["symbol:a.rs:run"]),
        );
        assert!(question.contains("a.rs, b.rs, c.rs"));
        assert!(
            question.contains("(and 2 more)"),
            "the tail is counted, not listed: {question}"
        );
        assert!(
            !question.contains("d.rs"),
            "the tail is not named: {question}"
        );
    }

    /// A collision can be recorded with no concrete path or symbol in common —
    /// two claims on the same task tree, say. The draft must still read as a
    /// sentence rather than trailing off into an empty list.
    #[test]
    fn an_overlap_with_nothing_named_still_reads_as_a_question() {
        let question = template_question("Tidy the board", &overlap(&[], &[]));
        assert!(question.contains("overlapping scope"));
        assert!(question.ends_with("land first."));
    }

    #[test]
    fn provenance_is_reported_so_a_drafted_sentence_is_never_read_as_evidence() {
        assert_eq!(DraftedBy::Template.as_str(), "template");
        assert_eq!(DraftedBy::Model.as_str(), "model");
    }
}
