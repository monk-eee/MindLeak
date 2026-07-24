//! Reviewed design materialization and append-only repair facade.

use crate::design::{
    DesignMaterializationMode, DesignMaterializationPlan, DesignMaterializationRecord,
    DesignPromotion, DesignStatus, DesignTaskDraft,
};
use crate::{now_unix, GoalKind, Lodestar, LodestarError, Result};

impl Lodestar {
    pub fn design_promotion(&self, id: &str) -> Result<Option<DesignPromotion>> {
        self.store.design_promotion(id)
    }

    pub fn design_materialization_history(
        &self,
        id: &str,
    ) -> Result<Vec<DesignMaterializationRecord>> {
        self.store.design_materialization_history(id)
    }

    /// Produce a read-only suggested create plan for human review.
    pub fn plan_design_promotion(
        &self,
        id: &str,
        objective_goal_id: &str,
    ) -> Result<DesignMaterializationPlan> {
        let item = self
            .store
            .get_design_item(id)?
            .ok_or_else(|| LodestarError::NotFound(id.to_string()))?;
        if item.status != DesignStatus::Accepted {
            return Err(LodestarError::Invalid(format!(
                "design item {id} is {}; only an accepted design can be planned",
                item.status.as_str()
            )));
        }
        let goal = self
            .store
            .get_goal(objective_goal_id)?
            .ok_or_else(|| LodestarError::NotFound(objective_goal_id.to_string()))?;
        if goal.kind != GoalKind::Objective {
            return Err(LodestarError::Invalid(format!(
                "promotion target {objective_goal_id} is a {}; tasks must serve an objective",
                goal.kind.as_str()
            )));
        }
        let statement = if item.summary.trim().is_empty() {
            format!("Implement the design recorded in {}", item.adr_path)
        } else {
            item.summary.clone()
        };
        let drafts = self.decompose_drafts(&item.title, &statement);
        Ok(DesignMaterializationPlan {
            mode: DesignMaterializationMode::Create,
            tasks: drafts
                .into_iter()
                .map(|(title, acceptance)| DesignTaskDraft {
                    goal_id: objective_goal_id.to_string(),
                    title,
                    acceptance,
                })
                .collect(),
            task_ids: Vec::new(),
            constraints: Vec::new(),
            rationale: None,
        })
    }

    /// Materialize exactly the reviewed plan.
    pub fn promote_design(
        &self,
        id: &str,
        plan: &DesignMaterializationPlan,
    ) -> Result<DesignPromotion> {
        let item = self
            .store
            .get_design_item(id)?
            .ok_or_else(|| LodestarError::NotFound(id.to_string()))?;
        let actor = item.decided_by.as_deref().ok_or_else(|| {
            LodestarError::Invalid(format!("accepted design item {id} has no human decider"))
        })?;
        self.store
            .materialize_design_item(id, plan, actor, false, now_unix())
    }

    /// Append an attributed repair revision and replace the current projection.
    pub fn revise_design_promotion(
        &self,
        id: &str,
        human: &str,
        plan: &DesignMaterializationPlan,
    ) -> Result<DesignPromotion> {
        let human = human.trim();
        let item = self
            .store
            .get_design_item(id)?
            .ok_or_else(|| LodestarError::NotFound(id.to_string()))?;
        if human.is_empty() || item.proposed_by.as_deref() == Some(human) {
            return Err(LodestarError::Invalid(
                "a repair requires a human reviewer other than the proposing agent".to_string(),
            ));
        }
        self.store
            .materialize_design_item(id, plan, human, true, now_unix())
    }
}
