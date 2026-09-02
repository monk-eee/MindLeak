//! Federated claim projection (ADR-0096 clause 3, extended by ADR-0096
//! clause completion to park/answer): copying an Ackplane-decided grant,
//! release, park, or answer into the local task cache. Every method here is
//! unconditional — Ackplane already ran the compare-and-swap, so these write
//! exactly what it decided rather than re-deciding anything locally.

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

    /// Copy an Ackplane-confirmed park into the local task cache (ADR-0096
    /// clause completion): unconditional, matching `apply_federated_grant`/
    /// `apply_federated_release` — Ackplane already decided the park, so
    /// this projects that decision and records the question rather than
    /// re-guarding ownership itself. The question text is local-only
    /// (task_qa), never sent to Ackplane.
    pub(crate) fn apply_federated_park(
        &self,
        id: &str,
        agent: &str,
        question: &str,
        audience: Option<&str>,
        now: i64,
    ) -> Result<()> {
        let transaction = Transaction::new_unchecked(&self.conn, TransactionBehavior::Immediate)?;
        transaction.execute(
            "UPDATE tasks
                SET status = 'needs_input', lease_expires_at = NULL, parked_at = ?2,
                    updated_at = ?2
              WHERE id = ?1",
            params![id, now],
        )?;
        transaction.execute(
            "INSERT INTO task_qa (task_id, kind, body, author, audience, created_at)
             VALUES (?1, 'question', ?2, ?3, ?4, ?5)",
            params![id, question, agent, audience, now],
        )?;
        events::record(
            &transaction,
            id,
            TaskEventKind::Questioned,
            Some(agent),
            now,
            &match audience {
                Some(to) => format!(r#"{{"source":"federated","audience":"{to}"}}"#),
                None => r#"{"source":"federated"}"#.to_string(),
            },
        )?;
        transaction.commit()?;
        Ok(())
    }

    /// Copy an Ackplane-confirmed answer into the local task cache
    /// (ADR-0096 clause completion): unconditional, matching
    /// `apply_federated_park` — Ackplane already decided the answer grants
    /// the parking owner a fresh lease, so this projects that decision and
    /// records the answer text (local-only, task_qa) rather than
    /// re-guarding ownership itself.
    pub(crate) fn apply_federated_answer(
        &self,
        id: &str,
        author: &str,
        answer: &str,
        grant: &FederatedClaimGrant,
        now: i64,
    ) -> Result<()> {
        let transaction = Transaction::new_unchecked(&self.conn, TransactionBehavior::Immediate)?;
        transaction.execute(
            "UPDATE tasks
                SET status = 'claimed', lease_expires_at = ?2, parked_at = NULL,
                    updated_at = ?3
              WHERE id = ?1",
            params![id, grant.lease_expires_at, now],
        )?;
        transaction.execute(
            "INSERT INTO task_qa (task_id, kind, body, author, audience, created_at)
             VALUES (?1, 'answer', ?2, ?3, NULL, ?4)",
            params![id, answer, author, now],
        )?;
        events::record(
            &transaction,
            id,
            TaskEventKind::Answered,
            Some(author),
            now,
            r#"{"source":"federated"}"#,
        )?;
        transaction.commit()?;
        Ok(())
    }

    /// Copy an Ackplane-confirmed park into the local task cache as a
    /// deliberate `paused` suspension (ADR-0096 clause completion) rather
    /// than `ask_question`'s `needs_input` — the same wire-level `park` (no
    /// free text beyond the CAS itself) backs both local shapes, since
    /// Ackplane arbitrates only the claim-state transition, never which
    /// local status name it represents.
    pub(crate) fn apply_federated_pause(
        &self,
        id: &str,
        agent: &str,
        reason: Option<&str>,
        now: i64,
    ) -> Result<()> {
        let transaction = Transaction::new_unchecked(&self.conn, TransactionBehavior::Immediate)?;
        transaction.execute(
            "UPDATE tasks
                SET status = 'paused', lease_expires_at = NULL, parked_at = ?2,
                    updated_at = ?2
              WHERE id = ?1",
            params![id, now],
        )?;
        super::super::questions::append_note_on(&transaction, id, reason, agent, now)?;
        events::record(
            &transaction,
            id,
            TaskEventKind::Paused,
            Some(agent),
            now,
            r#"{"source":"federated"}"#,
        )?;
        transaction.commit()?;
        Ok(())
    }

    /// Copy an Ackplane-confirmed answer into the local task cache as a
    /// resume (ADR-0096 clause completion) — the counterpart to
    /// `apply_federated_pause`, mirroring `apply_federated_answer` but with
    /// no dialogue text to record (a deliberate pause carries only an
    /// optional note, already appended when it was paused).
    pub(crate) fn apply_federated_resume(
        &self,
        id: &str,
        agent: &str,
        grant: &FederatedClaimGrant,
        now: i64,
    ) -> Result<()> {
        let transaction = Transaction::new_unchecked(&self.conn, TransactionBehavior::Immediate)?;
        transaction.execute(
            "UPDATE tasks
                SET status = 'claimed', lease_expires_at = ?2, parked_at = NULL,
                    updated_at = ?3
              WHERE id = ?1",
            params![id, grant.lease_expires_at, now],
        )?;
        events::record(
            &transaction,
            id,
            TaskEventKind::Resumed,
            Some(agent),
            now,
            r#"{"source":"federated"}"#,
        )?;
        transaction.commit()?;
        Ok(())
    }
}
