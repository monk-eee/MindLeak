use crate::{
    BudgetReport, CompiledContext, ExcludedCandidate, MindLeak, Result, ScoredNode, WorkingSetItem,
};

/// One candidate considered for a compiled context packet's token budget
/// (ADR-0102 decision 2): a fact from `recall` or an item from `working_set`,
/// reduced to exactly what ranking and budgeting need. `rank` reuses each
/// source's own already-decayed relevance score rather than inventing a third
/// heuristic (`ScoredNode::score` / `WorkingSetItem::attention`). `index`
/// locates the original item in its own source vector, so a duplicate id
/// appearing in both `facts` and `working_set` is still tracked as two
/// independent inclusion decisions rather than one shared by id alone.
struct Candidate {
    id: String,
    rank: f64,
    estimated_tokens: usize,
    source: Source,
    index: usize,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Source {
    Fact,
    Working,
}

/// The same bytes/4 approximation the advertised MCP tool surface already
/// uses (`scripts/measure-tool-surface.mjs`), applied to one candidate's own
/// serialized JSON rather than the whole packet.
fn estimate_tokens<T: serde::Serialize>(value: &T) -> usize {
    serde_json::to_vec(value)
        .map(|bytes| bytes.len() / 4)
        .unwrap_or(0)
}

/// Sort candidates by rank, strongest first. Ties break on id so the ordering
/// is deterministic across calls, not incidental to a HashMap or float-equal
/// pair landing in whatever order the source query happened to return them.
fn rank_for_context(mut candidates: Vec<Candidate>) -> Vec<Candidate> {
    candidates.sort_by(|a, b| {
        b.rank
            .partial_cmp(&a.rank)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.id.cmp(&b.id))
    });
    candidates
}

impl MindLeak {
    /// Compile one bounded, ranked, token-budgeted context packet from
    /// existing retrieval primitives (ADR-0102) — no new source of truth,
    /// only composition, ranking, and a token budget over sources that
    /// already exist. `governing` is supplied by the caller (Lodestar's
    /// `advise()`, a separate plane this crate has no dependency on) and
    /// passes through unfiltered; `evidence` is populated only when a task
    /// evidence window is supplied.
    #[allow(clippy::too_many_arguments)]
    pub fn compile_context(
        &self,
        agent: &str,
        query: Option<&str>,
        recall_limit: usize,
        working_set_limit: Option<usize>,
        max_tokens: usize,
        evidence_window: Option<(Option<&str>, i64, i64)>,
        governing: serde_json::Value,
    ) -> Result<CompiledContext> {
        let facts = match query {
            Some(q) if !q.trim().is_empty() => self.recall(q, recall_limit)?,
            _ => Vec::new(),
        };
        let working_set = self.working_set(agent, working_set_limit)?;
        let evidence = match evidence_window {
            Some((task_id, started_at, ended_at)) => {
                Some(self.evidence_for(task_id, agent, started_at, ended_at)?)
            }
            None => None,
        };

        let candidates: Vec<Candidate> = facts
            .iter()
            .enumerate()
            .map(|(index, f)| Candidate {
                id: f.node.id.clone(),
                rank: f.score,
                estimated_tokens: estimate_tokens(f),
                source: Source::Fact,
                index,
            })
            .chain(working_set.iter().enumerate().map(|(index, w)| Candidate {
                id: w.node.id.clone(),
                rank: w.attention,
                estimated_tokens: estimate_tokens(w),
                source: Source::Working,
                index,
            }))
            .collect();
        let ranked = rank_for_context(candidates);

        let mut kept_fact_indices = Vec::new();
        let mut kept_working_indices = Vec::new();
        let mut excluded = Vec::new();
        let mut tokens_used = 0usize;
        for candidate in &ranked {
            let would_use = tokens_used + candidate.estimated_tokens;
            if would_use <= max_tokens {
                tokens_used = would_use;
                match candidate.source {
                    Source::Fact => kept_fact_indices.push(candidate.index),
                    Source::Working => kept_working_indices.push(candidate.index),
                }
            } else {
                excluded.push(ExcludedCandidate {
                    id: candidate.id.clone(),
                    rank: candidate.rank,
                });
            }
        }

        let facts: Vec<ScoredNode> = kept_fact_indices
            .into_iter()
            .map(|i| facts[i].clone())
            .collect();
        let working_set: Vec<WorkingSetItem> = kept_working_indices
            .into_iter()
            .map(|i| working_set[i].clone())
            .collect();

        if let Ok(bytes) = serde_json::to_vec(&governing) {
            tokens_used += bytes.len() / 4;
        }
        if let Some(ev) = &evidence {
            tokens_used += estimate_tokens(ev);
        }

        Ok(CompiledContext {
            facts,
            working_set,
            governing,
            evidence,
            budget_report: BudgetReport {
                tokens_requested: max_tokens,
                tokens_used,
                excluded,
            },
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn engine_with_observed_file() -> MindLeak {
        let engine = MindLeak::open_in_memory().unwrap();
        engine
            .ingest_file_for_agent("agent-tester", "a.rs", "fn a() {}\n")
            .unwrap();
        engine
    }

    #[test]
    fn compiles_a_packet_from_working_set_alone_when_no_query_is_given() {
        let engine = engine_with_observed_file();

        let packet = engine
            .compile_context(
                "agent-tester",
                None,
                5,
                None,
                10_000,
                None,
                serde_json::json!([]),
            )
            .unwrap();

        assert!(packet.facts.is_empty());
        assert_eq!(packet.working_set.len(), 1);
        assert!(packet.working_set[0].node.id.starts_with("artifact:"));
        assert!(packet.budget_report.excluded.is_empty());
        assert_eq!(packet.budget_report.tokens_requested, 10_000);
    }

    #[test]
    fn a_zero_token_budget_excludes_every_working_set_candidate_explicitly() {
        let engine = engine_with_observed_file();

        let packet = engine
            .compile_context(
                "agent-tester",
                None,
                5,
                None,
                0,
                None,
                serde_json::json!({}),
            )
            .unwrap();

        assert!(packet.working_set.is_empty());
        assert_eq!(packet.budget_report.excluded.len(), 1);
        assert!(packet.budget_report.excluded[0].id.starts_with("artifact:"));
    }

    #[test]
    fn governing_passes_through_unfiltered_and_is_never_a_candidate() {
        let engine = engine_with_observed_file();
        let governing = serde_json::json!({"disposition": "advise", "findings": ["proceed"]});

        let packet = engine
            .compile_context("agent-tester", None, 5, None, 0, None, governing.clone())
            .unwrap();

        assert_eq!(packet.governing, governing);
    }

    #[test]
    fn rank_for_context_orders_by_rank_descending_with_a_deterministic_tiebreak() {
        let candidates = vec![
            Candidate {
                id: "b".to_string(),
                rank: 0.5,
                estimated_tokens: 1,
                source: Source::Fact,
                index: 0,
            },
            Candidate {
                id: "a".to_string(),
                rank: 0.9,
                estimated_tokens: 1,
                source: Source::Fact,
                index: 1,
            },
            Candidate {
                id: "c".to_string(),
                rank: 0.5,
                estimated_tokens: 1,
                source: Source::Working,
                index: 0,
            },
        ];

        let ranked = rank_for_context(candidates);

        assert_eq!(
            ranked.iter().map(|c| c.id.as_str()).collect::<Vec<_>>(),
            vec!["a", "b", "c"]
        );
    }
}
