//! Validation helpers for reviewed design materialization plans.

use std::collections::HashSet;

use rusqlite::{params, Connection, OptionalExtension};

use crate::design::{DesignMaterializationMode, DesignMaterializationPlan};
use crate::error::{LodestarError, Result};

pub(super) fn validate_materialization_plan(plan: &DesignMaterializationPlan) -> Result<()> {
    let has_rationale = plan
        .rationale
        .as_deref()
        .is_some_and(|value| !value.trim().is_empty());
    match plan.mode {
        DesignMaterializationMode::Create if plan.tasks.is_empty() => {
            return Err(LodestarError::Invalid(
                "a create materialization needs at least one reviewed task".to_string(),
            ));
        }
        DesignMaterializationMode::Create if !plan.task_ids.is_empty() => {
            return Err(LodestarError::Invalid(
                "a create materialization cannot link task ids".to_string(),
            ));
        }
        DesignMaterializationMode::Link if plan.task_ids.is_empty() => {
            return Err(LodestarError::Invalid(
                "a link materialization needs at least one existing task id".to_string(),
            ));
        }
        DesignMaterializationMode::Link if !plan.tasks.is_empty() => {
            return Err(LodestarError::Invalid(
                "a link materialization cannot create task drafts".to_string(),
            ));
        }
        DesignMaterializationMode::Link if !has_rationale => {
            return Err(LodestarError::Invalid(
                "a link materialization needs a rationale".to_string(),
            ));
        }
        DesignMaterializationMode::NoWork
            if !plan.tasks.is_empty() || !plan.task_ids.is_empty() =>
        {
            return Err(LodestarError::Invalid(
                "a no-work materialization cannot contain tasks".to_string(),
            ));
        }
        DesignMaterializationMode::NoWork if !has_rationale => {
            return Err(LodestarError::Invalid(
                "a no-work materialization needs a rationale".to_string(),
            ));
        }
        _ => {}
    }

    let mut task_keys = HashSet::new();
    for draft in &plan.tasks {
        if draft.goal_id.trim().is_empty()
            || draft.title.trim().is_empty()
            || draft.acceptance.trim().is_empty()
        {
            return Err(LodestarError::Invalid(
                "created tasks require a goal id, title, and acceptance criteria".to_string(),
            ));
        }
        if !task_keys.insert((&draft.goal_id, &draft.title)) {
            return Err(LodestarError::Invalid(format!(
                "duplicate task draft for {}: {}",
                draft.goal_id, draft.title
            )));
        }
    }
    let mut linked_ids = HashSet::new();
    for task_id in &plan.task_ids {
        if task_id.trim().is_empty() || !linked_ids.insert(task_id) {
            return Err(LodestarError::Invalid(
                "linked task ids must be non-empty and unique".to_string(),
            ));
        }
    }
    for constraint in &plan.constraints {
        if !constraint.kind.is_normative()
            || constraint.title.trim().is_empty()
            || constraint.statement.trim().is_empty()
        {
            return Err(LodestarError::Invalid(
                "materialized constraints require a normative kind, title, and statement"
                    .to_string(),
            ));
        }
    }
    Ok(())
}

pub(super) fn ensure_objective(connection: &Connection, goal_id: &str) -> Result<()> {
    let kind = connection
        .query_row(
            "SELECT kind FROM goals WHERE id = ?1",
            params![goal_id],
            |row| row.get::<_, String>(0),
        )
        .optional()?
        .ok_or_else(|| LodestarError::NotFound(goal_id.to_string()))?;
    if kind != "objective" {
        return Err(LodestarError::Invalid(format!(
            "task goal {goal_id} is a {kind}; materialized tasks must serve objectives"
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::design::{DesignConstraintDraft, DesignTaskDraft};
    use crate::model::GoalKind;
    use crate::store::test_support::{goal, store, NOW};

    fn plan(mode: DesignMaterializationMode) -> DesignMaterializationPlan {
        DesignMaterializationPlan {
            mode,
            tasks: Vec::new(),
            task_ids: Vec::new(),
            constraints: Vec::new(),
            rationale: None,
        }
    }

    fn task_draft(goal_id: &str, title: &str, acceptance: &str) -> DesignTaskDraft {
        DesignTaskDraft {
            goal_id: goal_id.to_string(),
            title: title.to_string(),
            acceptance: acceptance.to_string(),
        }
    }

    fn constraint_draft(kind: GoalKind, title: &str, statement: &str) -> DesignConstraintDraft {
        DesignConstraintDraft {
            kind,
            title: title.to_string(),
            statement: statement.to_string(),
        }
    }

    #[test]
    fn create_mode_requires_at_least_one_task() {
        let p = plan(DesignMaterializationMode::Create);
        let err = validate_materialization_plan(&p).unwrap_err();
        assert_eq!(
            err.to_string(),
            "invalid: a create materialization needs at least one reviewed task"
        );
    }

    #[test]
    fn create_mode_rejects_linked_task_ids() {
        let mut p = plan(DesignMaterializationMode::Create);
        p.tasks.push(task_draft("goal:a", "Do it", "done when x"));
        p.task_ids.push("task:already-exists".to_string());
        let err = validate_materialization_plan(&p).unwrap_err();
        assert_eq!(
            err.to_string(),
            "invalid: a create materialization cannot link task ids"
        );
    }

    #[test]
    fn link_mode_requires_at_least_one_task_id() {
        let p = plan(DesignMaterializationMode::Link);
        let err = validate_materialization_plan(&p).unwrap_err();
        assert_eq!(
            err.to_string(),
            "invalid: a link materialization needs at least one existing task id"
        );
    }

    #[test]
    fn link_mode_rejects_task_drafts() {
        let mut p = plan(DesignMaterializationMode::Link);
        p.task_ids.push("task:already-exists".to_string());
        p.tasks.push(task_draft("goal:a", "Do it", "done when x"));
        p.rationale = Some("linking existing work".to_string());
        let err = validate_materialization_plan(&p).unwrap_err();
        assert_eq!(
            err.to_string(),
            "invalid: a link materialization cannot create task drafts"
        );
    }

    #[test]
    fn link_mode_requires_a_rationale() {
        let mut p = plan(DesignMaterializationMode::Link);
        p.task_ids.push("task:already-exists".to_string());
        let err = validate_materialization_plan(&p).unwrap_err();
        assert_eq!(
            err.to_string(),
            "invalid: a link materialization needs a rationale"
        );
    }

    #[test]
    fn link_mode_succeeds_with_a_rationale_and_task_ids() {
        let mut p = plan(DesignMaterializationMode::Link);
        p.task_ids.push("task:already-exists".to_string());
        p.rationale = Some("linking existing work".to_string());
        assert!(validate_materialization_plan(&p).is_ok());
    }

    #[test]
    fn no_work_mode_rejects_task_drafts() {
        let mut p = plan(DesignMaterializationMode::NoWork);
        p.tasks.push(task_draft("goal:a", "Do it", "done when x"));
        p.rationale = Some("nothing to build".to_string());
        let err = validate_materialization_plan(&p).unwrap_err();
        assert_eq!(
            err.to_string(),
            "invalid: a no-work materialization cannot contain tasks"
        );
    }

    #[test]
    fn no_work_mode_rejects_task_ids() {
        let mut p = plan(DesignMaterializationMode::NoWork);
        p.task_ids.push("task:already-exists".to_string());
        p.rationale = Some("nothing to build".to_string());
        let err = validate_materialization_plan(&p).unwrap_err();
        assert_eq!(
            err.to_string(),
            "invalid: a no-work materialization cannot contain tasks"
        );
    }

    #[test]
    fn no_work_mode_requires_a_rationale() {
        let p = plan(DesignMaterializationMode::NoWork);
        let err = validate_materialization_plan(&p).unwrap_err();
        assert_eq!(
            err.to_string(),
            "invalid: a no-work materialization needs a rationale"
        );
    }

    #[test]
    fn no_work_mode_succeeds_with_a_rationale() {
        let mut p = plan(DesignMaterializationMode::NoWork);
        p.rationale = Some("nothing to build".to_string());
        assert!(validate_materialization_plan(&p).is_ok());
    }

    #[test]
    fn create_mode_rejects_a_task_draft_missing_a_goal_id() {
        let mut p = plan(DesignMaterializationMode::Create);
        p.tasks.push(task_draft("  ", "Do it", "done when x"));
        let err = validate_materialization_plan(&p).unwrap_err();
        assert_eq!(
            err.to_string(),
            "invalid: created tasks require a goal id, title, and acceptance criteria"
        );
    }

    #[test]
    fn create_mode_rejects_a_task_draft_missing_a_title() {
        let mut p = plan(DesignMaterializationMode::Create);
        p.tasks.push(task_draft("goal:a", "  ", "done when x"));
        let err = validate_materialization_plan(&p).unwrap_err();
        assert_eq!(
            err.to_string(),
            "invalid: created tasks require a goal id, title, and acceptance criteria"
        );
    }

    #[test]
    fn create_mode_rejects_a_task_draft_missing_acceptance() {
        let mut p = plan(DesignMaterializationMode::Create);
        p.tasks.push(task_draft("goal:a", "Do it", "  "));
        let err = validate_materialization_plan(&p).unwrap_err();
        assert_eq!(
            err.to_string(),
            "invalid: created tasks require a goal id, title, and acceptance criteria"
        );
    }

    #[test]
    fn create_mode_rejects_duplicate_task_drafts() {
        let mut p = plan(DesignMaterializationMode::Create);
        p.tasks.push(task_draft("goal:a", "Do it", "done when x"));
        p.tasks
            .push(task_draft("goal:a", "Do it", "a different acceptance"));
        let err = validate_materialization_plan(&p).unwrap_err();
        assert_eq!(
            err.to_string(),
            "invalid: duplicate task draft for goal:a: Do it"
        );
    }

    #[test]
    fn link_mode_rejects_an_empty_task_id() {
        let mut p = plan(DesignMaterializationMode::Link);
        p.task_ids.push("  ".to_string());
        p.rationale = Some("linking existing work".to_string());
        let err = validate_materialization_plan(&p).unwrap_err();
        assert_eq!(
            err.to_string(),
            "invalid: linked task ids must be non-empty and unique"
        );
    }

    #[test]
    fn link_mode_rejects_duplicate_task_ids() {
        let mut p = plan(DesignMaterializationMode::Link);
        p.task_ids.push("task:already-exists".to_string());
        p.task_ids.push("task:already-exists".to_string());
        p.rationale = Some("linking existing work".to_string());
        let err = validate_materialization_plan(&p).unwrap_err();
        assert_eq!(
            err.to_string(),
            "invalid: linked task ids must be non-empty and unique"
        );
    }

    #[test]
    fn create_mode_rejects_a_non_normative_constraint() {
        let mut p = plan(DesignMaterializationMode::Create);
        p.tasks.push(task_draft("goal:a", "Do it", "done when x"));
        p.constraints.push(constraint_draft(
            GoalKind::Objective,
            "Not normative",
            "objectives cannot be constraints",
        ));
        let err = validate_materialization_plan(&p).unwrap_err();
        assert_eq!(
            err.to_string(),
            "invalid: materialized constraints require a normative kind, title, and statement"
        );
    }

    #[test]
    fn create_mode_rejects_a_constraint_missing_a_title() {
        let mut p = plan(DesignMaterializationMode::Create);
        p.tasks.push(task_draft("goal:a", "Do it", "done when x"));
        p.constraints
            .push(constraint_draft(GoalKind::Constraint, "  ", "must hold"));
        let err = validate_materialization_plan(&p).unwrap_err();
        assert_eq!(
            err.to_string(),
            "invalid: materialized constraints require a normative kind, title, and statement"
        );
    }

    #[test]
    fn create_mode_rejects_a_constraint_missing_a_statement() {
        let mut p = plan(DesignMaterializationMode::Create);
        p.tasks.push(task_draft("goal:a", "Do it", "done when x"));
        p.constraints
            .push(constraint_draft(GoalKind::Invariant, "Title", "  "));
        let err = validate_materialization_plan(&p).unwrap_err();
        assert_eq!(
            err.to_string(),
            "invalid: materialized constraints require a normative kind, title, and statement"
        );
    }

    #[test]
    fn create_mode_succeeds_with_valid_tasks_and_a_normative_constraint() {
        let mut p = plan(DesignMaterializationMode::Create);
        p.tasks.push(task_draft("goal:a", "Do it", "done when x"));
        p.constraints.push(constraint_draft(
            GoalKind::Invariant,
            "Never regress",
            "the fix must never be reverted",
        ));
        assert!(validate_materialization_plan(&p).is_ok());
    }

    #[test]
    fn ensure_objective_accepts_a_real_objective_goal() {
        let s = store();
        let g = goal(&s);
        assert!(ensure_objective(&s.conn, &g.id).is_ok());
    }

    #[test]
    fn ensure_objective_rejects_a_nonexistent_goal() {
        let s = store();
        let err = ensure_objective(&s.conn, "goal:does-not-exist").unwrap_err();
        assert_eq!(err.to_string(), "not found: goal:does-not-exist");
    }

    #[test]
    fn ensure_objective_rejects_a_non_objective_goal() {
        let s = store();
        let g = s
            .define_goal(
                GoalKind::Constraint,
                "Never regress",
                "the fix must never be reverted",
                None,
                NOW,
            )
            .unwrap();
        let err = ensure_objective(&s.conn, &g.id).unwrap_err();
        assert_eq!(
            err.to_string(),
            format!(
                "invalid: task goal {} is a constraint; materialized tasks must serve objectives",
                g.id
            )
        );
    }
}
