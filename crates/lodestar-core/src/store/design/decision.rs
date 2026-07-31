//! The guarded accept/reject transition, and repairing a decision after the fact.
//!
//! Split out of `design.rs` (see `super`); the code is unchanged.

use super::action::apply_design_action;
use super::*;

impl LodestarStore {
    /// Reconcile structured repository ADR metadata into the durable design
    /// ledger. Existing rows always win: repository discovery must never
    /// overwrite a Design Board decision or re-arm promotion.
    /// Reconcile one ADR's repository-derived metadata. The ADR file is
    /// authoritative for `title` and `summary`, so both refresh on every pass —
    /// otherwise a design registered before its summary could be extracted keeps
    /// an empty one forever, and promotion planning sees only the title. A
    /// durable human decision is never touched: `status`, `decided_by`,
    /// `reason`, `proposed_by`, and promotion state all survive reconciliation.
    /// `updated_at` moves only when a fact actually changed, so a no-op pass
    /// stays genuinely idempotent.
    pub fn reconcile_design_item(&self, metadata: &DesignMetadata, now: i64) -> Result<DesignItem> {
        let id = design_id_from_path(&metadata.adr_path);
        // The declared status is imported as-is, so a repository's ADR history
        // arrives as the record it already is rather than as 35 decisions
        // pretending to be pending. The cost is that an imported status has no
        // `decided_by`: it reflects a decision, it does not record one. That is
        // visible in the row, and `reopen_undecided_design` is the way to turn
        // one into an attributed decision (ADR-0047).
        self.conn.execute(
            "INSERT INTO design_items
                (id, adr_path, title, summary, status, proposed_by, decided_by,
                 reason, created_at, updated_at, promotion_status, materialization_revision)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, NULL, NULL, ?7, ?7,
                     'not_required', 0)
             ON CONFLICT(id) DO UPDATE SET
                 title = excluded.title,
                 summary = excluded.summary,
                 updated_at = CASE
                     WHEN design_items.title <> excluded.title
                       OR design_items.summary <> excluded.summary
                     THEN excluded.updated_at
                     ELSE design_items.updated_at
                 END",
            params![
                id,
                metadata.adr_path,
                metadata.title,
                metadata.summary,
                metadata.status.as_str(),
                metadata.proposed_by,
                now
            ],
        )?;
        self.get_design_item(&id)?
            .ok_or_else(|| LodestarError::NotFound(id))
    }

    /// Return a design whose status was never actually decided to `proposed`,
    /// so a human can decide it (ADR-0047).
    ///
    /// Guarded on `decided_by IS NULL`: a real decision carries an actor, and
    /// this verb must never be able to erase one. It also refuses once
    /// promotion has moved off `not_required`, because materialized work rests
    /// on that acceptance. What remains is exactly the damage an over-trusting
    /// reconciliation caused — a status with nobody behind it.
    pub fn reopen_undecided_design(&self, id: &str, now: i64) -> Result<bool> {
        if self.get_design_item(id)?.is_none() {
            return Err(LodestarError::NotFound(id.to_string()));
        }
        let changed = self.conn.execute(
            "UPDATE design_items
             SET status = 'proposed', reason = NULL, updated_at = ?2
             WHERE id = ?1
               AND decided_by IS NULL
               AND status <> 'proposed'
               AND promotion_status = 'not_required'
               AND retired_at IS NULL",
            params![id, now],
        )?;
        Ok(changed == 1)
    }

    /// Record who made a decision the ledger already asserts but attributes to
    /// nobody (ADR-0051). Sets `decided_by` and nothing else: the status, the
    /// reason, and the promotion state are all left exactly as they are,
    /// because the decision itself is not in question — only who made it.
    ///
    /// The guard is the deliberate complement of
    /// [`reopen_undecided_design`](Self::reopen_undecided_design). That verb
    /// takes the rows still worth deciding properly; this one takes the rows it
    /// must refuse, where promotion has already materialised work and reopening
    /// would leave tasks descending from a decision the ledger no longer shows.
    /// Between them every undecided row has exactly one route, so neither is a
    /// softer way of doing the other's job.
    ///
    /// `decided_by IS NULL` is the load-bearing condition: a recorded human act
    /// is never overwritten here, so a wrong name cannot be quietly corrected
    /// into a different one.
    pub fn attribute_design_decision(&self, id: &str, human: &str, now: i64) -> Result<bool> {
        if self.get_design_item(id)?.is_none() {
            return Err(LodestarError::NotFound(id.to_string()));
        }
        let changed = self.conn.execute(
            "UPDATE design_items
             SET decided_by = ?2, updated_at = ?3
             WHERE id = ?1
               AND decided_by IS NULL
               AND status <> 'proposed'
               AND promotion_status <> 'not_required'",
            params![id, human, now],
        )?;
        Ok(changed == 1)
    }

    /// Every distinct decider label the ledger has recorded.
    ///
    /// Read so a new label can be compared against the ones already in use
    /// before it becomes permanent. A label is never validated — it is an
    /// unverifiable declaration (ADR-0071) — but it can be compared, and the
    /// comparison is the only chance to catch a typo while it is still fixable.
    pub fn recorded_deciders(&self) -> Result<Vec<String>> {
        let mut statement = self.conn.prepare(
            "SELECT DISTINCT decided_by FROM design_items
              WHERE decided_by IS NOT NULL AND TRIM(decided_by) <> ''
              ORDER BY decided_by",
        )?;
        let rows = statement.query_map([], |row| row.get::<_, String>(0))?;
        collect(rows)
    }

    /// Park an active proposal without deciding it (ADR-0077).
    pub fn defer_design_item(
        &self,
        id: &str,
        human: &str,
        reason: &str,
        now: i64,
    ) -> Result<DesignItem> {
        self.apply_design_actions(
            &[id.to_string()],
            DesignActionKind::Defer,
            human,
            reason,
            now,
        )?
        .pop()
        .ok_or_else(|| LodestarError::NotFound(id.to_string()))
    }

    /// Return a deferred proposal to the working board (ADR-0077).
    pub fn resume_design_item(
        &self,
        id: &str,
        human: &str,
        reason: &str,
        now: i64,
    ) -> Result<DesignItem> {
        self.apply_design_actions(
            &[id.to_string()],
            DesignActionKind::Resume,
            human,
            reason,
            now,
        )?
        .pop()
        .ok_or_else(|| LodestarError::NotFound(id.to_string()))
    }

    /// Guarded CAS: move a *proposed* item to accepted/rejected. Returns `false`
    /// when the item is not currently proposed (missing or already decided), so
    /// a concurrent second decider cannot overwrite the first.
    pub fn decide_design_item(
        &self,
        id: &str,
        target: DesignStatus,
        decided_by: &str,
        reason: Option<&str>,
        now: i64,
    ) -> Result<bool> {
        // Accepting arms promotion (pending); any other decision leaves it off.
        let transaction = Transaction::new_unchecked(&self.conn, TransactionBehavior::Immediate)?;
        if target == DesignStatus::Rejected {
            let changed = apply_design_action(
                &transaction,
                id,
                DesignActionKind::Reject,
                decided_by,
                reason.unwrap_or_default(),
                now,
            )?;
            transaction.commit()?;
            return Ok(changed);
        }
        let changed = transaction.execute(
            "UPDATE design_items
             SET status = ?2, decided_by = ?3, reason = ?4,
                 promotion_status = 'pending', updated_at = ?5
             WHERE id = ?1
               AND status = 'proposed'
               AND deferred_at IS NULL
               AND retired_at IS NULL
               AND superseded_by IS NULL",
            params![id, target.as_str(), decided_by, reason, now],
        )?;
        transaction.commit()?;
        Ok(changed == 1)
    }
}
