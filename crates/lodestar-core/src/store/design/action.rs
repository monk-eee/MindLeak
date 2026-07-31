//! The append-only audit of attributed design backlog acts (ADR-0077).

use super::*;

impl LodestarStore {
    pub fn apply_design_actions(
        &self,
        ids: &[String],
        action: DesignActionKind,
        human: &str,
        reason: &str,
        now: i64,
    ) -> Result<Vec<DesignItem>> {
        if ids.is_empty() {
            return Err(LodestarError::Invalid(
                "a design action requires at least one design id".to_string(),
            ));
        }
        let transaction = Transaction::new_unchecked(&self.conn, TransactionBehavior::Immediate)?;
        for id in ids {
            let exists: bool = transaction.query_row(
                "SELECT EXISTS(SELECT 1 FROM design_items WHERE id = ?1)",
                params![id],
                |row| row.get(0),
            )?;
            if !exists {
                return Err(LodestarError::NotFound(id.clone()));
            }
            if !apply_design_action(&transaction, id, action, human, reason, now)? {
                return Err(LodestarError::Invalid(action_refusal(action, id)));
            }
        }
        transaction.commit()?;
        ids.iter()
            .map(|id| {
                self.get_design_item(id)?
                    .ok_or_else(|| LodestarError::NotFound(id.clone()))
            })
            .collect()
    }

    pub fn design_action_history(&self, id: &str) -> Result<Vec<DesignAction>> {
        if self.get_design_item(id)?.is_none() {
            return Err(LodestarError::NotFound(id.to_string()));
        }
        let mut statement = self.conn.prepare(
            "SELECT id, design_id, action, human, reason, created_at
             FROM design_actions WHERE design_id = ?1 ORDER BY id ASC",
        )?;
        let rows = statement.query_map(params![id], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, i64>(5)?,
            ))
        })?;
        let mut actions = Vec::new();
        for row in rows {
            let (id, design_id, action, human, reason, created_at) = row?;
            let action = DesignActionKind::from_tag(&action).ok_or_else(|| {
                LodestarError::Invalid(format!(
                    "design action {id} for {design_id} has unknown kind: {action}"
                ))
            })?;
            actions.push(DesignAction {
                id,
                design_id,
                action,
                human,
                reason,
                created_at,
            });
        }
        Ok(actions)
    }
}

pub(super) fn apply_design_action(
    transaction: &Transaction<'_>,
    id: &str,
    action: DesignActionKind,
    human: &str,
    reason: &str,
    now: i64,
) -> Result<bool> {
    let changed = match action {
        DesignActionKind::Defer => transaction.execute(
            "UPDATE design_items
             SET deferred_at = ?2, deferred_by = ?3, deferred_reason = ?4, updated_at = ?2
             WHERE id = ?1
               AND status = 'proposed'
               AND deferred_at IS NULL
               AND retired_at IS NULL
               AND superseded_by IS NULL",
            params![id, now, human, reason],
        )?,
        DesignActionKind::Resume => transaction.execute(
            "UPDATE design_items
             SET deferred_at = NULL, deferred_by = NULL, deferred_reason = NULL, updated_at = ?2
             WHERE id = ?1
               AND status = 'proposed'
               AND deferred_at IS NOT NULL
               AND retired_at IS NULL
               AND superseded_by IS NULL",
            params![id, now],
        )?,
        DesignActionKind::Reject => transaction.execute(
            "UPDATE design_items
             SET status = 'rejected', decided_by = ?2, reason = ?3,
                 promotion_status = 'not_required', updated_at = ?4
             WHERE id = ?1
               AND status = 'proposed'
               AND deferred_at IS NULL
               AND retired_at IS NULL
               AND superseded_by IS NULL",
            params![id, human, reason, now],
        )?,
        DesignActionKind::Retire => transaction.execute(
            "UPDATE design_items
             SET retired_at = ?2, retired_by = ?3, retired_reason = ?4, updated_at = ?2
             WHERE id = ?1 AND retired_at IS NULL",
            params![id, now, human, reason],
        )?,
    };
    if changed == 1 {
        append_design_action(transaction, id, action, human, reason, now)?;
    }
    Ok(changed == 1)
}

fn action_refusal(action: DesignActionKind, id: &str) -> String {
    match action {
        DesignActionKind::Defer => {
            format!("only an active proposed design can be deferred: {id}")
        }
        DesignActionKind::Resume => format!("only a deferred proposal can be resumed: {id}"),
        DesignActionKind::Reject => {
            format!("only an active, non-deferred proposal can be rejected: {id}")
        }
        DesignActionKind::Retire => format!("design item already retired: {id}"),
    }
}
