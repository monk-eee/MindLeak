//! Current projection of what a design item is linked to.
//!
//! Split out of `design.rs` (see `super`); the code is unchanged.

use super::*;

impl LodestarStore {
    pub(super) fn linked_tasks(&self, design_id: &str) -> Result<Vec<Task>> {
        let ids: Vec<String> = {
            let mut stmt = self.conn.prepare(
                "SELECT task_id FROM design_task_links WHERE design_id = ?1 ORDER BY position ASC",
            )?;
            let rows = stmt.query_map(params![design_id], |row| row.get::<_, String>(0))?;
            collect(rows)?
        };
        let mut tasks = Vec::with_capacity(ids.len());
        for id in ids {
            if let Some(task) = self.get_task(&id)? {
                tasks.push(task);
            }
        }
        Ok(tasks)
    }

    pub(super) fn linked_constraint_goals(&self, design_id: &str) -> Result<Vec<Goal>> {
        let ids: Vec<String> = {
            let mut stmt = self.conn.prepare(
                "SELECT goal_id FROM design_goal_links
                 WHERE design_id = ?1 AND role <> 'objective' ORDER BY position ASC",
            )?;
            let rows = stmt.query_map(params![design_id], |row| row.get::<_, String>(0))?;
            collect(rows)?
        };
        let mut goals = Vec::with_capacity(ids.len());
        for id in ids {
            if let Some(goal) = self.get_goal(&id)? {
                goals.push(goal);
            }
        }
        Ok(goals)
    }

    pub(super) fn linked_objective_goals(&self, design_id: &str) -> Result<Vec<Goal>> {
        let ids: Vec<String> = {
            let mut stmt = self.conn.prepare(
                "SELECT goal_id FROM design_goal_links
                 WHERE design_id = ?1 AND role = 'objective' ORDER BY position ASC",
            )?;
            let rows = stmt.query_map(params![design_id], |row| row.get::<_, String>(0))?;
            collect(rows)?
        };
        let mut goals = Vec::with_capacity(ids.len());
        for id in ids {
            if let Some(goal) = self.get_goal(&id)? {
                goals.push(goal);
            }
        }
        Ok(goals)
    }
}
