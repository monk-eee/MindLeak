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
