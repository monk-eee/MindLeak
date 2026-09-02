//! Stage two of ADR-0140 decision 3: deciding which of stage one's candidates
//! is actually an answer.
//!
//! [`super::embeddings::Projector::similar_nodes`] asks PostgreSQL for a
//! bounded candidate set ordered by `<=>` cosine distance. That is retrieval,
//! and retrieval always returns *something* — its nearest rows exist whether or
//! not the question has an answer here. This module is the part that can say
//! nothing.
//!
//! It applies the same three mechanisms `mindleak_core::embed::recall` applies
//! locally, through the same shared functions rather than a second copy
//! (ADR-0140 decision 3, decision 5): the `kind_prior` the graph already holds
//! for a node's kind, the per-query `distinctive_cut`, and the caller's floor.
//! The reported score stays the raw cosine similarity (decision 4) so a caller
//! sees what was measured, not an internal composite.

use mindleak_model::discrimination::{distinctive_cut, kind_prior};
use mindleak_model::NodeType;

use super::embeddings::SimilarNode;

/// One node stage two is willing to report, with the similarity that was
/// actually measured for it.
#[derive(Debug, Clone, PartialEq)]
pub struct RankedNode {
    pub node_id: String,
    pub label: String,
    pub node_type: String,
    /// Raw cosine similarity, `1.0 - cosine_distance` (ADR-0140 decision 4).
    /// `1.0` is identical. Never the kind-weighted score used for ordering:
    /// that is an internal ranking device, and reporting it would tell a
    /// caller a node resembles their query more or less than it does.
    pub similarity: f32,
}

/// Rank `candidates` and return only those worth reporting, best first.
///
/// Returns an **empty** vector when nothing stands out, which is the whole
/// point (ADR-0053): an unanswerable query must get nothing rather than the
/// least-bad row PostgreSQL happened to order first. A caller handed a
/// plausible stranger cannot tell it is wrong, and stops asking.
///
/// A pure function over the candidate set, deliberately: the decision that
/// decides whether recall answers at all is testable without a database, so
/// the tests that prove it can be run anywhere and cannot be quietly skipped
/// for want of one.
pub fn rank(candidates: Vec<SimilarNode>, floor: f32, limit: usize) -> Vec<RankedNode> {
    let scored: Vec<(SimilarNode, f32, f32)> = candidates
        .into_iter()
        .map(|candidate| {
            // pgvector reports distance; the discrimination contract, the
            // floor, and the caller all speak similarity.
            let similarity = 1.0 - candidate.cosine_distance as f32;
            // An unrecognised type tag ranks as ordinary structure rather than
            // guessing, exactly as it does locally.
            let kind = NodeType::from_tag(&candidate.node_type);
            let weighted = similarity * kind_prior(kind);
            (candidate, similarity, weighted)
        })
        .collect();

    let field: Vec<f32> = scored
        .iter()
        .map(|(_, similarity, _)| *similarity)
        .collect();
    let cut = distinctive_cut(&field);

    let mut kept: Vec<(SimilarNode, f32, f32)> = scored
        .into_iter()
        .filter(|(_, similarity, _)| *similarity >= floor && *similarity >= cut)
        .collect();
    // Ordered by the kind-weighted score, reported as the raw one.
    kept.sort_by(|a, b| b.2.partial_cmp(&a.2).unwrap_or(std::cmp::Ordering::Equal));
    kept.truncate(limit);
    kept.into_iter()
        .map(|(candidate, similarity, _)| RankedNode {
            node_id: candidate.node_id,
            label: candidate.label,
            node_type: candidate.node_type,
            similarity,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use mindleak_model::discrimination::DISTINCTIVE_MIN_FIELD;

    fn candidate(node_id: &str, node_type: &str, similarity: f32) -> SimilarNode {
        SimilarNode {
            node_id: node_id.to_owned(),
            label: format!("label for {node_id}"),
            node_type: node_type.to_owned(),
            cosine_distance: (1.0 - similarity) as f64,
        }
    }

    /// A field of candidates that all resemble the query equally is exactly the
    /// shape a nonsense question produces: everything lifts together and
    /// nothing stands above it. This is ADR-0053's guarantee and the reason
    /// this module exists, so it is the test to break first.
    #[test]
    fn an_unanswerable_query_reports_nothing_rather_than_the_least_bad_row() {
        let candidates: Vec<SimilarNode> = (0..DISTINCTIVE_MIN_FIELD)
            .map(|n| candidate(&format!("artifact:{n}"), "artifact", 0.54))
            .collect();

        let ranked = rank(candidates, 0.5, 10);

        assert!(
            ranked.is_empty(),
            "a flat field clears the floor but answers nothing; got {ranked:?}"
        );
    }

    #[test]
    fn the_candidate_that_stands_out_from_a_flat_field_is_reported() {
        let mut candidates: Vec<SimilarNode> = (0..DISTINCTIVE_MIN_FIELD)
            .map(|n| candidate(&format!("artifact:{n}"), "artifact", 0.54))
            .collect();
        candidates.push(candidate("intent:real-answer", "intent", 0.95));

        let ranked = rank(candidates, 0.5, 10);

        assert_eq!(ranked.len(), 1);
        assert_eq!(ranked[0].node_id, "intent:real-answer");
    }

    /// Decision 4: the caller sees the similarity that was measured, not the
    /// kind-weighted number used to order the list.
    #[test]
    fn the_reported_score_is_the_raw_similarity_not_the_weighted_one() {
        let mut candidates: Vec<SimilarNode> = (0..DISTINCTIVE_MIN_FIELD)
            .map(|n| candidate(&format!("artifact:{n}"), "artifact", 0.10))
            .collect();
        candidates.push(candidate("agent:someone", "agent", 0.80));

        let ranked = rank(candidates, 0.0, 10);

        let reported = ranked
            .iter()
            .find(|node| node.node_id == "agent:someone")
            .expect("the standout candidate is reported");
        // Agent's prior is 0.70, so a weighted score would read 0.56.
        assert!(
            (reported.similarity - 0.80).abs() < 1e-6,
            "expected the measured 0.80, got {}",
            reported.similarity
        );
    }

    /// The prior is a tie-breaker, not an override: it decides near-ties, which
    /// is where the measured overlap between real answers and shared-vocabulary
    /// matches actually lives.
    #[test]
    fn the_kind_prior_breaks_a_near_tie_in_favour_of_recorded_intent() {
        let mut candidates: Vec<SimilarNode> = (0..DISTINCTIVE_MIN_FIELD)
            .map(|n| candidate(&format!("artifact:{n}"), "artifact", 0.10))
            .collect();
        candidates.push(candidate("symbol:parse_imports", "symbol", 0.81));
        candidates.push(candidate("intent:why-we-parse", "intent", 0.80));

        let ranked = rank(candidates, 0.0, 10);

        assert_eq!(
            ranked[0].node_id, "intent:why-we-parse",
            "0.80 * 1.00 must outrank 0.81 * 0.85"
        );
    }

    /// A genuinely closer symbol still wins: the spread is small enough that
    /// "which function parses imports" keeps working.
    #[test]
    fn a_clearly_closer_symbol_still_outranks_a_barely_related_intent() {
        let mut candidates: Vec<SimilarNode> = (0..DISTINCTIVE_MIN_FIELD)
            .map(|n| candidate(&format!("artifact:{n}"), "artifact", 0.10))
            .collect();
        candidates.push(candidate("symbol:parse_imports", "symbol", 0.95));
        candidates.push(candidate("intent:unrelated", "intent", 0.60));

        let ranked = rank(candidates, 0.0, 10);

        assert_eq!(ranked[0].node_id, "symbol:parse_imports");
    }

    /// A small or fresh index has no distribution to reason about, so the floor
    /// alone decides. Statistics must not silence an index that is merely young.
    #[test]
    fn a_field_too_small_to_have_a_shape_is_judged_by_the_floor_alone() {
        let candidates = vec![
            candidate("artifact:a", "artifact", 0.90),
            candidate("artifact:b", "artifact", 0.20),
        ];

        let ranked = rank(candidates, 0.5, 10);

        assert_eq!(ranked.len(), 1);
        assert_eq!(ranked[0].node_id, "artifact:a");
    }

    #[test]
    fn nothing_clearing_the_floor_reports_nothing() {
        let candidates = vec![
            candidate("artifact:a", "artifact", 0.30),
            candidate("artifact:b", "artifact", 0.20),
        ];

        assert!(rank(candidates, 0.5, 10).is_empty());
    }

    #[test]
    fn an_unrecognised_type_tag_ranks_as_ordinary_structure_rather_than_failing() {
        let mut candidates: Vec<SimilarNode> = (0..DISTINCTIVE_MIN_FIELD)
            .map(|n| candidate(&format!("artifact:{n}"), "artifact", 0.10))
            .collect();
        candidates.push(candidate("mystery:1", "not_a_known_tag", 0.90));

        let ranked = rank(candidates, 0.0, 10);

        assert_eq!(ranked[0].node_id, "mystery:1");
    }

    #[test]
    fn the_limit_bounds_what_is_reported() {
        let mut candidates: Vec<SimilarNode> = (0..DISTINCTIVE_MIN_FIELD)
            .map(|n| candidate(&format!("artifact:{n}"), "artifact", 0.10))
            .collect();
        for n in 0..5 {
            candidates.push(candidate(&format!("intent:{n}"), "intent", 0.90));
        }

        let ranked = rank(candidates, 0.0, 2);

        assert_eq!(ranked.len(), 2);
    }

    #[test]
    fn no_candidates_reports_nothing_without_panicking() {
        assert!(rank(Vec::new(), 0.5, 10).is_empty());
    }
}
