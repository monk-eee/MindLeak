//! Promotion into work, and the immutable materialization revisions it writes.
//!
//! Split out of `design.rs` (see `super`); the code is unchanged.

use super::*;

impl LodestarStore {
    /// Resolve the durable objective/task/constraint provenance for a
    /// materialized design. Proposed and pending designs have no materialized
    /// plan yet and return `None`; this read never invokes planning.
    pub fn design_promotion(&self, id: &str) -> Result<Option<DesignPromotion>> {
        let item = self
            .get_design_item(id)?
            .ok_or_else(|| LodestarError::NotFound(id.to_string()))?;
        if item.promotion_status != DesignPromotionStatus::Materialized {
            return Ok(None);
        }
        self.resolve_promotion(&item).map(Some)
    }

    /// Append-only reviewed materialization decisions, oldest first.
    pub fn design_materialization_history(
        &self,
        id: &str,
    ) -> Result<Vec<DesignMaterializationRecord>> {
        if self.get_design_item(id)?.is_none() {
            return Err(LodestarError::NotFound(id.to_string()));
        }
        let mut statement = self.conn.prepare(
            "SELECT design_id, revision, plan_json, actor, created_at
             FROM design_materializations WHERE design_id = ?1 ORDER BY revision ASC",
        )?;
        let rows = statement.query_map(params![id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, i64>(4)?,
            ))
        })?;
        let mut records = Vec::new();
        for row in rows {
            let (design_id, revision, plan_json, actor, created_at) = row?;
            let plan = serde_json::from_str(&plan_json).map_err(|error| {
                LodestarError::Invalid(format!(
                    "design materialization {design_id} revision {revision} has invalid JSON: {error}"
                ))
            })?;
            records.push(DesignMaterializationRecord {
                design_id,
                revision,
                plan,
                actor,
                created_at,
            });
        }
        Ok(records)
    }

    /// Atomically materialize an explicit reviewed plan. A repair appends a new
    /// revision and replaces only the current link projection; prior plans and
    /// tasks remain durable.
    pub fn materialize_design_item(
        &self,
        id: &str,
        plan: &DesignMaterializationPlan,
        actor: &str,
        repair: bool,
        now: i64,
    ) -> Result<DesignPromotion> {
        validate_materialization_plan(plan)?;
        let actor = actor.trim();
        if actor.is_empty() {
            return Err(LodestarError::Invalid(
                "a human materialization reviewer is required".to_string(),
            ));
        }
        let item = self
            .get_design_item(id)?
            .ok_or_else(|| LodestarError::NotFound(id.to_string()))?;
        if item.status != DesignStatus::Accepted {
            return Err(LodestarError::Invalid(format!(
                "design item {id} is {}; only an accepted design can be materialized",
                item.status.as_str()
            )));
        }
        let previous = self.design_materialization_history(id)?.pop();
        if previous.as_ref().is_some_and(|record| record.plan == *plan) {
            return self.resolve_promotion(&item);
        }
        if repair {
            if item.promotion_status != DesignPromotionStatus::Materialized {
                return Err(LodestarError::Invalid(format!(
                    "design item {id} has no materialization to repair"
                )));
            }
            if plan
                .rationale
                .as_deref()
                .is_none_or(|value| value.trim().is_empty())
            {
                return Err(LodestarError::Invalid(
                    "a repair rationale is required".to_string(),
                ));
            }
        } else if item.promotion_status != DesignPromotionStatus::Pending {
            return Err(LodestarError::Invalid(format!(
                "design item {id} is already materialized; use revise_design_promotion with a rationale"
            )));
        }

        let revision = item.materialization_revision + 1;
        let transaction = Transaction::new_unchecked(&self.conn, TransactionBehavior::Immediate)?;
        let won = if repair {
            transaction.execute(
                "UPDATE design_items
                 SET materialization_revision = ?2, updated_at = ?3
                 WHERE id = ?1 AND status = 'accepted' AND promotion_status = 'materialized'
                   AND materialization_revision = ?4",
                params![id, revision, now, item.materialization_revision],
            )?
        } else {
            transaction.execute(
                "UPDATE design_items
                 SET promotion_status = 'materialized', materialization_revision = ?2, updated_at = ?3
                 WHERE id = ?1 AND status = 'accepted' AND promotion_status = 'pending'
                   AND materialization_revision = ?4",
                params![id, revision, now, item.materialization_revision],
            )?
        };
        if won != 1 {
            return Err(LodestarError::Invalid(format!(
                "design item {id} materialization changed concurrently"
            )));
        }
        let plan_json = serde_json::to_string(plan).map_err(|error| {
            LodestarError::Invalid(format!("could not serialize materialization plan: {error}"))
        })?;
        transaction.execute(
            "INSERT INTO design_materializations
                 (design_id, revision, mode, plan_json, rationale, actor, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                id,
                revision,
                plan.mode.as_str(),
                plan_json,
                plan.rationale.as_deref().map(str::trim),
                actor,
                now
            ],
        )?;
        transaction.execute(
            "DELETE FROM design_task_links WHERE design_id = ?1",
            params![id],
        )?;
        transaction.execute(
            "DELETE FROM design_goal_links WHERE design_id = ?1 AND role = 'objective'",
            params![id],
        )?;

        let mut tasks = Vec::new();
        match plan.mode {
            DesignMaterializationMode::Create => {
                for draft in &plan.tasks {
                    // A repair re-states the drafts it is repairing, and the
                    // links to the tasks the previous revision created were
                    // deleted above. Creating again would leave those originals
                    // live and unreachable from the design while their twins
                    // took their place on the board.
                    let task = match coordination::live_task_titled_on(
                        &transaction,
                        &draft.goal_id,
                        &draft.title,
                    )? {
                        Some(existing) => existing,
                        None => coordination::create_task_on(
                            &transaction,
                            &draft.goal_id,
                            &draft.title,
                            &draft.acceptance,
                            now,
                        )?,
                    };
                    tasks.push(task);
                }
            }
            DesignMaterializationMode::Link => {
                for task_id in &plan.task_ids {
                    tasks.push(
                        coordination::get_task_on(&transaction, task_id)?
                            .ok_or_else(|| LodestarError::NotFound(task_id.clone()))?,
                    );
                }
            }
            DesignMaterializationMode::NoWork => {}
        }

        let mut objective_ids = Vec::new();
        for (position, task) in tasks.iter().enumerate() {
            ensure_objective(&transaction, &task.goal_id)?;
            transaction.execute(
                "INSERT INTO design_task_links (design_id, task_id, position)
                 VALUES (?1, ?2, ?3)",
                params![id, task.id, position as i64],
            )?;
            if !objective_ids.contains(&task.goal_id) {
                objective_ids.push(task.goal_id.clone());
            }
        }
        for (position, goal_id) in objective_ids.iter().enumerate() {
            transaction.execute(
                "INSERT INTO design_goal_links (design_id, goal_id, role, position)
                 VALUES (?1, ?2, 'objective', ?3)",
                params![id, goal_id, position as i64],
            )?;
        }
        let constraint_offset: i64 = transaction.query_row(
            "SELECT COALESCE(MAX(position) + 1, 0) FROM design_goal_links
             WHERE design_id = ?1 AND role <> 'objective'",
            params![id],
            |row| row.get(0),
        )?;
        for (position, draft) in plan.constraints.iter().enumerate() {
            let constraint = goals::define_goal_on(
                &transaction,
                draft.kind,
                &draft.title,
                &draft.statement,
                None,
                now,
            )?;
            transaction.execute(
                "INSERT INTO design_goal_links (design_id, goal_id, role, position)
                 VALUES (?1, ?2, ?3, ?4)",
                params![
                    id,
                    constraint.id,
                    draft.kind.as_str(),
                    constraint_offset + position as i64
                ],
            )?;
        }
        transaction.commit()?;
        let item = self
            .get_design_item(id)?
            .ok_or_else(|| LodestarError::NotFound(id.into()))?;
        self.resolve_promotion(&item)
    }

    /// Reconstruct a materialised promotion from its durable provenance links so
    /// a retry returns the same plan without re-running planning.
    pub(super) fn resolve_promotion(&self, item: &DesignItem) -> Result<DesignPromotion> {
        let record = self
            .design_materialization_history(&item.id)?
            .pop()
            .ok_or_else(|| {
                LodestarError::Invalid(format!(
                    "design item {} is materialized without an audit record",
                    item.id
                ))
            })?;
        let tasks = self.linked_tasks(&item.id)?;
        let goals = self.linked_objective_goals(&item.id)?;
        let constraints = self.linked_constraint_goals(&item.id)?;
        Ok(DesignPromotion {
            item: item.clone(),
            mode: record.plan.mode,
            revision: record.revision,
            rationale: record.plan.rationale,
            goals,
            tasks,
            constraints,
        })
    }
}
