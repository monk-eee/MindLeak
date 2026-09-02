//! Recall discrimination contract shared between MindLeak's two planes
//! (ADR-0140 decisions 3, 5).
//!
//! `mindleak-core::embed`'s semantic recall and Ackplane's pgvector-backed
//! recall both need to answer the same question -- "does this candidate
//! actually stand out, or is it background noise a fresh index cannot yet
//! judge?" -- against the same `NodeType` vocabulary. ADR-0140 decision 3
//! requires this to be "ideally the *same* code, factored into a small shared
//! crate ... not a second, independently-written copy that can silently
//! drift from the constants and reasoning the local version's comments
//! already justify with a dated measurement." This module is that one place.
//!
//! Moved verbatim from `mindleak-core::embed` (no behavior change); that
//! crate's existing tests are the proof.

use crate::NodeType;

/// Candidates a field needs before its *shape* means anything.
///
/// Below this, "stands out from the field" is not a question the data can
/// answer, so the floor alone decides and recall behaves exactly as it did
/// before. A fresh or tiny index must not be silenced by statistics it is too
/// small to support.
pub const DISTINCTIVE_MIN_FIELD: usize = 8;

/// How far above the field, in standard deviations, a candidate must stand to
/// count as an answer rather than as background.
///
/// Cosine similarity is **not comparable across queries**: embedding spaces are
/// anisotropic, so every text carries a baseline resemblance to every other
/// text. Measured 2026-07-27 against this repository's own index, the nonsense
/// query `zzzzqqq wibble flarp` scored **0.54** — above the 0.5 default floor —
/// because the entire field scores about that for any query at all. So an
/// absolute constant cannot tell an answer from the background, and raising it
/// is measurably worse, not better: recorded conclusions scored 0.553–0.790
/// while structural nodes matched on shared vocabulary scored 0.527–0.667, and
/// those ranges overlap. Every threshold high enough to exclude the worst
/// stranger also excludes real conclusions.
///
/// What *does* separate them is distinctiveness. A real hit stands out from its
/// own query's field; nonsense lifts the whole field uniformly and leaves
/// nothing standing above it. That is a per-query question, and this is the
/// margin it must clear.
pub const DISTINCTIVE_SIGMA: f32 = 1.0;

/// The ranking prior the graph already holds for a node of this kind.
///
/// The governing goal is explicit that *"embeddings may only seed graph
/// traversal"*. Ranking purely by cosine is the vector-only memory this engine
/// exists to replace: it throws away everything the graph knows and asks the
/// embedding model to be the whole answer. A recorded conclusion or decision is
/// categorically more likely to answer "what did we learn here" than a symbol
/// name or a shell command that happens to share a word with the question.
///
/// This is deliberately a **tie-breaker, not an override**. The spread is small
/// enough that a genuinely closer symbol still outranks a barely-related
/// intent, so "which function parses imports" keeps working; it only decides
/// the near-ties, which is exactly where the measured overlap lives.
pub fn kind_prior(kind: Option<NodeType>) -> f32 {
    match kind {
        // A conclusion, decision, or commit rationale: what a question is for.
        Some(NodeType::Intent) => 1.00,
        // A compiled digest is itself a distilled answer, the same reason
        // Intent ranks highest -- it exists to be read as a conclusion.
        Some(NodeType::Digest) => 1.00,
        Some(NodeType::Artifact) => 0.92,
        Some(NodeType::Symbol) => 0.85,
        Some(NodeType::Execution) => 0.85,
        // Raw agent tool-call evidence: same tier as Execution -- transient,
        // not a distilled conclusion.
        Some(NodeType::ToolInvocation) => 0.85,
        Some(NodeType::Package) => 0.80,
        // Attribution, not knowledge; it answers no question a caller asks.
        Some(NodeType::Agent) => 0.70,
        // A type tag this build does not recognise: rank it as ordinary
        // structure rather than guessing. A missing node no longer reaches
        // here, since `embeddings` cascades from `nodes`.
        None => 0.85,
    }
}

/// The score a candidate must reach to stand out from `field`.
///
/// Returns [`f32::MIN`] when the field is too small to have a shape, so the
/// floor alone decides. Returns [`f32::MAX`] when the field has no spread at
/// all: if every candidate resembles the query equally then none of them is an
/// answer, which is precisely the shape a nonsense question produces. See
/// [`DISTINCTIVE_SIGMA`] for why a per-query cut succeeds where an absolute
/// constant cannot.
pub fn distinctive_cut(field: &[f32]) -> f32 {
    if field.len() < DISTINCTIVE_MIN_FIELD {
        return f32::MIN;
    }
    let count = field.len() as f32;
    let mean = field.iter().sum::<f32>() / count;
    let variance = field
        .iter()
        .map(|score| (score - mean).powi(2))
        .sum::<f32>()
        / count;
    let spread = variance.sqrt();
    if spread <= f32::EPSILON {
        return f32::MAX;
    }
    mean + DISTINCTIVE_SIGMA * spread
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kind_prior_ranks_intent_and_digest_highest() {
        assert_eq!(kind_prior(Some(NodeType::Intent)), 1.00);
        assert_eq!(kind_prior(Some(NodeType::Digest)), 1.00);
        assert!(kind_prior(Some(NodeType::Agent)) < kind_prior(Some(NodeType::Artifact)));
    }

    #[test]
    fn distinctive_cut_returns_min_below_field_floor() {
        let field: Vec<f32> = vec![0.5; DISTINCTIVE_MIN_FIELD - 1];
        assert_eq!(distinctive_cut(&field), f32::MIN);
    }

    #[test]
    fn distinctive_cut_returns_max_when_field_has_no_spread() {
        let field: Vec<f32> = vec![0.5; DISTINCTIVE_MIN_FIELD];
        assert_eq!(distinctive_cut(&field), f32::MAX);
    }

    #[test]
    fn distinctive_cut_sits_above_the_mean_by_sigma_spread() {
        let field: Vec<f32> = vec![0.1, 0.2, 0.3, 0.4, 0.5, 0.6, 0.7, 0.8];
        let cut = distinctive_cut(&field);
        assert!(cut.is_finite());
        let mean = field.iter().sum::<f32>() / field.len() as f32;
        assert!(cut > mean);
    }
}
