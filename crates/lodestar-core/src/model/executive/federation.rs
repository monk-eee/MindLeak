//! Ackplane federation seams: reading and arbitrating claims when a
//! repository's coordination mode is federated (ADR-0096).

use serde::{Deserialize, Serialize};

/// One active claim as reported by a federated repository's Ackplane claim
/// registry (ADR-0096 clause 5) — the federated counterpart to reading a row
/// from the local `tasks`/`task_scopes` tables.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FederatedClaim {
    pub task_id: String,
    pub owner: String,
    /// The branch Ackplane recorded for the owner, if any.
    pub owner_branch: Option<String>,
    pub lease_expires_at: i64,
    pub paths: Vec<String>,
    pub symbols: Vec<String>,
}

/// Where `check_federated_claim_overlap` reads active claims from when a
/// repository's coordination mode is federated (ADR-0096 clause 5).
///
/// A seam, not an implementation: `lodestar-core` stays local and
/// stdio-only (ADR-0004), so the concrete Ackplane RPC client lives outside
/// this crate. Tests inject a fixed in-memory implementation; a real one is
/// wired in by whichever binary composes this store with a live client.
pub trait FederatedClaimSource: Send + Sync {
    /// Every currently-active claim in the federated repository, excluding
    /// `exclude_task_id` if given. Ackplane decides what "active" means
    /// (whether a lease has actually expired); this seam does not filter or
    /// second-guess that answer.
    fn active_claims(&self, exclude_task_id: Option<&str>) -> crate::Result<Vec<FederatedClaim>>;
}

/// The authoritative claim state Ackplane granted for a `delegate`/`renew`/
/// `recover` request (ADR-0096 clause 3) — exactly the fields a
/// cache-projection copies into the local task row so `board`/`next`/`scope`
/// keep reading local, instant data afterward.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FederatedClaimGrant {
    pub owner: String,
    /// The branch Ackplane recorded for the owner, if any.
    pub branch: Option<String>,
    pub claim_started_at: i64,
    pub lease_expires_at: i64,
    pub claim_lapses: i64,
    pub paths: Vec<String>,
    pub symbols: Vec<String>,
}

/// One `delegate`/`renew`/`recover` round trip's outcome against Ackplane's
/// claim CAS.
///
/// Not the same distinction as `Result`: a rejection is Ackplane's arbiter
/// answering and refusing (someone else holds the task, a renew's owner
/// mismatches, ...), which is a normal CAS outcome exactly like a local
/// `claim_task_with_partial_scope` returning `Ok(false)`. A transport or
/// protocol failure — the arbiter did not answer at all — is not represented
/// here; it is the `Err` side of the `Result` this outcome is wrapped in, so
/// it can never be confused with a business refusal (ADR-0096 clause 3's
/// "actionable typed refusal").
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FederatedClaimOutcome {
    Granted(FederatedClaimGrant),
    Rejected { diagnostic: String },
}

/// Where Ackplane's claim CAS is asked to decide `claim`/`renew`/`release`/
/// `recover` for a federated repository (ADR-0096 clauses 2-4, 6) instead of
/// the local `tasks` table deciding them.
///
/// A seam, not an implementation, matching [`FederatedClaimSource`]: this
/// crate stays local and stdio-only (ADR-0004), so the concrete
/// authenticated Ackplane RPC client — and the blocking bridge a synchronous
/// call site needs for it — lives outside this crate. Tests inject a fixed
/// implementation; a real one is wired in by whichever binary composes this
/// store with a live client.
pub trait FederatedClaimAuthority: Send + Sync {
    /// `paths`/`symbols` are the full scope to request, already resolved by
    /// the caller from any partial declaration — the wire contract has no
    /// "leave scope alone" value, unlike the local partial-scope call.
    fn delegate(
        &self,
        task_id: &str,
        owner: &str,
        branch: Option<&str>,
        lease_secs: i64,
        paths: &[String],
        symbols: &[String],
    ) -> crate::Result<FederatedClaimOutcome>;

    fn renew(
        &self,
        task_id: &str,
        owner: &str,
        lease_secs: i64,
    ) -> crate::Result<FederatedClaimOutcome>;

    /// `true` iff a live lease was actually holed. Ackplane holes the lease
    /// rather than deleting the row (ADR-0096 clause 6), so a release of an
    /// already-expired or foreign claim is a no-op, exactly like the local
    /// `release_task`.
    fn release(&self, task_id: &str, owner: &str) -> crate::Result<bool>;

    fn recover(
        &self,
        request: &FederatedClaimRecoverRequest,
    ) -> crate::Result<FederatedClaimOutcome>;
}

/// Everything [`FederatedClaimAuthority::recover`] needs, bundled to keep the
/// method's own argument count sane (mirrors `ackplane-server`'s
/// `ClaimRecoverRequest` doing the same for the wire-level equivalent).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FederatedClaimRecoverRequest {
    pub task_id: String,
    pub expected_owner: String,
    pub owner: String,
    pub branch: Option<String>,
    pub reason: String,
    pub lease_secs: i64,
    pub paths: Vec<String>,
    pub symbols: Vec<String>,
}
