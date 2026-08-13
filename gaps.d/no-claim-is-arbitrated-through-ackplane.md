- **No claim is arbitrated through Ackplane, so resolving `federated` would
  report an authority that is never exercised — and nothing tracks the adapter
  that would make it real. MEASURED 2026-08-14, left OPEN.** The pieces now look
  connectable, which is what makes this a trap rather than ordinary
  incompleteness.

  What is actually true on `origin/main` at `1551270`:

  | | |
  |---|---|
  | `NodeSyncService.Synchronize` | accepts every `Hello`; no enrolment or repository-authority lookup, so a handshake cannot distinguish `Ready` from `NotEnrolled` |
  | `mindleak-mcp`, `lodestar-mcp` | still hold their local Lodestar and MindLeak stores |
  | claims routed elsewhere | none — grepping the two MCP crates and `lodestar-core` for `arbitrat` returns one file, `lodestar-core/src/llm.rs`, unrelated |

  So a successful `federated` resolution would mean only that something answered
  on a socket.

  **Why this is a trap.** The transport answers, `CoordinationMode::Federated`
  exists, `ensure_supported` is one line from accepting it, and a client task
  sits on the board. An implementer who wires *handshake succeeded, therefore
  federated resolves* ships reachability presented as arbitration — the false
  authority ADR-0082 decision 3 refuses, and the second arbiter for one
  repository's claims that ADR-0045 exists to prevent. The trap gets more
  convincing as the transport matures, not less, because each landed piece makes
  the last step look smaller.

  **What is missing.** Two things. A server-side enrolment and authority
  contract, in flight under `task:c265276db1ba`. And a plane-to-Ackplane claim
  arbitration adapter, which no task tracks — the larger piece, and the one that
  makes `federated` *mean* something rather than merely resolve.

  Deliberately no design here. Where arbitration crosses the plane boundary —
  whether a claim is proxied, mirrored or leased, and what becomes of the local
  store while it is — belongs in a decision taken with ADR-0082 and ADR-0045
  open, not settled inside a gap fragment by whoever noticed the hole.

  Distinct from
  [`the-coordination-mode-is-declared-per-process-not-per-repository.md`](the-coordination-mode-is-declared-per-process-not-per-repository.md),
  which covers *where* the mode is declared and defers its own impact to "the
  moment an Ackplane client exists". This fragment is about what must be true
  before that client can mean anything.
