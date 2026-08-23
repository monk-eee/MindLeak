//! Federated claim projection (ADR-0096 clause 3): copying an
//! Ackplane-decided grant or release into the local task cache. Both methods
//! are unconditional — Ackplane already ran the compare-and-swap, so these
//! write exactly what it decided rather than re-deciding anything locally.

use super::super::*;

impl LodestarStore {
    /// Copy an Ackplane-granted claim into the local task cache (ADR-0096
    /// clause 3). Unconditional: Ackplane already ran the CAS, so this writes
    /// exactly what it decided rather than re-deciding anything locally. Used
    /// after `delegate`/`renew`/`recover` all grant the same result shape.
    pub(crate) fn apply_federated_grant(
        &self,
        id: &str,
        agent: &str,
        grant: &FederatedClaimGrant,
        kind: TaskEventKind,
        now: i64,
    ) -> Result<()> {
        let transaction = Transaction::new_unchecked(&self.conn, TransactionBehavior::Immediate)?;
        transaction.execute(
            "UPDATE tasks
                SET status = 'claimed', owner = ?2, branch = ?3,
                    claim_started_at = ?4, lease_expires_at = ?5, parked_at = NULL,
                    updated_at = ?6
              WHERE id = ?1",
            params![
                id,
                grant.owner,
                grant.branch,
                grant.claim_started_at,
                grant.lease_expires_at,
                now
            ],
        )?;
        transaction.execute(
            "DELETE FROM task_scopes WHERE task_id = ?1 AND kind = 'path'",
            params![id],
        )?;
        for path in &grant.paths {
            transaction.execute(
                "INSERT INTO task_scopes (task_id, kind, value) VALUES (?1, 'path', ?2)",
                params![id, path],
            )?;
        }
        transaction.execute(
            "DELETE FROM task_scopes WHERE task_id = ?1 AND kind = 'symbol'",
            params![id],
        )?;
        for symbol in &grant.symbols {
            transaction.execute(
                "INSERT INTO task_scopes (task_id, kind, value) VALUES (?1, 'symbol', ?2)",
                params![id, symbol],
            )?;
        }
        events::record(
            &transaction,
            id,
            kind,
            Some(agent),
            now,
            &format!(
                r#"{{"source":"federated","claim_lapses":{}}}"#,
                grant.claim_lapses
            ),
        )?;
        transaction.commit()?;
        Ok(())
    }

    /// Clear the local task cache after Ackplane confirms a release (ADR-0096
    /// clause 3): unconditional, matching `apply_federated_grant` — Ackplane
    /// already decided the release, so this projects that decision rather
    /// than re-guarding ownership itself.
    pub(crate) fn apply_federated_release(&self, id: &str, agent: &str, now: i64) -> Result<()> {
        let transaction = Transaction::new_unchecked(&self.conn, TransactionBehavior::Immediate)?;
        transaction.execute(
            "UPDATE tasks
                SET status = 'open', owner = NULL, claim_started_at = NULL,
                    lease_expires_at = NULL, updated_at = ?2
              WHERE id = ?1",
            params![id, now],
        )?;
        events::record(
            &transaction,
            id,
            TaskEventKind::Released,
            Some(agent),
            now,
            r#"{"source":"federated"}"#,
        )?;
        transaction.commit()?;
        Ok(())
    }
}
