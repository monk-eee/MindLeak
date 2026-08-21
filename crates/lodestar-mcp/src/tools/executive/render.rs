//! Markdown rendering helpers for human-facing executive tool responses.

use lodestar_core::{GoverningClause, HumanQuestion};

/// The human inbox as readable markdown.
///
/// Rendered rather than dumped as JSON because the reader is a person deciding
/// what to answer, and a wall of objects is not a decision aid. The structured
/// form still travels in `structuredContent` for programmatic callers.
pub(super) fn render_human_questions(questions: &[HumanQuestion]) -> String {
    if questions.is_empty() {
        return "## Waiting on you\n\nNothing. No agent is parked on a question from you."
            .to_string();
    }
    let mut out = format!("## Waiting on you ({})\n", questions.len());
    for question in questions {
        out.push_str(&format!(
            "\n### {}\n\n{}\n\n- task: `{}`\n- asked by: `{}`\n- waiting: {}\n- reply: `answer` with this task_id\n",
            question.task_title,
            question.question,
            question.task_id,
            question.asked_by,
            humanize_seconds(question.waiting_seconds),
        ));
    }
    out
}

/// A duration a person can read at a glance. Coarse on purpose: the point is
/// "this has been sitting for three days", not a stopwatch.
pub(super) fn humanize_seconds(seconds: i64) -> String {
    match seconds {
        s if s < 60 => format!("{s}s"),
        s if s < 3_600 => format!("{}m", s / 60),
        s if s < 86_400 => format!("{}h {}m", s / 3_600, (s % 3_600) / 60),
        s => format!("{}d {}h", s / 86_400, (s % 86_400) / 3_600),
    }
}

/// Render the clauses governing a task's scope as a bounded Markdown section for
/// pickup responses (`task_query` view=next / `task_claim` step=claim). Empty
/// when nothing governs.
pub(super) fn render_governing(governing: &[GoverningClause]) -> String {
    if governing.is_empty() {
        return String::new();
    }
    let mut section = String::from("\n\n**Governed by:**");
    for clause in governing {
        section.push_str(&format!(
            "\n- `{}` ({}, {}) — {}",
            clause.goal.id,
            clause.goal.kind.as_str(),
            clause.mode.as_str(),
            clause.goal.title
        ));
    }
    section
}
