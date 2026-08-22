//! Advisory overlap detection: local and federated pre-flight scope checks
//! that change no state and grant no lock, plus the declared-branch and
//! governed-node reads they and their callers rely on.

use super::scope::{compile_scope_glob, normalize_scope_values};
use super::*;

impl LodestarStore {
    /// Active claims whose declared scope intersects a requested pre-flight
    /// scope. Advisory only: no state is changed and no lock is granted.
    ///
    /// `requester` is the asking agent id, used only to read the branch that
    /// agent already declared at `open_session`. The branch is never taken as a
    /// call argument: it is declared once per session (ADR-0035 decision 2), and
    /// a second place to state it would let a caller check against a branch it
    /// is not on, with nothing able to tell which one was true.
    pub fn check_claim_overlap(
        &self,
        requested: &TaskScope,
        exclude_task_id: Option<&str>,
        requester: Option<&str>,
        now: i64,
    ) -> Result<ClaimOverlapReport> {
        let requested = normalize_scope_values(requested);
        let requester_branch = self.declared_branch(requester)?;
        let mut statement = self.conn.prepare(
            "SELECT id, owner, lease_expires_at
             FROM tasks
             WHERE status = 'claimed' AND lease_expires_at >= ?1
               AND (?2 IS NULL OR id != ?2)
             ORDER BY created_at ASC, id ASC",
        )?;
        let claims = statement.query_map(params![now, exclude_task_id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
            ))
        })?;
        let mut overlaps = Vec::new();
        for claim in claims {
            let (task_id, owner, lease_expires_at) = claim?;
            let scope = task_scope_on(&self.conn, &task_id)?;
            let matching_paths = intersect_paths(&requested.paths, &scope.paths)?;
            let matching_symbols = requested
                .symbols
                .iter()
                .filter(|symbol| scope.symbols.contains(symbol))
                .cloned()
                .collect::<Vec<_>>();
            if !matching_paths.is_empty() || !matching_symbols.is_empty() {
                let owner_branch = self.declared_branch(Some(&owner))?;
                let signal =
                    OverlapSignal::classify(requester_branch.as_deref(), owner_branch.as_deref());
                overlaps.push(ClaimOverlap {
                    task_id,
                    owner,
                    lease_expires_at,
                    scope,
                    matching_paths,
                    matching_symbols,
                    owner_branch,
                    signal,
                });
            }
        }
        Ok(ClaimOverlapReport {
            requester_branch,
            claims: overlaps,
        })
    }

    /// The federated counterpart to [`check_claim_overlap`](Self::check_claim_overlap):
    /// reads active claims from `source` (a federated repository's Ackplane
    /// claim registry, ADR-0096 clause 5) instead of the local `tasks` table,
    /// then applies the identical scope-intersection and branch-signal logic
    /// so a caller sees one report shape regardless of coordination mode.
    pub fn check_federated_claim_overlap(
        &self,
        source: &dyn FederatedClaimSource,
        requested: &TaskScope,
        exclude_task_id: Option<&str>,
        requester: Option<&str>,
    ) -> Result<ClaimOverlapReport> {
        let requested = normalize_scope_values(requested);
        let requester_branch = self.declared_branch(requester)?;
        let claims = source.active_claims(exclude_task_id)?;
        let mut overlaps = Vec::new();
        for claim in claims {
            let scope = TaskScope {
                paths: claim.paths,
                symbols: claim.symbols,
            };
            let matching_paths = intersect_paths(&requested.paths, &scope.paths)?;
            let matching_symbols = requested
                .symbols
                .iter()
                .filter(|symbol| scope.symbols.contains(symbol))
                .cloned()
                .collect::<Vec<_>>();
            if !matching_paths.is_empty() || !matching_symbols.is_empty() {
                let signal = OverlapSignal::classify(
                    requester_branch.as_deref(),
                    claim.owner_branch.as_deref(),
                );
                overlaps.push(ClaimOverlap {
                    task_id: claim.task_id,
                    owner: claim.owner,
                    lease_expires_at: claim.lease_expires_at,
                    scope,
                    matching_paths,
                    matching_symbols,
                    owner_branch: claim.owner_branch,
                    signal,
                });
            }
        }
        Ok(ClaimOverlapReport {
            requester_branch,
            claims: overlaps,
        })
    }

    /// The branch an agent declared, treating an unregistered agent and a blank
    /// declaration alike: both are "said nothing", not "said empty".
    pub(crate) fn declared_branch(&self, agent: Option<&str>) -> Result<Option<String>> {
        let Some(agent) = agent.map(str::trim).filter(|agent| !agent.is_empty()) else {
            return Ok(None);
        };
        Ok(self
            .session_context(agent)?
            .and_then(|(context, _)| context.branch)
            .map(|branch| branch.trim().to_string())
            .filter(|branch| !branch.is_empty()))
    }

    /// Read one task's declared advisory scope.
    pub fn task_scope(&self, task_id: &str) -> Result<TaskScope> {
        if self.get_task(task_id)?.is_none() {
            return Err(LodestarError::NotFound(task_id.to_string()));
        }
        task_scope_on(&self.conn, task_id)
    }

    /// Active policy nodes covered by a task's declared advisory scope.
    pub(crate) fn governed_nodes_for_task_scope(&self, task_id: &str) -> Result<Vec<String>> {
        let scope = self.task_scope(task_id)?;
        let matchers = scope
            .paths
            .iter()
            .map(|path| compile_scope_glob(path))
            .collect::<Result<Vec<_>>>()?;
        let mut nodes = Vec::new();
        for node in self.governed_node_ids()? {
            let path_matches = node
                .strip_prefix("artifact:")
                .is_some_and(|path| matchers.iter().any(|matcher| matcher.is_match(path)));
            if path_matches || scope.symbols.contains(&node) {
                nodes.push(node);
            }
        }
        nodes.sort();
        nodes.dedup();
        Ok(nodes)
    }
}
