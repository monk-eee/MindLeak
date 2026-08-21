//! Claim lifecycle: acquire, renew, release, and recover ownership of a task,
//! plus the read-only queries over declared scope and claim windows.

use crate::{
    now_unix, ClaimOverlapReport, ClaimTransfer, ClaimWindow, FederatedClaimOutcome, Lodestar,
    LodestarError, Result, Task, TaskEventKind, TaskScope,
};

impl Lodestar {
    pub fn next_task(&self) -> Result<Option<Task>> {
        self.store.next_task(now_unix())
    }

    pub fn claim_task(&self, id: &str, agent: &str, lease_secs: i64) -> Result<bool> {
        let agent = self.resolve_agent(agent)?;
        self.store.claim_task(id, agent, lease_secs, now_unix())
    }

    /// Atomically claim work and declare its advisory path/symbol scope
    /// (ADR-0024). A losing claimant cannot overwrite scope.
    pub fn claim_task_with_scope(
        &self,
        id: &str,
        agent: &str,
        lease_secs: i64,
        scope: &TaskScope,
    ) -> Result<bool> {
        let agent = self.resolve_agent(agent)?;
        self.store
            .claim_task_with_scope(id, agent, lease_secs, scope, now_unix())
    }

    /// Claim work while replacing only explicitly supplied scope fields.
    ///
    /// Routes through a federated repository's Ackplane claim CAS when
    /// [`with_federated_claim_authority`](Self::with_federated_claim_authority)
    /// was called (ADR-0096 clauses 2-4): Ackplane is the sole authority, so
    /// no local CAS decision runs before or after the remote request. A
    /// rejection or transport failure leaves the local row untouched — the
    /// former resolves to `Ok(false)` exactly like a lost local CAS, the
    /// latter surfaces as `Err` rather than being silently treated as a loss.
    pub fn claim_task_with_partial_scope(
        &self,
        id: &str,
        agent: &str,
        lease_secs: i64,
        paths: Option<&[String]>,
        symbols: Option<&[String]>,
    ) -> Result<bool> {
        let agent = self.resolve_agent(agent)?;
        if let Some(authority) = &self.federated_claim_authority {
            let branch = self.store.declared_branch(Some(agent))?;
            let existing = self.store.task_scope(id)?;
            let paths = paths.map(<[String]>::to_vec).unwrap_or(existing.paths);
            let symbols = symbols.map(<[String]>::to_vec).unwrap_or(existing.symbols);
            return match authority.delegate(
                id,
                agent,
                branch.as_deref(),
                lease_secs,
                &paths,
                &symbols,
            )? {
                FederatedClaimOutcome::Granted(grant) => {
                    self.store.apply_federated_grant(
                        id,
                        agent,
                        &grant,
                        TaskEventKind::Claimed,
                        now_unix(),
                    )?;
                    Ok(true)
                }
                FederatedClaimOutcome::Rejected { .. } => Ok(false),
            };
        }
        self.store
            .claim_task_with_partial_scope(id, agent, lease_secs, paths, symbols, now_unix())
    }

    /// Read one task's declared advisory scope.
    pub fn task_scope(&self, task_id: &str) -> Result<TaskScope> {
        self.store.task_scope(task_id)
    }

    /// Has this already been done?
    ///
    /// Distinct from `check_overlap`, which asks who is touching a file *right
    /// now* and only sees live claims. This asks whether the work exists at all,
    /// so it includes finished and abandoned tasks — a task that is already
    /// `done` is the most useful answer it can give, and the one `board` hides.
    ///
    /// Answering nothing is a legitimate answer, and answering wrongly is not:
    /// this reports, and no caller may refuse work on the strength of it. A
    /// second task against one goal is often right, and a gate here would be
    /// wrong more often than it was right (ADR-0015).
    pub fn existing_work(&self, goal_id: Option<&str>, paths: &[String]) -> Result<Vec<Task>> {
        self.store.existing_work(goal_id, paths)
    }

    /// The continuity of a task's current evidence window, derived from the log
    /// (ADR-0064 d5/d6). Replaces the `claim_lapses` / `unleased_seconds`
    /// columns that used to ride on the task row.
    pub fn claim_window(&self, task_id: &str) -> Result<ClaimWindow> {
        self.store.claim_window(task_id)
    }

    /// Read-only active-claim intersection for concrete requested paths/symbols.
    /// It warns; it never locks.
    ///
    /// `requester` is optional so an agent that never registered a session still
    /// gets today's answer — the signal degrades to `undeclared` rather than the
    /// check refusing to run (ADR-0035 decision 5).
    ///
    /// Routes through a federated repository's Ackplane claim registry when
    /// [`with_federated_claim_source`](Self::with_federated_claim_source) was
    /// called (ADR-0096 clause 5); otherwise unchanged local-table behavior.
    pub fn check_claim_overlap(
        &self,
        scope: &TaskScope,
        exclude_task_id: Option<&str>,
        requester: Option<&str>,
    ) -> Result<ClaimOverlapReport> {
        if let Some(source) = &self.federated_claim_source {
            return self.store.check_federated_claim_overlap(
                source.as_ref(),
                scope,
                exclude_task_id,
                requester,
            );
        }
        self.store
            .check_claim_overlap(scope, exclude_task_id, requester, now_unix())
    }

    /// Extend a still-live lease. Routes through Ackplane in federated mode,
    /// same rules as [`claim_task_with_partial_scope`](Self::claim_task_with_partial_scope).
    pub fn renew_lease(&self, id: &str, agent: &str, lease_secs: i64) -> Result<bool> {
        let agent = self.resolve_agent(agent)?;
        if let Some(authority) = &self.federated_claim_authority {
            return match authority.renew(id, agent, lease_secs)? {
                FederatedClaimOutcome::Granted(grant) => {
                    self.store.apply_federated_grant(
                        id,
                        agent,
                        &grant,
                        TaskEventKind::LeaseRenewed,
                        now_unix(),
                    )?;
                    Ok(true)
                }
                FederatedClaimOutcome::Rejected { .. } => Ok(false),
            };
        }
        self.store.renew_lease(id, agent, lease_secs, now_unix())
    }

    /// Renew a lease because the owner is demonstrably still working (ADR-0052).
    ///
    /// Called as a side effect of any authenticated call that names a task, so
    /// the heartbeat is free and an agent doing its job cannot lose its claim.
    /// Silent by design: it reports whether it renewed, and every caller ignores
    /// that, because the call it rides on has its own job and must not fail
    /// because a heartbeat did not apply.
    pub fn touch_lease(&self, id: &str, agent: &str, lease_secs: i64) -> Result<bool> {
        let agent = self.resolve_agent(agent)?;
        self.store.touch_lease(id, agent, lease_secs, now_unix())
    }

    /// Hand the claim back to open. Routes through Ackplane in federated
    /// mode: a live lease is holed immediately rather than deleted (ADR-0096
    /// clause 6), same as the local behavior it replaces.
    pub fn release_task(&self, id: &str, agent: &str) -> Result<bool> {
        let agent = self.resolve_agent(agent)?;
        if let Some(authority) = &self.federated_claim_authority {
            let released = authority.release(id, agent)?;
            if released {
                self.store.apply_federated_release(id, agent, now_unix())?;
            }
            return Ok(released);
        }
        self.store.release_task(id, agent, now_unix())
    }

    /// Take over a stranded claim. Routes through Ackplane in federated mode,
    /// recovering only an expired lease (ADR-0096 clause 6): the wire
    /// contract has no reviewer field, so a paused-task transfer before its
    /// grace expires — the one path that needs a human reviewer locally —
    /// is refused rather than silently attempted without one.
    pub fn recover_claim(
        &self,
        id: &str,
        expected_owner: &str,
        recovering: (&str, &str),
        reason: &str,
        reviewer: Option<&str>,
        lease_secs: i64,
    ) -> Result<bool> {
        let (agent, name) = recovering;
        let agent = self.resolve_agent(agent)?;
        if let Some(authority) = &self.federated_claim_authority {
            if reviewer.is_some() {
                return Err(LodestarError::Federated(
                    "federated recovery does not yet support a paused-task transfer with a \
                     human reviewer; only an expired lease can be recovered (ADR-0096 clause 6)"
                        .to_string(),
                ));
            }
            let branch = self.store.declared_branch(Some(agent))?;
            let scope = self.store.task_scope(id)?;
            let request = crate::FederatedClaimRecoverRequest {
                task_id: id.to_string(),
                expected_owner: expected_owner.to_string(),
                owner: agent.to_string(),
                branch,
                reason: reason.to_string(),
                lease_secs,
                paths: scope.paths,
                symbols: scope.symbols,
            };
            return match authority.recover(&request)? {
                FederatedClaimOutcome::Granted(grant) => {
                    self.store.apply_federated_grant(
                        id,
                        agent,
                        &grant,
                        TaskEventKind::ClaimRecovered,
                        now_unix(),
                    )?;
                    Ok(true)
                }
                FederatedClaimOutcome::Rejected { .. } => Ok(false),
            };
        }
        self.store.recover_claim_authorized(
            id,
            expected_owner,
            crate::store::RecoveringSession { agent, name },
            (reason, reviewer),
            lease_secs,
            now_unix(),
        )
    }

    pub fn claim_transfer_history(&self, task_id: &str) -> Result<Vec<ClaimTransfer>> {
        self.store.claim_transfer_history(task_id)
    }
}
