//! Coordination snapshot and read-only diagnostics over the live board.

use crate::stalls::{stalls, Stall, StallKind};
use crate::{now_unix, BoardFinding, Lodestar, Result, ReworkReport, Task};

impl Lodestar {
    pub fn board(&self, include_terminal: bool) -> Result<Vec<Task>> {
        self.store.board(include_terminal)
    }

    /// Diagnose the live board: duplicate titles, the same title forked across
    /// goals, and work blocked on no predecessor.
    ///
    /// Read-only and judgement-free. Every condition here was found and
    /// repaired by hand before this existed, and none is surfaced by another
    /// view — `stalled` reports lateness, and nothing about a duplicate or an
    /// ungated block is late.
    pub fn diagnose_board(&self) -> Result<Vec<BoardFinding>> {
        self.store.diagnose_board()
    }

    /// The rework rate ADR-0057 named as this line's measurable outcome.
    ///
    /// `since` is a unix second; pass 0 for the whole ledger. Windowing is the
    /// point rather than a convenience: a lifetime average cannot show a rate
    /// falling, so it cannot answer the question the ADR asked.
    pub fn rework_rate(&self, since: i64) -> Result<ReworkReport> {
        self.store.rework_rate(since)
    }

    /// Every task that is not progressing, and the fact that stalled it.
    ///
    /// Read-only and evidence-free: it records nothing, changes no task state,
    /// and produces no verdict. It reports how long each stall has been true
    /// and deliberately does not decide whether that is too long — inventing a
    /// staleness threshold here would make it policy nobody agreed to.
    ///
    /// Terminal tasks are included in the scan because a block behind a `done`
    /// or `abandoned` task is precisely the stale block worth surfacing.
    ///
    /// The wait graph (ADR-0046) is read from the same place the fleet view
    /// reads it, so a parked task is named by who actually owes the answer — a
    /// human, a specific peer, or nobody reachable because the peer is waiting
    /// back. Deriving it twice would let the two surfaces disagree.
    pub fn stalled_work(&self) -> Result<Vec<Stall>> {
        let tasks = self.store.board(true)?;
        let waits = self.store.waits()?;
        Ok(stalls(&tasks, &waits, now_unix()))
    }

    /// The work only a person can move: completed into `in_review` awaiting a
    /// decision, or parked on a question addressed to nobody in particular.
    ///
    /// A filter over [`Self::stalled_work`] rather than its own query, because
    /// the two must never disagree about what "waiting on a human" means. The
    /// stall rules already encode it; a second derivation would drift from them
    /// the first time either changed.
    ///
    /// This exists on the facade rather than in the MCP layer so the fact is
    /// available to any caller. It is a fleet-level question, not a per-agent
    /// one: completing into `in_review` clears the owner, and a human has no
    /// agent id (ADR-0046), so there is nobody to filter by. The agent is told
    /// because the agent is the only thing the human talks to.
    pub fn work_awaiting_a_human(&self) -> Result<Vec<Stall>> {
        Ok(self
            .stalled_work()?
            .into_iter()
            .filter(|stall| stall.kind == StallKind::AwaitingHuman)
            .collect())
    }

    /// Work a newly arrived agent can rescue because waiting for the current
    /// owner cannot make progress: an expired claim, a pause beyond its
    /// protection grace, or a wait cycle.
    ///
    /// Addressed peer waits remain private to the addressed agent, healthy
    /// pauses remain with their owner, and human decisions stay in
    /// [`Self::work_awaiting_a_human`]. This filter changes no task state; it
    /// only makes the existing stalled-work facts unavoidable at session start.
    pub fn work_needing_rescue(&self) -> Result<Vec<Stall>> {
        Ok(self
            .stalled_work()?
            .into_iter()
            .filter(|stall| {
                matches!(stall.kind, StallKind::LapsedLease | StallKind::Deadlocked)
                    || (stall.kind == StallKind::Paused
                        && stall.stalled_seconds > crate::store::PARKING_GRACE_SECS)
            })
            .collect())
    }
}
