- **Three doc comments in the old `executive.rs` (now `tools/executive/claim.rs`)
  are interleaved across the wrong functions -- OBSERVED 2026-08-25, OPEN.**
  `lost_claim_reason`'s doc comment opens with "Attach any questions addressed
  to this agent to a response (ADR-0046)." -- `attach_owner_attention`'s actual
  purpose -- then switches mid-paragraph to "Why a compare-and-swap claim
  missed", which is what `lost_claim_reason` actually does. `attach_lease_warning`
  has the same problem in reverse: its doc comment opens with
  "`renew_lease` as the heartbeat -- rather than by a new obligation to poll"
  and "Absent when nothing is waiting: no key, no empty array..." (both
  `attach_owner_attention` material), then switches to "How close a lease may
  get to expiry before the agent is told" (its own, correct, subject).
  `attach_owner_attention` itself is left with no doc comment at all -- its
  real content is scattered across the other two.

  Impact: cosmetic only (doc comments, not behavior) but actively misleading to
  a reader trying to understand any of these three functions from their doc
  comment alone.

  Left for later rather than fixed here: this was found while splitting
  `executive.rs` into `tools/executive/{mod,constants,definitions,claim,tasks,render}.rs`
  (module-length ratchet), a "no behavior change" mechanical move. All three
  affected functions landed in the same new file (`claim.rs`), so preserving
  the bug verbatim kept the split a pure move with no unrelated content
  changes -- fixing three scrambled doc comments belongs in its own commit,
  not bundled into a file-structure refactor.
