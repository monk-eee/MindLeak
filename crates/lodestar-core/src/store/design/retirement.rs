//! Ending a design item's life without deciding it: retire and supersede.
//!
//! Split out of `design.rs` (see `super`); the code is unchanged.

use super::*;

impl LodestarStore {
    /// Retire one design record, attributed to a person (ADR-0042).
    ///
    /// Never inferred: nothing retires a design because its ADR file is absent,
    /// because several worktrees on different branches share one database and a
    /// missing file is a routine branch-local condition, not evidence.
    ///
    /// Guarded on `retired_at IS NULL` in a single statement so two concurrent
    /// retirements cannot both claim to be the one that did it, and so the
    /// original actor and reason are never overwritten.
    pub fn retire_design_item(
        &self,
        id: &str,
        human: &str,
        reason: &str,
        now: i64,
    ) -> Result<DesignItem> {
        self.apply_design_actions(
            &[id.to_string()],
            DesignActionKind::Retire,
            human,
            reason,
            now,
        )?
        .pop()
        .ok_or_else(|| LodestarError::NotFound(id.to_string()))
    }

    /// Record that an accepted design has been replaced by another (ADR-0050).
    ///
    /// Guarded on `status = 'accepted' AND decided_by IS NOT NULL`: superseding
    /// is a statement *about a decision that was actually made*. A row carrying
    /// an imported status with nobody behind it has no decision to supersede —
    /// it should be reopened and decided (ADR-0047), or retired (ADR-0042).
    ///
    /// `status` is deliberately untouched. The design stays `accepted` because
    /// it was accepted; supersession is a separate fact, and collapsing the two
    /// would lose both the decision and what replaced it.
    ///
    /// Guarded on `superseded_by IS NULL` in a single statement so two
    /// concurrent supersessions cannot both claim to be the one that did it,
    /// and the original actor is never overwritten.
    pub fn supersede_design_item(
        &self,
        id: &str,
        superseded_by: &str,
        human: &str,
        now: i64,
    ) -> Result<DesignItem> {
        let item = self
            .get_design_item(id)?
            .ok_or_else(|| LodestarError::NotFound(id.to_string()))?;
        // A dangling replacement would leave a reader with a withdrawn decision
        // and nowhere to go, so the successor must already be registered.
        if self.get_design_item(superseded_by)?.is_none() {
            return Err(LodestarError::NotFound(superseded_by.to_string()));
        }
        if superseded_by == id {
            return Err(LodestarError::Invalid(format!(
                "a design cannot supersede itself: {id}"
            )));
        }
        if item.status != DesignStatus::Accepted || item.decided_by.is_none() {
            return Err(LodestarError::Invalid(format!(
                "only an accepted design with a recorded decider can be superseded: {id}"
            )));
        }
        let changed = self.conn.execute(
            "UPDATE design_items
             SET superseded_by = ?2, superseded_at = ?3, superseded_by_human = ?4,
                 updated_at = ?3
             WHERE id = ?1
               AND superseded_by IS NULL
               AND retired_at IS NULL",
            params![id, superseded_by, now, human],
        )?;
        if changed == 0 {
            return Err(LodestarError::Invalid(format!(
                "design item is already superseded or retired: {id}"
            )));
        }
        self.get_design_item(id)?
            .ok_or_else(|| LodestarError::NotFound(id.to_string()))
    }
}
