# Changelog

All notable changes to MindLeak are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and this project adheres
to [Semantic Versioning](https://semver.org/).

## [Unreleased]

## [0.1.4] - 2026-07-30

### Added
- Build-artefact hygiene now runs itself. A fleet of worktrees rebuilds the same
  crates endlessly and nothing ever removed the result: a measured 149 GiB across
  124 cache directories, on clean branches already merged into `main`. Cleanup
  that depends on someone remembering does not happen, because the agent that
  filled a worktree has finished and moved on before it is safe to empty it. So
  the sweep has no schedule of its own — it rides on the delivery watcher
  (`make queue-watch`), which is already persistent and already single-owner:
  once at startup, then on a bounded cadence, with the last run and a lock both
  held in the common Git directory so two worktrees can never sweep at once.
  `make sweep` and `node scripts/artefact-sweep.mjs` report the same plan for
  diagnosis, and `--apply` acts. Safety is the contract: it removes only
  reproducible build output, never a worktree, source, Git state, `target/tmp`,
  telemetry, completion offers, release assets, or the bare host's
  `target/release`, which serves the running MCP binaries. It skips any worktree
  that is detached, dirty, unmerged, backing an open pull request, or active
  within the grace period, re-checks every one of those immediately before
  deleting so a plan that went stale while the disk was walked is abandoned
  rather than acted on, and counts every skip with its reason in the report.
- **Shell-specific plumbing is now refused at the commit.** The project already
  required platform-agnostic operation, but that rule is stated as an outcome,
  so it was only ever noticed after something had broken on someone else's
  machine — and in practice it was broken repeatedly by the agents who had just
  read it. The `no-shell-plumbing` hook checks the plumbing itself: a
  documentation fence tagged `powershell`/`pwsh`/`cmd`/`bat` is a command the
  reader on another OS cannot run, and an inline interpreter one-liner
  (`node -e`, `python -c`, `powershell -Command`, `cmd /c`) embeds a program
  inside shell quoting that every shell quotes differently — the same line that
  works in one mangles its input in another, silently, surfacing only when
  someone reads the file it wrote. Deliberately narrow so it stays quiet on
  legitimate usage: ```bash fences and ordinary interpreter invocations pass.
  It is a ratchet, not a backlog — the tracked tree is already clean, and a test
  asserts that it stays so.
- **A worktree now refuses a second writer.** Worktree isolation assumed a
  linked worktree belonged to whoever was standing in it — git isolates files,
  the index, and branch selection, but not *who may type*. So nothing stopped an
  agent committing inside a peer's checkout, which is exactly what happened:
  a commit landed in a branch its author did not own, mid-merge, corrupting
  files there. The failure surfaced in the *other* agent's branch, naming files
  the intruder never touched, which is what made it expensive rather than merely
  wrong. A linked worktree now records the session that first commits in it, and
  refuses any other session, both in `scripts/scoped-commit.mjs` and in a new
  `worktree-owner` pre-commit hook that covers every commit path. The marker
  lives in the per-worktree git dir, so it is never committed and never collides
  between worktrees. A deliberate handover is still possible with
  `--adopt-worktree`; an accidental one is not. Verified in both directions: the
  previous script let the intruder commit land, the current one exits 4 and
  leaves the branch untouched.
- **Five fleet-discipline clauses adopted into the constitution**
  (`mindleak-fleet-discipline@1`): worktree ownership, claim-before-first-commit,
  provenance recorded at commit time, a lapsed claim being a human matter, and
  no shell-specific plumbing in committed instructions. Each is drawn from a
  measured incident rather than from principle. Two are backed by real
  mechanisms and reach their declared consequence — `control:worktree-owner`
  (mechanical, ceiling `block`) and `control:ingest-commit` (observed, ceiling
  `review`); the other three resolve at advise until a mechanism exists, which
  is the honest reading of a rule nothing enforces.
- **An agent may hold at most three claims at once (ADR-0067).** Measured with
  six agents running: **36 tasks `claimed`, 4 with a live lease** — the rest
  lapsed a median of 13 hours earlier, with two agents holding 15 and 14 apiece.
  The board therefore read as a fleet with 36 things in flight when it had 4,
  and establishing that took a bespoke script. `claim_task` now refuses a claim
  that would take an agent past the limit, naming the tasks it already holds and
  what to do with them. Lapsed claims count: letting a claim go stale is not
  finishing it, and a cap on live leases alone would make going stale the
  cheapest way to dodge the limit. Re-claiming a task you already hold is never a
  new claim, so the ADR-0052 heartbeat and the ADR-0048 window-preserving
  re-claim are untouched. `board` rows also carry a derived `lease_state`
  (`live`/`lapsed`), so a claim nobody is holding never again reads as work in
  progress.
  Deliberately *not* done: releasing claims on lapse. A lapsed claim is already
  claimable by anyone — `claim_task`, `next_task` and `stalled_work` all handle
  it — so a sweep would fix nothing, and `release_task` nulls `claim_started_at`,
  which would destroy the evidence window ADR-0048 exists to preserve.
- **A control can now be stood down through the tool surface.**
  `register_control` and `register_ratchet` were exposed and `retire_control`
  was not, so a control registered under the wrong id was permanent: its
  version can never move backwards, which means re-registering the id is
  refused, and there was no supported way to withdraw it. Dead and duplicate
  mechanisms accumulated against live clauses and went on reporting.
  `retire_control` is now an MCP tool. Retirement is deliberately not deletion —
  the control keeps recording what it once enforced, so an observation naming it
  resolves as `unknown` rather than quietly disappearing, which is the honest
  answer to "this measurement came from a mechanism we have since stood down".
- **A link checker now guards the living documentation, and the one dead link
  it found is fixed.**
  `scripts/link-check.mjs` validates every relative markdown link in the living
  docs (README, AGENTS, DEVELOPERS, `docs/*.md`, the extension README) against
  the working tree, and its test runs from pre-push via `script-tests`, so a doc
  that starts pointing at a moved, renamed, or deleted file fails the push
  instead of rotting unnoticed. It resolves a target file-relative or
  root-relative (the repo mixes both), treats a directory target as valid, and
  exempts the `media/screenshots/` images the capture checklist tracks. It found
  AGENTS.md still pointing `GraphStore` at `graph.rs` after that module was split
  into `graph/`; the link now points at `graph/mod.rs`. `docs/adr/` is out of
  scope on purpose — an ADR's cross-references are historical, number-identified,
  and some point at decisions since renamed or never given their own file;
  repairing those is a maintainer's call about intent, tracked separately.
- `merge_evidence` builds a conformance evidence bundle from a merge that
  already landed (ADR-0058), instead of the agent assembling one by hand.
  Name the commit that carried the work; the plane verifies deterministically
  that git can resolve it, that it is reachable from `main`, and that it touched
  paths inside the task's declared scope, then derives the bundle from what git
  reports. It refuses a commit that never merged, one outside the task's scope,
  one the calling agent does not hold the task for, and a task that declared no
  scope at all — with nothing to match on, any merged commit in the repository
  would otherwise serve as a receipt.
  It does not complete the task: conformance still judges the result and
  somebody still has to submit it.
- A decider label that is one edit from one already in the ledger is now flagged
  at the moment it is recorded. Attribution labels are free text and
  deliberately unverified — ADR-0071 is explicit that they are attributed, not
  authenticated — so a typo cannot be detected by checking it against anything.
  It can only be compared with what is already there.
  This matters because the moment of writing is the *only* moment a slip is
  fixable. Afterwards every verb that could correct one refuses by design:
  `attribute` answers "a recorded human act is not rewritten here" and `reopen`
  answers "a recorded decision is not undone here". Both refusals are right —
  an agent that could rewrite who decided something would make attribution
  worthless — but together they mean a mistyped name is permanent.
  Measured on the live ledger: 73 rows carried 70 decisions by `monk-eee`, one
  by `Lyndon Swan`, and one by `monk-ee`, which is a typo for the first and can
  never be corrected. This is what would have caught it.
  The check is advisory and never refuses. Two people can legitimately have
  similar names, and rejecting a genuine new reviewer to catch a typo is the
  worse failure; the response carries the recorded label, what it resembles, and
  what to do about it, and the decision itself proceeds untouched.
- **Policy can grow: a new clause can be written into an amendment.** Two
  correct rules met in a corner. `define_goal` states a rule that is live the
  moment it is written, and `complete_clause_contract` refuses to give a live
  rule a contract, because hardening what people are already working under is
  precisely what an amendment is for. But `propose_amendment` only carried
  existing clauses forward and nothing could add one — so the clause that most
  needed an enforcement contract was the one clause that could never be given
  one, and belonging to no constitutional version it never appeared in
  `constitution_diff` either. The only route into a version was
  `register_policy_pack`, which records immutable *upstream* provenance: minting
  a pack to carry a rule this project wrote itself would have put a fabricated
  source in the record. Measured impact — this blocked registering a ratchet
  over the MCP tool surface, because `register_ratchet` needs an active clause
  that authorises it and none of the 25 clauses mentioned the tool surface.
  `draft_clause` authors a clause into an open draft: it enters as part of the
  draft rather than as live policy, reads as `added` in the diff a reviewer
  sees, and carries the same id shape as a clause copied forward, so nothing
  downstream can tell an authored clause from an inherited one once promoted.
- A task now records the branch its evidence window is being done on (ADR-0057).
  The value is joined at claim time from what the claiming session already
  declared to `open_session`, so nobody is asked to declare anything twice, and
  a session that declared no branch records nothing rather than a guess — the
  server never inspects Git (ADR-0044). It follows the window rather than the
  agent: a same-owner re-claim keeps it, so an agent that has since moved on
  cannot silently rename the branch its earlier commits were made on, while a
  claim by a different owner opens a fresh window and re-reads it. Existing
  databases gain the column as NULL, which is the honest record of a branch that
  was never captured. This is the fact a verified merge will be checked against
  (ADR-0058).
- **ADR-0064 records that the task lifecycle becomes an append-only log, with
  `tasks` as its projection.** The schema had already improvised this primitive
  three times: `claim_lapses` and `unleased_seconds` are aggregates of events
  nobody wrote down, `task_claim_transfers` is a single-verb log with a
  hand-written before-image, and `conformance` is append-only already. The cost
  showed up on 2026-07-29 — diagnosing board growth across 220 tasks produced a
  **wrong** first answer, reading 29 expired-lease tasks as abandoned when four
  agents were actively working them; a sweep on that reading would have stripped
  live work from all four. The same gap makes ADR-0048's `needs_human` cap fire
  on healthy work, because two integers cannot tell "lapsed while idle" from
  "lapsed mid-build with commits landing in the hole", and the 300-second default
  lease is shorter than `cargo test --all`.
  Decision only; no behaviour changes in this commit. Per ADR-0063 the migration
  never rebuilds `tasks` destructively — live claims are not ours to touch — and
  imports each existing task as a genesis event that honestly declares it carries
  no prior history. Verdict recomputation and forking are explicitly deferred,
  and this does **not** shrink the board: that growth is agent fan-out (69
  created against 36 closed in a day), which the log makes legible, not smaller.
- **`make reingest` lets an extractor improvement reach the graph that already
  exists.** Structural extraction happens once, at ingest time, and nothing
  revisited it: `reconcile_workspace` only forgets files that vanished, `index`
  only fills embeddings, and the editor sensor re-ingests a file only when
  somebody saves it. So when the extractor learned Rust `mod`/`use` edges, the
  3,703 artifact nodes already in the graph did not learn anything — each would
  have caught up only on its next save, silently, over months.
  Measured 2026-07-29, immediately after Rust import extraction shipped:
  `get_impact_radius` on `crates/mindleak-core/src/model.rs`, which nearly every
  module in the crate imports, returned 11 nodes, 11 edges and **zero** imports
  edges. The improvement was real and completely invisible. After one pass:

  | `model.rs` impact | before | after |
  |---|--:|--:|
  | nodes | 11 | 189 |
  | edges | 11 | 216 |
  | `imports` edges | 0 | 41 |
  | dependent `.rs` files reached | 0 | 25 |

  The pass enumerates tracked files with `git ls-files`, skips what the
  extractor cannot read, and drives `ingest_file` through a server it builds and
  spawns itself — deliberately not whichever server an editor is running, since
  a rebuilt binary does not change an already-running process, and that stale
  process is exactly what the pass exists to get past. Re-ingesting is safe by
  construction: `replace_structure` atomically replaces everything an artifact
  emitted.
  The cost is stated rather than hidden: re-asserting a structural edge resets
  its decay clock, so the structural tier reads as uniformly fresh afterwards.
  That is defensible for structure, which is true exactly as long as the file
  says so, and attention (`observed`) edges are not written by this pass.
  The first run also surfaced that 43 of 247 tracked files cannot be re-ingested
  at all, because an absolute id from a sibling worktree owns their structural
  edges. Recorded in the Known gaps of `DEVELOPERS.md`; it is an ownership
  decision rather than a patch.
- `scripts/binding-audit.mjs` reports Lodestar goal/code binding coverage: source
  files no goal binds, bindings naming a path that no longer exists, and
  bindings stranded on superseded goals. `--check` exits non-zero on the first
  two, so it can gate CI. Cross-platform, read-only, no model.
- **`make board-health` separates work a human must decide from work nobody
  can.** ADR-0058 decision 4 says the board should report what it cannot close;
  this is that report. `needs_human` was one verdict covering two unrelated
  situations — conformance found something arguable, or the evidence bundle was
  empty and there is nothing to rule on at all. It also names stranded claims:
  a lapsed lease still holding scope against other agents (ADR-0048).
  Measured on this repository, 207 tasks: **0 decidable, 0 unresolvable, 27
  stranded**. The first draft of this report said 51 parked instead of 0,
  because a task keeps its conformance audits after it finishes and classifying
  by "latest audit" alone counted completed work as pending — every one of
  those 51 was already `done` or `abandoned`. Inflating a backlog is not a
  milder failure than hiding one; it sends people looking for work that does
  not exist. Terminal tasks are now excluded, with a test.
  Reporting only: nothing here closes, abandons, or reassigns anything, because
  ADR-0058 decision 5 is explicit that nothing closes automatically.
- **Known gaps now records that an agent can work all day and certify nothing.**
  48 of 101 `done` tasks rest on a `needs_human` receipt rather than an affirmed
  one, thirty-three claims sit lapsed, and an audit against `origin/main` found
  at least nine of those tasks already fully implemented in main — so the board
  cannot distinguish unfinished work from unclosed work, and an agent that
  trusts it re-implements what already shipped. The entry names the measurement,
  the impact, why `check_conformance`'s refusal is correct and must not be
  loosened, and the three candidate repairs. It also corrects the older
  "a lapsed claim can never certify the work it was claimed for" entry, which
  ADR-0048 has since made untrue: a same-owner re-claim keeps `claim_started_at`
  and records the hole, verified end to end on a task whose lease lapsed twice.
- **Publishing offers the task's exact completion evidence and check (ADR-0065).** `canonical-push` now uses the one moment the claim is guaranteed live and the published commits are already ingested to assemble `evidence_for`, run `check_conformance`, and write the exact `{ task_id, evidence, check }` payload under ignored `target/completion-offers/`. It prints one bounded instruction for the explicit `complete_task` call; it never calls `complete_task` itself. Ignoring the offer is the entire decline path, and every offer-side failure is silent because the push has already succeeded. Multiple live claims are left unoffered rather than guessing which task the branch served.
- **Delivery has a queue again, and we run it ourselves.** ADR-0061 chose
  GitHub's merge queue; the `merge_queue` ruleset rule is refused on this
  repository, because merge queue requires an organisation-owned repo and this
  one belongs to a user account. The same endpoint accepts other rules with the
  same credentials, so it is the feature that is unavailable, not the request.
  `make queue` (ADR-0062) serialises the step that was actually contended:
  with eleven armed pull requests and up-to-dateness required, every merge makes
  the other ten stale, and each one that refreshes itself burns a full check run
  against a `main` the next merge invalidates — O(N²) runs that never drain. The
  queue brings exactly one branch up to date at a time, in the order they were
  armed, which makes it O(N). It **never merges**: merging stays with GitHub's
  auto-merge behind the same five required checks, so it cannot become a second
  route into `main` that branch protection does not govern. `make queue-watch`
  runs it as an agent.
- **Work that finished and is waiting on a person now says so where the human's
  agent already looks.** A `drift` or `needs_human` verdict completes a task
  into `in_review` rather than `done` — the honest outcome, and by design
  (ADR-0009). Only a person can finish it. But nothing told anyone: completing
  into `in_review` clears the owner, and a human has no agent id (ADR-0046), so
  there was no agent to notify and no queue to read. Measured on 2026-07-30,
  five tasks were sitting finished from at least three sessions, three of them
  more than a day old, surfaced nowhere but a board query somebody had to think
  to run.
  `open_session` now carries `awaiting_a_human` alongside the `stale_build`,
  `waiting_on_you` and `paused_by_you` it already reports. The agent is told
  because the agent is the only thing the human talks to.
  It is a **filter over `stalled_work`'s existing `awaiting_human` rule**, not a
  second query. Deriving "waiting on a person" twice would let the two surfaces
  disagree about what that means, and the one that drifted would be the one
  nobody tested. The query lives on the facade rather than in the response, so
  the fact is available to any caller.
  Read-only and advisory: it reports and can never refuse. It says **nothing**
  when the queue is empty, because a field that always appears is one readers
  learn to scroll past — the same reason `stale_build` stays quiet on a current
  build. Both the reporting and the silence are tested.
### Added

- **`existing_work` answers whether this has already been done.** Six identical
  "carry controls across an amendment" tasks and four identical "run the merge
  queue ourselves" tasks reached the board because nothing could answer that
  question: `check_overlap` reports who is touching a file *right now*, and
  `board` hides finished work — so completed and abandoned work, the answers
  that matter most, were invisible. `existing_work(goal_id | paths)` returns
  the tasks already serving a goal or already declaring those paths in their
  scope, terminal states included. Path matching reuses `check_overlap`'s glob
  comparison so the two cannot drift, and asking about nothing is refused
  rather than answered "nothing exists" — a clean bill of health for a question
  never asked is the failure this exists to prevent.

  `create_task` now names the prior work serving the same goal and still
  creates the task: a second task against one goal is often legitimate, and a
  gate here would be wrong more often than right (ADR-0015).

  Not yet answered: which branch that prior work is on, and whether it is
  merged. `Task` has no branch field — that is a separate open task, and
  reporting a branch before one is recorded would be a guess.
- **A lease about to die, or already dead, says so.** A lapse was silent until
  `complete_task`, which is far too late: closing a lapsed claim means
  re-claiming it, re-claiming records the lapse, and conformance then refuses to
  certify across the hole (ADR-0048), so the only warning arrived after the cost
  had become unrecoverable. Twenty-nine claims on this repository are stuck
  behind exactly that. `complete_task` now reports a claim within ninety seconds
  of expiry — the default lease is five minutes and `cargo test --all` alone can
  outlast it — and separately reports one that has already lapsed, with
  different advice, because renewing cannot repair a window that already has a
  hole in it. A comfortable lease says nothing at all: a warning on every call
  is a warning nobody reads.
- **ADR-0065 proposes that completion belongs at the publication boundary.**
  Everything in this project that relies on remembering has failed — ADR-0046
  measured zero adoption for a capability needing its own call — and everything
  hung off an action already being taken has held, from the publication ledger
  to the delivery queue. Completion is the last obligation still waiting to be
  remembered, and the cost of forgetting it is unrecoverable rather than merely
  untidy. `Proposed`: it gives publishing a second meaning, which is a real cost
  and deserves argument.
- **Telemetry now says whether each registered agent session read memory before
  its first attributed write.** `telemetry_snapshot` reports a bounded list of
  the 32 most-recent sessions with successful memory-read and write counts plus
  `yes`, `no`, or `unknown` when no write exists yet. The metric is derived from
  the existing append-only audit trail and scans at most 10,000 recent
  attributed events; no stored verdict or new MCP tool was added. Identity comes
  only from `SessionRegistry`: callers may still use session-less read tools,
  but cannot forge `resolved_agent` to improve the result. Failed reads do not
  count, and opening the same session again starts a fresh observation window.
  This turns memory adoption into an outcome metric rather than treating raw
  recall call volume as proof of a habit.
- **Module length is now measured by a committed script, so the number the
  constitution ratchets against can be reproduced by anyone.**
  `scripts/measure-module-length.mjs` counts Rust source modules under
  `crates/` whose non-test length exceeds 450 lines — measuring above the
  colocated test block, so a well-tested module is not mistaken for a bloated
  one, and excluding integration suites and `tests.rs` modules outright,
  because splitting those pays nothing. The count is deliberately an advisory
  signal rather than a verdict: the governing clause says file length is
  "resolved by human judgment", and a genuinely cohesive module may sit above
  the line. What the bound ratchet prevents is the count drifting upward
  unnoticed — a module crossing the threshold surfaces at review, where it is
  either split or a new baseline is accepted, and an accepted baseline is
  attributed and version-bumped, which is how the cohesion exception ends up
  stated and justified instead of forgotten.
- **Paused work now finds its owner or an accountable successor (ADR-0070).**
  `open_session`, `claim_task` and `renew_lease` return `paused_by_you` with the
  task, parked time and exact pause reason, plus the `resume_task` action; empty
  reminders are omitted. A paused task whose owner is known gone may now be
  transferred before the seven-day grace through the existing `recover_claim`
  path when a distinct human reviewer, expected owner and reason are supplied.
  The reviewer and reason are recorded in the task event/thread history and the
  successor starts a fresh evidence window. Agent-only takeover, `needs_input`
  recovery and the ordinary grace-based fallback are unchanged. The reviewer
  label is explicitly an attributable declaration, not authentication.
- **Policy pack authoring and upgrading is documented (SPEC-CONSTITUTION §6,
  §12.1 task 6).** `docs/POLICY-PACKS.md` covers the pack and clause schema,
  namespacing and why a clause without a scope, evidence contract and
  consequence is deliberately review-only, the adoption sequence, and the
  upgrade path. The organising rule is stated first because everything else
  follows from it: composition happens at proposal time, never through live
  inheritance, so an adopted clause is copied into the project with its source
  pack id, version, digest and key, and re-publishing upstream cannot change
  local law. The guide ends by naming the tests that enforce each limit rather
  than asserting good behaviour, so a reader can check the claims instead of
  trusting them.
- **PR effectiveness telemetry is now reproducible instead of a one-off
  analysis.** `node scripts/evaluate-pr-effectiveness.mjs --limit=50` joins a
  bounded GitHub cohort to Lodestar tasks through branch, durable thread, and
  evidence-commit provenance, then reports conformance coverage/causes, claim
  timing, human resolution, reconciliation churn, required-check completeness,
  runtime latency/errors, polling share, and memory-read-before-write adoption.
  Timestamped JSON and Markdown land under `target/telemetry`; deterministic
  controls keep missing checks and incomplete attribution visible. Reports
  contain no prompts, secrets, source, model reasoning, raw task threads, or raw
  conformance evidence.
- **`scripts/mcp-build-probe.mjs` asks each MCP build what it writes, instead of
  guessing from dates or git distance.** A stale server that still writes
  absolute node ids is not detectable from either. Measured across ten
  worktrees: two that were only **five commits behind `main`** wrote absolute
  ids, while others 17 and 38 behind wrote correct ones — and every worktree was
  behind on `crates/`, so any threshold-based warning would have fired on all
  ten. A warning that always fires is one people learn to skip, which is how the
  original defect survived three days; that design was measured, rejected, and
  is recorded here so it is not proposed again.
  What separates them is behaviour. Node ids are repo-relative by contract
  (ADR-0038), so the probe hands each binary one file by absolute path against a
  throwaway database and reads the id it produces. No heuristics, no false
  positives, and no live data touched. On first run it found **6 of 15 builds**
  still writing absolute ids — the same six a manual sweep had found, in one
  command. `--check` exits non-zero for CI or a pre-flight before trusting a
  fleet-wide result.
- **The delivery queue now names the open work it is not managing.** Arming a
  pull request is what puts it in the queue (ADR-0045), so an unarmed one is not
  last in line — it is not in the line at all. The tick reported only the armed
  entries, which made "nothing is waiting" and "three pull requests are waiting
  and nobody armed them" print identically. Measured on the day this landed:
  three of five open pull requests were invisible to the queue, and no change to
  the ordering could have reached them, because ordering only ever applies to
  what is in the queue. Each unmanaged pull request is now listed with its merge
  state and the reason — `not queued: nobody armed it`. This is reporting, not
  policy: arming still decides membership and the first-in-first-out order by
  arming time is unchanged.
- **Rust files now declare what they import, so impact can say what breaks.**
  The impact traversal is the deterministic half of MindLeak's memory — the part
  that answers "what depends on the file I am about to change" — and for Rust it
  had nothing to work with. Measured on this repository, the impact of a real
  `.rs` file (`crates/mindleak-core/src/facade/query.rs`) was 15 nodes over 15
  edges: its own commits, its own symbols, and not one other file, because Rust
  ingestion emitted no inter-file edges at all. Meanwhile `docs/EVALUATION.md`
  reported 1.00 precision on the impact question — measured on a **JS/TS**
  fixture where those edges exist. The benchmark and the experience were both
  honest and described different languages.
  Rust ingestion now recovers the module graph without compiling anything and
  without spending a token: `mod x;` resolves to the declaring module's
  directory (which for a non-root file is a directory named after the file, not
  the file's own directory — getting that backwards silently points every child
  module somewhere wrong), and `use crate::`/`self::`/`super::` resolve through
  a longest-first candidate ladder the store picks a known file from. That
  ladder is the same mechanism the JavaScript arm already used, reused rather
  than reinvented, because a `use` path cannot be split into module part and
  item part by looking at it: `crate::graph::GraphStore` and
  `crate::graph::query` are the same shape.
  Deliberately conservative where certainty runs out. Another workspace crate
  records as `package:<name>` rather than a guessed `crates/<name>/src/lib.rs`,
  because that mapping is a convention this code cannot verify; an inline
  `mod x { .. }` produces nothing, because no file is behind it; and comments
  and string literals are masked before parsing, so the prose in this
  repository's doc comments cannot fabricate an edge. All of these under-report,
  which is the safe direction — a missing edge is a smaller lie than an invented
  one.
  This is the follow-up ADR-0066 named: the pre-flight was put on the mandatory
  checklist with this limit stated rather than left unused, and closing it meant
  emitting the edges, not rewording the docs. `impact_radius` and the
  `check_overlap` pre-flight both return a Rust dependent end to end.
- **`silent-knowledge` reports the recorded lessons that can never be read.**
  The conformance advisory matches recorded knowledge on referenced nodes and
  nothing else, so a record whose evidence carries no `nodes` array is stored,
  counted, decayed on schedule, and can reach nobody. Nothing measured that.
  `active_knowledge` reports `surfaces` per record, but only for whatever
  filter you happened to ask for — reading it as a repository-wide number
  required already suspecting the problem and then constructing the query,
  which is why an early spot check of a filtered subset read as "3 of 17" and
  the real figure went unnoticed. Measured across the whole ledger: **65 of 153
  records, 42%, cannot be read.** Among them are the lessons most worth having
  — that testing a facade method proves the logic and says nothing about the
  wiring, which is precisely how `merge_evidence` shipped refusing every
  caller; that a guard asserting over a retired name silently stops guarding;
  and one recording the cost of skipping the mandatory ADR-0029 pre-flight,
  which is exactly the mistake it could not warn anybody about. The audit ranks
  by weight and confirmation so the list is workable rather than a heap, takes
  `--top N`, and `--check` exits non-zero for a hook or a pipeline. It only
  reports: knowledge is append-only and nothing attaches nodes retrospectively,
  so the repair is to re-record the content with an evidence `nodes` array
  after re-verifying that it is still true. Copying a stale claim forward would
  be worse than leaving it silent, which is why this does not attempt to do it
  automatically.
- **`make stranded-report` turns a lapsed claim into a judgement rather than an
  investigation, and `board-health` stops implying an agent could close one.**
  A lapsed claim cannot be closed by an agent, and the reason is structural
  rather than a gap: closing one means re-claiming it, and re-claiming after a
  lapse records the lapse, whereupon conformance returns `needs_human` for a
  discontinuous evidence window and refuses to certify across the hole.
  Narrowing the window around the gap is exactly the laundering ADR-0048 exists
  to stop, so the refusal is the guarantee working. Measured while trying: a
  task showing `0 lapse(s)` reported `the lease lapsed 1 time(s), leaving
  85730s unleased` the moment it was claimed in order to close it. Calling them
  "stranded claims" invited precisely the response that cannot work, so the
  report now says `awaiting confirmation` and names who can act. The new report
  proposes the commit that most likely shipped each one, graded strong / likely
  / weak / none — a close second downgrades the confidence, because a coin toss
  presented as a finding turns a judgement into a rubber stamp.
- **The task lifecycle now has an append-only log (`task_events`), seeded with
  the present.** ADR-0064's first step: the table, the `TaskEvent` model, and a
  once-per-database genesis import. Each event carries the full after-image of
  the task the transition produced, so replaying the log is a deterministic
  assignment rather than a re-derivation that could drift from the guarded
  UPDATE it mirrors. `append` takes a connection rather than `&self`
  specifically so callers pass the transaction already open for the state
  write — an event committed separately from the row it describes could exist
  without it, and the projection would stop being checkable.
  Two deliberate choices. There is **no** foreign key to `tasks`:
  `task_claim_transfers` cascades on delete, which is right for an audit of a
  row that must exist, but a record of what happened must outlive its subject
  rather than vanish with it. And the genesis import writes state only, with no
  invented history before it — the claims and lapses that produced each current
  row were never recorded, and manufacturing plausible ones would put fiction
  in an audit ledger. Per ADR-0063 it is registered by name in
  `schema_migrations` and touches no task row, so no live claim moves.
  Nothing emits events yet; the write path is unchanged.
- **The board now reports the claims it cannot close.** Work that shipped and
  never closed stays on the board indefinitely, and a board that understates what
  is finished is expensive in a way an overstated one is not: `next_task` offers
  work that already exists and an agent rebuilds it. Observed repeatedly on this
  repository — a task was offered whose branch was sitting in an open pull
  request, and four separate open tasks turned out to be already delivered, each
  costing a fresh investigation to discover.
  `make board-health` now names any non-terminal task whose recorded branch has
  merged into `main`, with the merge commit, so a person can check it in seconds.
  Branches are read from merge subjects rather than `git branch --merged`,
  because a branch is usually deleted the moment it merges — the ref is gone
  while the history proving it landed is not.
  It reports and never closes. Completing one of these would manufacture a
  receipt for work the script did not witness, which ADR-0009 refuses.
  The count distinguishes **`unknown` from `0`**, which matters more than the
  feature: a task claimed before the branch column existed records none, and a
  server built before it does not return the column at all. Both produce an empty
  result that reads as "nothing shipped unclosed" while actually meaning "nothing
  to check against". The first live run produced exactly that false zero. A bare
  count there would have been the same falsely-reassuring signal this report
  exists to remove.
- **The claim decision now surfaces the branch, on both the `claim_task`
  response and the VS Code board row (ADR-0035 decision 5).**
  A won claim confirms the branch its evidence window was pinned to; a lost
  claim names not just who holds the task (`owner`) but the branch they hold it
  on (`owner_branch`) — the fact a colliding agent needs to tell a merge risk
  from the same work twice. The board row shows the owner's branch beside the
  owner (`alice on fleet/x`) and in its tooltip. Both come from what the owner
  declared to `open_session`, pinned to the task's window at claim time, and are
  `null`/omitted cleanly when no branch was declared — never guessed, because
  the server never inspects Git (ADR-0044).
- **The Lodestar tool surface is now tiered: the default profile is the common
  path, and the specialist machinery is advertised only when asked for.**
  Every agent loads `tools/list` before its first question, so an unspent
  minute of governance authoring — the constitution, amendments, policy packs,
  waivers, ratchets, the design board, database admin — was a tax paid in every
  session of every worktree (ADR-0059 rule 2). The default profile now
  advertises the seventeen tools an agent uses to find, claim, do, prove and
  hand off work, plus the ones it reads to know what governs it: 17 tools,
  ~4,513 tokens, down from 67 tools, ~13,757 tokens. Nothing became
  unreachable — dispatch is unchanged, so a specialist tool called by name
  still runs. Set `LODESTAR_TOOL_PROFILE=full` to advertise the whole surface.
  The allowlist is deliberate: a tool added anywhere else is specialist until
  someone puts its name on the common path, so the surface an agent pays for
  every session grows by decision rather than by default.
- The fleet can now reclaim its own disk. `scripts/worktree-reclaim.mjs` reports
  worktrees whose commits have landed on `origin/main` and, when told to,
  removes them along with their local branch, their merged remote branch, and
  their build output. `make reclaim` reports; `make reclaim ARGS="--reclaim
  --remote"` acts.
  This exists because cleanup never happens on goodwill. The agent that created
  a worktree has finished and moved on by the time it is safe to remove, so the
  mess is always somebody else's and it grows every time the fleet works
  correctly. Measured 2026-07-30: 88 worktrees, 86 carrying `target/`, 61
  carrying `node_modules`, one sampled `target/` holding 82,891 entries. On the
  first real run the tool found 22 reclaimable worktrees holding **62.32 GiB**
  of build output.
  Reporting is the default and acting is explicit, because the failure mode of a
  cleanup tool is deleting work somebody still needed and no report can be
  un-deleted. It refuses the bare primary, protected branches, any tree with
  uncommitted **or untracked** changes, any tree mid-build, any tree whose
  ownership marker names another session, and any branch whose commits have not
  landed. Every refusal names the rule that stopped it, so a worktree that is
  kept does not read like one the tool failed to notice.
  Landing is judged by patch equivalence (`git cherry`), not commit identity. A
  squash or rebase merge lands every line under a new commit id, so
  `git merge-base --is-ancestor` answers "no" for work that is fully merged —
  the mistake that previously led an agent here to declare 245 merged lines lost
  and queue a PR to restore code already on main.
  The decision for each worktree is a pure function of gathered facts, so all
  six refusals are tested without creating or destroying anything. The tests are
  weighted toward what the tool must *not* take, because a cleanup tool tested
  only on what it deletes has not been tested on what matters.
- **The module-length ratchet is now observed, not merely registered.**
  `control:rust-module-length` had a reviewed baseline and a committed measurer
  and nothing ever told it anything — the same shape as the six script suites
  and the merged-branch audit found earlier the same day: a mechanism that
  exists, works, and runs nowhere. `scripts/observe-module-length.mjs` measures
  the governed modules and reports the count through `observe_ratchet`, and it
  runs on every publication, because publication is when the work becomes
  visible to the fleet and therefore the honest moment to measure what the fleet
  now has to live with. It reports locally rather than in CI on purpose: the
  Intent Plane is a per-developer store, so an observation recorded on a
  throwaway runner is recorded nowhere. It never blocks a push — the clause
  resolves at `review` and the control's power is `observed`, so failing a push
  on a regression would enforce harder than the rule it serves (ADR-0034); a
  rising count is a question for a human, and cohesion still outranks size. What
  it does refuse is running blind: an unattributed session or an unreachable
  Intent Plane fails loudly, because a reporter that quietly says nothing is
  indistinguishable from one reporting a pass.
- **The recall ranking change is measured against a real index, including the
  part of it that did not work.** ADR-0075 shipped on deterministic unit tests
  whose fields were synthetic and uniform. A real index is neither, so it was
  measured against this repository's own — 19,317 embedded nodes, ten queries,
  the pre-change algorithm as the control arm and the built binary as the
  treatment arm.

  Two claims held. Hits naming a node the graph no longer holds fell from **24
  of 50 to 0 of 49**: nearly half of what recall used to hand back was an id the
  caller could not open. Recorded conclusions rose from **14% of hits served to
  96%**, where they had been outnumbered five to one by symbols, executions and
  dangling references.

  One did not, and it is recorded with equal weight because the fixtures could
  not see it: **a nonsense query is still answered rather than met with
  silence.** Top-hit distance above the field is 3.11–3.90 standard deviations
  for nonsense controls and 3.71–6.21 for real questions, so the bands overlap
  by 0.19σ and no single threshold rejects one while keeping the other. The
  shipped 1σ cut sits far below both. The reasoning that failed was that
  nonsense lifts a field uniformly — true of the fixture, false of a diverse
  19,000-node index, where even nonsense has relative outliers.

  The constant is deliberately **not** tuned in response: three samples
  separated by a negative margin is the same global constant the floor
  measurement already warned against, one level up. ADR-0075 is still Proposed
  and carries a correction saying so.

  New: `scripts/evaluate-recall.mjs`, with unit tests, reproducing all of the
  above. It needs a populated index and a reachable embeddings server — both
  optional parts of the product (ADR-0008) — and reports rather than fails when
  either is absent.
- **The advertised MCP tool surface is now a measured number, so growth can no
  longer pass unnoticed.**
  `scripts/measure-tool-surface.mjs` asks both servers for `tools/list` over
  MCP stdio and reports what a session pays to load them: 118 tools, 63.7 KB,
  roughly 16,316 tokens spent before the first question. It asks the servers
  rather than counting definitions in the Rust source, because the number that
  matters is what a client is actually served; the unit is the compact JSON
  that crosses the wire, and the token figure is bytes/4 and says so, since
  only the count is exact. A server it cannot reach fails the run instead of
  being left out — half a surface reported as the whole one reads as an
  improvement and is a missing build. Measuring cost is not judging worth, so
  the number is meant to be held by a ratchet reporting at review: whether a
  tool earns its place in the context window is a decision for a human, and
  what was missing was never the judgment but the prompt to make it. That
  ratchet is not yet registered — no active clause authorises one and a new
  clause cannot currently be given an enforcement contract (see Known gaps) —
  so for now the surface is measured and published rather than enforced. The
  ratchet is tracked separately as task:8000f45e0dfd. ADR-0059 recorded 89
  `lodestar-mcp` tools; the first run recorded 90, and the reconciled run
  recorded 91.
- **A task's evidence-window continuity is now derivable from the log.**
  `claim_window()` replays the recorded transitions to compute the lapses and
  unleased seconds that `tasks.claim_lapses` and `tasks.unleased_seconds`
  currently carry as running totals (ADR-0064 decisions 5 and 6). Nothing is
  removed yet: the derivation is asserted **against** those columns across
  every shape they take — never claimed, clean claim with renewals, one lapse,
  two lapses, handover to a new owner, and park/resume — because that agreement
  can only be proved while both still exist. After the columns go there is
  nothing left to disagree with.
  The genesis event now carries the counters it imported. Deriving continuity
  from in-log transitions alone would report zero lapses for any window that
  opened before the log did, and under ADR-0048 a window with no lapses may
  certify itself as `aligned` — so a migration would have quietly laundered a
  discontinuous window clean and handed out a receipt for work with holes in
  it. Derivation is therefore genesis seed plus in-log transitions, with a test
  that a pre-log window keeps the three lapses it had and accumulates a fourth
  on top.

### Changed
- Squash and rebase merging are now disabled on `monk-eee/MindLeak`, so the
  merge commit is the only button available. AGENTS.md has always asked for
  merge commits so that a commit id stays evidence, and ADR-0038 is explicit
  that squashing, rebasing and cherry-picking replace evidence-bearing commit
  identities — but nothing enforced it, so the rule depended on which button an
  agent clicked. Verified by reading the repository settings back rather than by
  intention: `allow_squash_merge` and `allow_rebase_merge` are both false and
  `allow_merge_commit` is true.
  This had already cost real time. PR #205 was armed with `--squash`, landing all
  245 lines under a new commit id; the merge audit compared ancestry, could not
  tell a squash from a branch that never merged, and reported the work as lost.
  Another agent confirmed that with `git merge-base --is-ancestor`, wrote it into
  durable knowledge as fact, and queued a pull request to restore code that was
  already on `main`.
  AGENTS.md now also states the test that distinguishes the two: `git cherry -v
  origin/main <branch>` compares patches, where `-` means an equivalent patch is
  already upstream and `+` means it never landed. Ancestry asks about commit
  identity and therefore answers "no" for work that is fully present. History
  still holds identities rewritten before the button was closed, so the
  distinction remains load-bearing even though new ones can no longer be
  created — `scripts/worktree-reclaim.mjs` depends on it to tell a merged
  worktree from an unmerged one.
  Checked before flipping: one pull request had auto-merge armed and it was
  armed with `MERGE`, so nothing in flight was broken by removing the other two
  methods.
- **`check_overlap` grades the collision instead of just reporting one
  (ADR-0035 heuristic 4).** Every intersecting claim came back as the same
  undifferentiated "overlap", so the caller had to guess which kind it had —
  and advice you have to guess at is advice you learn to skip. Each claim now
  carries the branch its owner declared at `open_session` and one of three
  signals: `same_branch_collision` (both sessions on one branch, the edits
  collide now), `cross_branch_merge_risk` (different branches, paid at merge),
  or `undeclared`. The result also echoes `requester_branch`, because an
  `undeclared` signal is ambiguous without knowing which side went quiet. The
  branch is never a call argument: it is declared once per session, and a
  second place to state it could disagree with the first — pass the optional
  `session_id` and the server reads the branch that session already declared.
  Still advisory and still never blocks a claim: no session, an unregistered
  token, or a session that declared no branch all fall back to exactly the
  answer this tool gave before, and say so rather than implying a verdict. The
  VS Code pre-flight warning names the cost in the same terms, and adds nothing
  when the context is undeclared.
- **A completion now says whether its evidence affirmed anything.** Reaching
  `done` said nothing about whether the conformance receipt behind it proved
  the work. Measured over this repository: **57 of 101 `done` tasks** rested on
  a `drift`/`needs_human` verdict or on an `aligned` one covering **zero
  nodes**, and 55 of 109 receipts overall covered no nodes at all — every one
  reading on the board exactly like a task whose evidence proved something.
  `board` rows now carry a derived `receipt` (`conformance_id`, `verdict`,
  `covered_nodes`, `checked_at`, `affirms`), and `export_evidence` gains a
  `Covered` column beside the verdict. `affirms` is true only for an `aligned`
  verdict that covered at least one node: agreement about nothing is not proof,
  so an `aligned` receipt over an empty bundle affirms as little as a
  `needs_human` one. Derived at read time from the durable record — nothing is
  stored twice — and a task with no record at all reports no receipt, which is
  distinct from one that proved nothing.
- **A conformance gate that checked nothing no longer reports OK.**
  `scripts/conformance-gate.mjs` printed `OK — N changed path(s), no governed
  gaps` whether it had verified every governed change or had inspected nothing
  at all. Those are not the same result, and on this repository it is nearly
  always the second: measured 2026-07-29, the constitution binds **8 code nodes
  and none of them are under `crates/`**, so a pull request touching fifty Rust
  files passed the gate having checked none of them — and said so in the words
  of a pass.
  That is the same shape as a conformance receipt that is `aligned` over an
  empty bundle, which this repository has already corrected once: agreement
  about nothing is not proof. The gate now returns what it was able to inspect
  (`inScope`, `ungoverned`, `governedNodes`) and reports `CHECKED NOTHING —
  none of N changed code path(s) are governed` when no changed path was in
  scope, naming how few nodes the constitution binds. Documentation is excluded
  from the ungoverned count, so a docs-only change does not read as a gap
  governance never claimed.
  Reporting only. Nothing new fails, and the dangling-binding check is
  unchanged. Two larger findings are recorded in the Known gaps of
  `DEVELOPERS.md` rather than acted on: 127 of 131 receipts cover zero governed
  nodes, and the gate cannot currently run in CI at all, because it reads an
  exported manifest that `.gitignore` excludes by policy.
- **A guard that has to be told what to check stops covering the next thing
  added.** The check that catches a server-side table naming a tool that no
  longer exists was given the tables to inspect, so it protected exactly the
  ones somebody had remembered to register with it — and forgetting is the
  entire failure it exists to prevent. That was not hypothetical: a fourth
  table, `REQUIRED_SESSION_ACTS`, was added after the guard was written and
  covered only because its author happened to also edit the guard. The next one
  would have been invisible, in the same silence that let ten stale session
  bindings survive a rename. The guard now discovers every `ToolAct` table by
  reading the source, so a table is covered the moment it is declared rather
  than when someone thinks to mention it. Proven by declaring a new table that
  names a tool which does not exist and is referenced nowhere else: the guard
  fails and names it. Two details carry the honesty of the scan. Emptiness is
  asserted per table rather than in total, because a scan that quietly stopped
  reading one table still satisfies a total — that is how a scan in this file
  passed on its own source for weeks. And the string the scan searches for is
  built at run time, so this file does not contain the literal being looked
  for; a guard that searches for text matches itself and then reads its own
  body as data, which has happened here before and did again while writing
  this one.
- **Every constitution acceptance property now names the test that proves it,
  and a guard fails if that test stops existing.** SPEC-CONSTITUTION §13 lists
  ten properties the constitutional machinery must satisfy; §13.1 now maps each
  to the tests that fail when it stops holding, so acceptance is re-checked by
  the suite on every change rather than by re-reading a list. Building the map
  found the one property with no proof at all: learned knowledge staying out of
  the constitution held only because knowledge and clauses live in unconnected
  stores, and "true because nothing links them" is precisely what a later
  convenience removes without anyone noticing it was load-bearing. That boundary
  is now pinned — a promoted signal becomes knowledge, creates no clause, and
  leaves an ungoverned repository ungoverned.
- **ADR-0061's merge queue is not available for this repository, and the ADR now
  says so.** GitHub's merge queue requires an organization-owned repository;
  this one is owned by a user account, so the "Require merge queue" checkbox is
  absent from branch protection rather than merely unticked, and no REST or
  GraphQL field exists behind it. The measurement that motivated the ADR stands —
  65% of CI in twenty-four hours spent re-running unchanged code — but the
  remedy is out of reach, so the status is now `Accepted (remedy blocked)` and
  the three genuinely available options are recorded: move the repository to an
  organisation, accept the churn, or reduce contention by arming fewer branches
  at once. Attempting the change also proved the ADR's own warning that its two
  halves must move together: unticking "require branches to be up to date"
  succeeded while ticking the queue was impossible, leaving `main` briefly able
  to accept two individually-green branches that break it together. The
  protection was restored with required checks unchanged. `merge_group` stays in
  `ci.yml` — inert without a queue, and it makes the organisation option a
  single settings change rather than a prerequisite to rediscover.
- **Agents are told about the delivery queue, and told not to fight it.**
  `AGENTS.md` gained a git-discipline rule: arm a pull request and leave it
  alone. The queue (ADR-0062) only removes contention if agents stop refreshing
  their own branches — if each one runs `gh pr update-branch` the moment it goes
  behind, they collide continuously and nothing drains, which is how eleven
  armed, green pull requests once sat unmerged for two hours. Merging `main` in
  by hand is now reserved for the one case the queue hands back: a real
  conflict. `scripts/delivery-queue.mjs --help` explains the same thing at the
  point of use.
- **Publishing to a branch with auto-merge armed now cycles the promise instead
  of refusing the push.** Arming auto-merge is a promise to merge whatever is on
  the branch the moment checks go green, so pushing afterwards races it — PR #37
  stranded four commits that way, and PR #134 later stranded five more. Refusing
  the push held the invariant but made every follow-up commit a manual
  disarm/re-arm dance, and the escape it pushed people toward was arming late,
  which means somebody sitting and watching a pull request instead. The
  publisher now withdraws the promise, pushes, and re-makes it about the tip
  that was actually published: at no point is there an armed promise about a
  branch being written to, and nobody merges or disarms by hand. A push that
  fails still restores the promise, because a failed push leaves the branch
  exactly as the promise already described it; a re-arm that fails leaves the
  pull request disarmed and says so, which is the safe direction — work sits
  unmerged and visible rather than merging something nobody promised. The guard
  module also has tests now, having been written with the stated purpose of
  being testable without a network and then shipped with none.
- **CI now triggers on `merge_group`, the prerequisite for a merge queue
  (ADR-0061).** Enabling a queue without it would have deadlocked delivery
  completely: the queue runs the required checks against a temporary
  `merge_group` ref holding the prospective merged result, and a required check
  that does not trigger on that event never reports — so the queue waits for it
  forever and nothing merges at all. All five required checks come from
  `ci.yml`, which triggered only on `push` to `main` and on `pull_request`. The
  trigger is inert until a queue exists, which is exactly why it lands first and
  on its own.
- **`DEVELOPERS.md` records what closing a stranded claim after the fact
  actually costs.** Most stranded claims are work that shipped and was never
  closed, so reconstructing the receipt is a natural move — and it has four
  traps that were all hit in one sitting, transitioning two live tasks to
  `in_review` in the process. `check_conformance` is not a dry run: it records
  an audit and moves the task, after which re-claiming fails. The whole claim
  window is too wide and produces `drift` from unrelated commits. `ingest_commit`
  defaults its `timestamp` to now, and because the node is upserted, one
  careless call fixes that commit at the wrong time permanently. The intent node
  is keyed by the sha string as passed, so comparing against `git rev-parse`
  reads a clean window as contaminated. And after all four are handled, a
  correctly bounded bundle for a documentation commit still returns
  `needs_human` — so until ADR-0060 is implemented the list cannot be worked to
  completion at all.
- **The command palette lists 8 commands instead of 34.** Typing "MindLeak"
  returned every contributed command, and 26 of them could do nothing from
  there: 20 are driven by a row in a view — from the palette they only answer
  "Run *X* from an Intent Board row" — and 6 are per-view refresh, which belongs
  on the view title where it already is. Those 26 are hidden from the palette
  and unchanged everywhere else: view title buttons and right-click menus behave
  exactly as before. What remains is the set that does something when invoked by
  name: prune, reconcile, export, back up, reset, ingest the active file, next
  task, sync ADRs. A test enforces the rule rather than the list, so a new
  row-driven command is caught instead of quietly rejoining the wall, and a
  budget assertion fails if the palette grows past ten without anyone deciding
  it should.
- **The constitution export is self-contained, so policy can actually be audited
  from it (SPEC-CONSTITUTION §13).** It rendered clause statements grouped by
  kind and nothing else: a reviewer handed the file could not tell which
  constitutional version it was, where a clause came from, what mechanically
  enforced it, or which exceptions were live — the four things an audit consists
  of. It now carries a `## Version` section (id, version, status, created and
  activated attribution, project identity, purpose, preamble), per-clause
  provenance, declared consequence, waivability and bound controls, and a
  `## Active waivers` section. Absent values render as `_not recorded_` rather
  than being omitted, because the migration that creates the first version
  deliberately invents neither rationale nor authority, and a document that drops
  its empty fields disguises that as completeness. On this repository the export
  grew from 7,133 to 10,674 bytes and immediately showed something the old one
  hid: the split between what enforces and what does not. Measured again after
  the fleet-discipline clauses were adopted, that split is **thirty active
  clauses, thirteen carrying a complete contract, and four binding any control
  at all — two of them mechanical**. The four are the source-file length
  ratchet and commit-provenance ingestion (both `observed`), and the
  shell-plumbing and worktree-ownership hooks (both `mechanical`).
  The earlier reading of this fragment — that six workflow rules bound
  mechanical controls — no longer holds, and the reason is worth knowing rather
  than quietly restating a number. Those delivery clauses were amended, and an
  amendment used to leave its controls pointing at the superseded clause id, so
  they were orphaned: `clause_controls` now reports `one-publishing-owner-per-task-branch`
  and `a-commit-stays-inside-its-declared-scope` as unguarded, though both still
  declare `block`. The mechanisms themselves never stopped working — the
  pre-commit hooks still exit non-zero — but the ledger can no longer resolve
  those clauses above `advise`, which is precisely the "a control that has
  stopped enforcing reads exactly like one that works" failure. Carrying active
  controls across an amendment by slug fixes it going forward and re-adopts the
  stranded ones at the next amendment; until then the count above is the honest
  one.
  The clauses that enforce nothing still include every locally-migrated clause,
  which is to say every invariant the project wrote about itself: the zero-token
  hot path, decay, derived effective weight, the local-only security boundary.
  Borrowed rules about how work is delivered are the ones that acquired
  mechanisms; the project's own rules about what it must never do have none.
  That much is correct behaviour — migration invents no authority (§10) and
  broad principles route to review (§13) — and it was invisible while the export
  rendered enforcing and inert clauses identically.
- **`tasks.claim_lapses` and `tasks.unleased_seconds` are gone; continuity is
  derived from the log.** ADR-0064 decision 5. The two running totals the claim
  compare-and-swap maintained are replaced by `claim_window`, which replays the
  recorded transitions. Their agreement was proved in the preceding commit while
  both still existed; the migration drops the columns only *after* the genesis
  import has carried their values into the log, because they are the sole
  surviving trace of a window that opened before the log did.
  `ALTER TABLE ... DROP COLUMN`, never a table rebuild: a rebuild rewrites every
  row including `owner` on live claims, and ADR-0063 is explicit that a live
  claim is not ours to touch. Dropping an unrelated column moves nothing.
  The fields are **not** kept on `Task` as derived values. Zero lapses means
  "this window may certify itself as aligned", so a field any read path could
  leave unpopulated would fail *open* — quietly handing out a clean receipt for
  work with holes in it. Conformance and the conformance token now ask for the
  window explicitly; there is no field to forget.
  Board rows carry `claim_window` instead, so the continuity a reader needs is
  still beside the status rather than a query away. `scripts/stranded-report.mjs`
  reads it from there.
- Goal coverage can now be declared while a claim is live, and is refused once
  conformance has judged the task (ADR-0074). `also_serves` was fixed at task
  creation, on the sound reasoning that coverage added after conformance
  complains is a rationalisation. But goals bind to files, so the governing set
  is learned while working, not predicted at creation — and after the first
  commit the previous remedy did not work at all: a task created to gain
  coverage cannot own the earlier work's evidence
  ("evidence interval falls outside the live claim"). Measured on 2026-07-30,
  one change took three task creations and still shipped a drift receipt.
  The boundary moves from creation to the first verdict, which is the
  distinction the original rationale already named — a rationalisation is for a
  finding *already raised*. Before any finding, a declaration is still a
  prediction the evidence can contradict.
  Declaring is owner-guarded and claimed-only, unions rather than replaces so it
  can never drop a goal declared earlier, and appends a `coverage_declared` task
  event so a task that grew its scope shows when and by whom. Work already in
  `in_review` cannot be re-claimed, so a verdict cannot be widened and re-judged.
  No new tool verb: the declaration rides on `task_claim`, which already says
  what a task expects to touch, and a same-owner re-claim keeps the evidence
  window open.
- Each window now roots its MCP servers at the worktree it is editing (ADR-0073).
  `.vscode/mcp.json` bound both servers to `${workspaceFolder}/target/release`,
  and every window's workspace folder was the primary checkout, so a file edited
  in any other worktree could not be made repository-relative. Measured on
  2026-07-30: `ingest_file` refused 257 of 6450 calls (4.0%), roughly two per
  minute, and those files never entered the graph at all.
  `cwd` and `MINDLEAK_WORKSPACE` still follow `${workspaceFolder}`, so opening
  the worktree as the workspace folder is now enough to make saves land under the
  canonical id. Worktrees continue to share one graph and one board — the
  repository id derives from the git common dir, not from the folder opened,
  which was verified across three worktrees before the change.
  The servers are installed once per machine at `~/.mindleak/bin` by
  `make install-servers`, rather than being built into all 56 worktrees (184 GB
  of build output already on disk, only 15 holding a server binary). Because the
  binary now sits outside the workspace, the build notice reports it as an
  installed binary — identity without a staleness claim it cannot support.
- **Every task transition now records itself in the log.** ADR-0064 step two:
  the sixteen verbs that mutate task state — claim, renew, heartbeat, block,
  reopen, abandon, resolve, ask, answer, pause, resume, release, both
  conformance transitions, claim recovery, and creation itself — append a typed
  event inside the same transaction as the guarded write they perform. Four
  verbs that previously wrote outside a transaction (`renew_lease`,
  `touch_lease`, `resume_task`, `release_task`) now open one, because a record
  that can commit separately from the row it describes is not a record.
  The claim compare-and-swap is untouched. It is a single guarded UPDATE inside
  an Immediate transaction and it was already right; the event is appended
  beside it rather than atomicity being rebuilt on top of a log.
  `open_blocked_successor_on` resolves the successor before updating it instead
  of updating through a subquery, so a predecessor-driven unblocking can be
  recorded against the task it actually moved. That event has no actor by
  design: nobody asked for it, the gate simply lifted, and naming a caller would
  attribute a decision no agent made.
  `project_tasks` replays the log into task state, and a test walks a task
  through most of its lifecycle and asserts the replay reproduces the live board
  exactly. That test is the point: `tasks` is written through rather than
  rebuilt (ADR-0063 forbids a migration from touching a live claim), so "the
  projection is derivable from the log" is a property that would otherwise
  quietly stop holding the first time a verb forgot to record itself.
- **An expired claim no longer wears the owner's icon on the board.** The sort
  already knew a lapsed claim is ready work — the store's compare-and-swap
  admits it and the row says "Claim expired · Ready" — but every claimed row,
  live or lapsed, still drew `account`, the icon that means *someone is holding
  this*. So the one row that means "abandoned, take it" looked exactly like the
  rows that mean "hands off", and a board carrying fifteen of them read as a
  fleet at capacity when most of it was free. An expired claim now draws
  `watch`: a lease is a timer, and this one ran out. Derived from the clock at
  render time in `boardIconId`, never reaped or written back, so nothing is
  mutated to make the picture true.
- **A goal may now bind the artefact it actually delivers, and the verb is named
  for it.** `link_goal_to_code` bound code, and a `governed` binding to a
  documentation node was discarded before it could be classified — so a goal
  whose delivery *is* an ADR, a doc, a benchmark or a build script had no way to
  say so. `touched_task_goal` was vacuously false for that work, and the finding
  "does not touch code bound to the task goal" was attached to tasks that had
  touched precisely the artefact their goal named; the only way to silence it was
  to bind an unrelated source file. The documentation exclusion now applies only
  to the *drift* branch, which is the case it was written for: an honest
  changelog touch still never drifts against a goal that merely bound it, but a
  doc bound to a task's own goal (or to a goal the task declared it covers)
  counts in scope. `link_goal_to_code` and `unlink_goal_from_code` are renamed to
  `link_goal_to_artifact` and `unlink_goal_from_artifact`, with every caller
  migrated in the same change and no alias shipped beside them (ADR-0059,
  ADR-0060).
- **The conformance gate now reports governed bindings that name no file
  (ADR-0031).** Splitting, renaming, or deleting a governed file moves the code
  and leaves the binding pointing at a path that no longer exists. Nothing
  failed when that happened: the constitution simply stopped governing the code,
  `advise` found no clauses for the new paths, and the loss was invisible —
  an orphaned binding looks exactly like code that was never governed. Measured
  on this repository after a refactor campaign: **7 governed ids named files
  that no longer existed**, including `graph/query.rs` and `graph/signal.rs`,
  split hours earlier by the very campaign that unbound them. The gate cannot
  catch this by watching diffs, because an orphaned id never appears in one; it
  now checks every governed binding against the working tree and reports the
  ones that resolve to nothing. Advisory alongside the existing receipt check,
  and failing under `--strict`.
- **`graph_stats` now reports what is silently broken, not just what exists.**
  Four capabilities were built correctly and left mute, and each cost hours: the
  publisher enforced claims but recorded no evidence, `AGENTS.md` never named the
  read tools it depended on, the embedding index existed but nothing refreshed
  it, and the build sha was reported but never compared. The common failure was
  not missing capability — it was capability that never speaks up.
  `graph_stats` is the call the fleet already makes constantly, so it is where a
  regression has to announce itself. It now also reports **nodes `recall` cannot
  see** (no embedding for the active model) and **nodes still carrying a split
  identity** (an absolute path in the id). Both rows appear only when the count
  is non-zero: a health row that is always present is one readers learn to skip,
  which is how these stayed invisible in the first place.
  The value was immediate. On first run against the live graph it reported 110
  unembedded nodes and **235 split identities that had reappeared since the
  repair**, because other agents' servers are still running binaries built before
  paths were made repo-relative. That regression was already underway and nothing
  would have said so. A missing embedding index reports every node as
  unrecallable rather than failing, because taking `graph_stats` down would
  remove the one health signal the fleet reads.
### Changed

- **Known gaps are fragments, so recording one never conflicts.** The Known gaps
  section of `DEVELOPERS.md` was a single shared append-only list of 81 entries,
  so every branch that recorded a gap edited the same lines. Almost every pull
  request collided there, and each conflict expressed no disagreement whatever —
  two agents adding two unrelated observations to the same paragraph. It was
  hand-resolved four times in one session.

  ADR-0056 already solved this shape for `CHANGELOG.md`: a fragment is a new
  file per item, and two branches never write the same path. Gaps now live in
  `gaps.d/`, one file per gap, with `node scripts/gaps.mjs --list` to read them
  and `--check` in the pre-commit hook to refuse a malformed one.

  One deliberate difference from `changelog.d/`: a changelog fragment folds into
  the file at release and is deleted, but a gap has no release event — it is
  open until it is fixed. Folding would put the shared list, and the conflict,
  straight back, so the fragments are the source of truth permanently and
  `DEVELOPERS.md` points at them instead of holding a generated copy. Closing a
  gap deletes its fragment in the commit that fixes it, so the fix and the
  retraction are one reviewable change.

  `--check` fails on an empty `gaps.d/` rather than reporting success. An empty
  Known Gaps section is almost always a lie, and a validator that passed over a
  directory which had quietly lost every gap ever recorded would give the one
  answer it must never give.
- **A scoped task claim now carries the memory pre-flight agents were
  skipping.** Live telemetry showed ADR-0066's adoption gate had failed: five
  writing sessions made 1,033 attributed writes without a successful memory
  read or MindLeak `check_overlap` before the first write. A won Lodestar
  `task_claim(step = "claim")` now returns a structured `memory_preflight` for
  the exact claimed paths and symbols, naming MindLeak `check_overlap` and the
  requirement to call it before the first edit. The response remains advisory
  and explicitly does not claim the cross-plane read already ran; unscoped or
  lost claims remain quiet. Memory-habit telemetry now counts a successful
  `check_overlap` as the deterministic retrieval ADR-0066 made it.
- **Memory tools now tell the model when to use them, without adding another
  tool or growing their advertised surface.** Telemetry measured the adoption
  failure on 2026-07-29: `ingest_execution`, `ingest_file`, and `ingest_commit`
  had run 10,122 times, while `recall`, `working_set`, `get_impact_radius`, and
  `graph_multi_hop_query` had run only 70 times between them. Their
  `tools/list` descriptions previously defined mechanisms but supplied no cue,
  so writing became habitual and reading did not. The four existing tools now
  name the moments already present in an agent's work: resume or task switch,
  before the first edit, questions about why/prior decisions/regressions, and
  deterministic task-text traversal when semantic recall is unavailable. A
  contract test reads the actual advertised definitions and preserves those
  cues while holding their combined compact JSON at or below the measured
  2,072-byte baseline.
- **The merged-branch audit now runs on every push to `main` instead of only
  when someone remembers.** A merged pull request whose commits never reached
  `main` fails nothing anywhere: the pull request reads merged, the branch reads
  ahead, and CI is green on both — the only signal is an ancestry check nobody
  was running. `scripts/merge-audit.mjs` is that check, and it identifies both
  known incidents correctly after the fact, naming the pull request and each
  commit that was left behind. It was reachable only through `make merge-audit`,
  a command with no reason to be typed on a good day, which is precisely when
  this failure happens. It now runs in CI on pushes to `main` — the moment when
  "did the thing that just merged leave work behind?" is a live question — and
  not on pull requests, where it is not yet a question at all.
- **ADR-0060 proposes that work whose product is not code must still be able to
  conform.** Conformance ends with two rules that look symmetric and are not:
  evidence touching no governed code with no task attached is `aligned`, while
  the same evidence with a task attached is `needs_human`. Attaching a task
  makes the verdict worse, so a task whose product is documentation, an ADR, a
  benchmark, or a build script can never reach `aligned`. Measured across this
  repository's 169 tasks (90 with an audit): 45 aligned, 34 `needs_human`, 11
  drift — and the 34 have exactly two causes, neither of them human judgement.
  24 are the `ingest_commit` argument-drop defect and 10 are this rule, so 38%
  of audited work is parked structurally.
- **A newly started agent now receives work whose owner disappeared.**
  `open_session` conditionally returns `rescue_work` for expired claims and
  deadlocked wait cycles already identified by Lodestar's durable
  `stalled_work` projection. Each entry names the prior owner and branch when
  known, explains the stall, and includes the canonical `task_query` action to
  inspect it plus the `task_claim` action that can take an expired claim.
  The field is absent when there is nothing to rescue. It is read-only: opening
  a session never steals, closes, or otherwise mutates work. Ordinary
  peer-addressed questions remain in `waiting_on_you`, deliberate pauses remain
  with their owner, and completed work awaiting a person remains in
  `awaiting_a_human`, so the rescue signal does not become another noisy board.
- **The README is a router again, and the tool reference has its own page.** The
  front door carried 90 rows of tool tables and put architecture and build
  instructions ahead of "how do I try this", so the fastest path for a new
  reader was to scroll past the design of the system to reach the install. The
  tables move to `docs/TOOLS.md` — a reference is for looking things up in, not
  for reading — and the getting-started sections now come before the ones that
  explain how it works. README drops from 436 lines to 316. Every pointer moved
  with it: `AGENTS.md`, `DEVELOPERS.md`, `docs/ARCHITECTURE.md`, `docs/USAGE.md`
  and the pull-request template all named the README table as the thing to
  update when a tool is added, and would otherwise have sent the next
  contributor to a table that is no longer there.
- **Both "adding an MCP tool" worked paths pointed at a file that does not
  exist.** `crates/mindleak-mcp/src/tools.rs` became a directory when the tools
  were split into modules, and the instruction to add a `CHANGELOG.md` line
  predates fragments (ADR-0056). A worked path that names the wrong file is
  worse than none: it is followed confidently.
- **`DEVELOPERS.md` now says what to do when a generated file conflicts.**
  `docs/adr/README.md` is derived from the ADR files, so every branch that adds
  an ADR appends a row at the same place and conflicts on every merge from
  `main` — three separate branches hit it in one session with nothing in the
  docs to say the resolution is `make adr-index`, not a hand-merge. Keeping
  "both sides" of a generated table produces a duplicated index that the
  pre-commit check then rejects, so hand-resolving it is discarded work.
- One-off data repairs move out of `db/migrations.rs` into `db/repairs.rs`. A
  schema migration changes shape and is cheap to re-run; a repair rewrites rows
  to undo damage a defect already did, and firing twice can undo work someone
  did in between. Filing them together made that distinction invisible. The
  split also returns `migrations.rs` below the module-length clause: adding the
  stranded-clause repair had pushed it from roughly 416 to 476 non-test lines,
  past the 450 the clause allows, taking the repository from 7 oversized modules
  to 8. It is back to 7.
- **A required tool argument must now be reachable — declared in the schema or
  injected by the session — and a guard fails if one is neither.** Two ways an
  argument gets to a handler: the caller reads it in the schema and sends it, or
  `bind_session` resolves the session and injects it. `agent` is the second kind
  — it is deliberately stripped from every session-bound tool and replaced by
  `session_id`, so attribution on the verbs that amend the constitution, grant
  waivers and accept ratchet baselines is *resolved from the session*, never
  asserted by the caller. A tool that declared `agent` would let a caller name
  itself anything it liked. But a handler reading `agent` beside a schema that
  never mentions it looks exactly like the `lease_secs` typo incident, and the
  symptom if it ever were one is identical: `missing required string arg`
  naming a field the tool does not advertise. The rule is now pinned across the
  constitution, amendment, control, waiver, executive and design tools, with a
  floor on how many arguments the check inspects — a source scan that quietly
  stops matching anything is a green tick over an empty set.
- **A paused task whose owner disappears now reaches new agents after its
  seven-day protection grace.** `open_session` keeps healthy pauses private to
  their owner, then includes overdue paused work in `rescue_work` with the
  former owner, branch, and canonical scope/claim actions once normal recovery
  is allowed. Reading the queue never transfers or mutates the task.
  Core and MCP regressions pin both sides of the boundary so deliberate short
  pauses stay quiet while abandoned pauses cannot remain invisible forever.
- **Completions that predate resolver attribution are accepted as historical
  (ADR-0069).** `resolve_task` validated the `human` argument and then discarded
  it, so for most of this project's life a human acceptance overriding a
  conformance verdict recorded that it happened but never by whom. The columns
  now exist and populate. Measured on the live board: **268 tasks, 147 `done`,
  17 carrying a resolver, 130 carrying none**, with the earliest recorded
  resolution at unix `1785285644`. Those 130 will not be reconstructed,
  annotated, or re-attested — the identity was never written anywhere, so there
  is nothing to recover, and re-accepting them now would manufacture attribution
  for judgements nobody can verify. `resolved_by IS NULL` on a completed task
  therefore means *predates attribution*, not *accepted by nobody*; a report
  rendering it as an absence of authority is wrong about what happened. The
  boundary is sharp from `1785285644` onward. Supersedes the earlier "57 of 101"
  figure, which measured a different cut (the verdict on the receipt rather than
  the presence of a resolver) on 28 July.
- **Retiring a control is now attributed.** Standing a control down is the one
  act that reduces what a clause can enforce without changing a word of the
  clause — closer to granting a waiver than to editing a configuration file —
  and it was recorded as a bare status flip with no author. The store now keeps
  `retired_by` and `retired_at`, the tool is session-bound so the author is
  resolved from the session rather than supplied by the caller, and an
  unattributed retirement is refused outright. Controls retired before this was
  recorded carry no author: those retirements cannot be reconstructed, and
  inventing one would be worse than admitting the gap.
- **The 18 extension settings are grouped instead of one flat list.** "Where is
  the server binary" sat beside "how many characters of terminal output to
  retain" with nothing to say which one a first-time user needs to touch. They
  are now four titled sections — Servers, Capture, Consolidation, Views — which
  VS Code renders as separate blocks in the settings UI. Setting ids are
  unchanged, so no existing configuration moves and nothing about behaviour
  changes; this is presentation only. A test asserts each setting belongs to
  exactly one non-empty titled group, and — the failure that actually costs
  someone an afternoon — that every setting the code reads is declared in the
  manifest. An undeclared setting silently returns its inline fallback, so it
  cannot be found in the settings UI and appears to do nothing when set by hand.
- **Task resolution now says what its reviewer field actually proves
  (ADR-0071).** `resolve_task` records a non-empty reviewer label in
  `resolved_by`; the label is attributable but not authenticated. Lodestar has
  no human identity provider, so core errors/docs and the MCP contract no longer
  call the value a verified identity. The same-string self-review guard remains,
  but any other label is accepted and stored unchanged. A regression pins that
  behavior with a deliberately non-credential label so the API cannot quietly
  regain stronger wording than its mechanism supports.
- **The Telemetry pane now polls periodically only after the user enables
  Live.** The three-second timer previously ran whenever the pane was visible,
  even with Live off. Lifetime telemetry measured 16,522 `graph_stats` calls
  and 12,567 `telemetry_snapshot` calls against 66 reads that could change a
  decision; `graph_stats` alone spent 57 cumulative minutes answering the
  dashboard. That wasted compute and made the telemetry record mostly describe
  its own observer.
  Opening the pane, clicking Refresh, and toggling Live still refresh
  immediately. The configured cadence is unchanged and applies only to the
  opt-in live stream. A pure four-state regression test pins hidden,
  visible-non-live, and visible-live behavior.
- The constitution and policy-pack tools collapse to a vocabulary (ADR-0059):
  `constitution_define`, `constitution_decide` and `constitution_query`, beside
  the already-collapsed `policy_pack_register`, `policy_pack_decide` and
  `policy_pack_query`. Each names its transition in an `action` argument rather
  than in a tool name.
  Every superseded name still answers for one minor version and its description
  names the call to make instead — a caller mid-task cannot read a changelog, so
  the deprecation has to teach rather than simply break. No guard was lost: each
  refusal a separate tool name used to encode is now an argument validation
  carrying the same message, including the attribution required to adopt policy.
- **The fifteen-tool design cluster is now four verbs.** Registering a design,
  deciding it, promoting it and reading the ledger each had their own tool
  name — fifteen entries on the surface for four things an agent actually
  does, and an agent choosing between `reopen_undecided_design` and
  `attribute_design_decision` had to know which one its row belonged to before
  it could ask. They are now `design_register`, `design_decide`,
  `design_promote` and `design_query`, with the act named as an argument
  (`decision`, `step`, `view`). Nothing was relaxed to make them fit: every
  refusal a separate name used to encode is now argument validation carrying
  the same message, and the ADR-0051 guards survive intact — `attribute` still
  refuses to overwrite a `decided_by` the ledger already holds, and `reopen`
  still defers to materialisation, refusing a row whose promotion has created
  work. The two therefore continue to partition the undecided rows rather than
  overlap. The old fifteen names still answer for one minor version, and each
  reply names the call to make instead, so the deprecation teaches rather than
  merely failing; removal ships with the release train named in ADR-0059.

- **The guard that checks the server advertises everything it answers to was
  reading its own source.** `every_tool_the_server_answers_to_is_advertised`
  scans dispatch blocks by searching for the text `match name {` — and
  `mod.rs`, which delegates to every other module and dispatches nothing
  itself, contains that text only inside the test's own search call. The scan
  therefore treated the rest of its own file as if it were tool dispatch. It
  now reads only the modules that answer to a name, and only the arms of the
  dispatch itself rather than the nested matches that parse arguments, since
  reporting an argument value as an unadvertised tool trains people to ignore
  the guard. Because that narrowing is exactly the kind that can stop reading a
  module without failing anything, the test now asserts it found dispatch in
  every module it claims to cover, not merely in total.
- The VS Code extension now provides both MCP servers itself, and the committed
  `.vscode/mcp.json` is gone (ADR-0073). The extension contributes them through
  `mcpServerDefinitionProviders`, rooting each server at the workspace folder of
  the window that provides it, so the rooting behaviour ADR-0073 established is
  unchanged. Where a binary lives is now decided by one tested rule —
  `resolveBinaryPath` — instead of a config file carrying a second, untested copy
  of it, and a new machine no longer needs a hand-edited config to reach the
  servers.
- **The extension now requires VS Code 1.101 or newer**, up from 1.93. The MCP
  extension API shipped in 1.101 (May 2025). `engines.vscode`, `@types/vscode`,
  the pinned Extension Host smoke version and its CI job name all move together,
  because a smoke job on 1.93 testing code that needs a later API is a green
  build that proves nothing. `@types/vscode` is pinned exactly to `1.101.0`
  rather than a caret, which resolves forward and would let code compile against
  APIs the declared floor does not have.
  This is a real support cut: the graph views and the passive sensor did work on
  1.93, because the extension speaks MCP through its own client. What never
  worked there is the editor's own MCP support — 1.93 shipped in August 2024 and
  MCP was announced that November — so the old floor advertised a version on
  which MindLeak's purpose was impossible.
- Server resolution now prefers the shared install at `~/.mindleak/bin` over a
  worktree's own `target/` build. Reusing the previous order would have
  reinstated the per-worktree binary ADR-0073 rejected on measurement (56
  worktrees, 184 GB of build output, only 15 holding a server binary). A side
  effect: the extension's own client and the servers offered to chat agents can
  no longer resolve to different builds, which they previously could.
- Action required in this repository: install the extension build that contains
  the provider (`npm --prefix editors/vscode run package:vsix`, then
  `code --install-extension`). There is no committed config to fall back on.
  Outside this repository, `editors/vscode/scripts/install.mjs` still writes a
  `.vscode/mcp.json` for editors without the extension and for the Copilot CLI;
  running both mechanisms in one workspace would register each server twice.
- **The pre-flight now answers the whole pre-flight question.**
  `check_overlap` already takes the paths and symbols an agent is about to
  touch, and the before-you-write checklist already mandates it — but it
  reported only which other agents were there. On a file nobody else was
  touching it returned an empty list, which reads as all-clear, while the graph
  already held that file's commit rationale and any execution that had failed
  on it. Learning that needed `get_impact_radius`, a second call at the moment
  attention has already moved on.
  Measured over this repository's lifetime telemetry: **8,109 ingests against
  66 reads at decision time** — `recall` 49, `graph_multi_hop_query` 10,
  `working_set` 4, `get_impact_radius` 3. Roughly 123 writes per read, plus
  32,980 dashboard polls (`graph_stats` alone has spent 57 minutes of compute
  answering "how many nodes are there"). The retrieval benchmarks were never
  wrong — `docs/EVALUATION.md` measures mean F1 0.77 against 0.44 for a vector
  arm — they answer *if you ask, is the answer good*, and never *does anyone
  ask*.
  So the answer now rides on the question agents already ask. `check_overlap`
  returns `impact` (dependents, previously failing executions, related
  intents), `unknown` (ids the graph has never seen), and `requested` alongside
  the existing `footprints`. No new tool: adding a sixth retrieval tool beside
  five that are already unused would repeat the failure, not fix it.
  `unknown` is reported separately from an empty `impact` on purpose. "The
  graph has never seen this file" and "nothing depends on this file" are
  different facts, and a caller that cannot tell them apart reads silence as
  reassurance.
  Deterministic and zero-token throughout; the ingest and query hot paths are
  untouched. See ADR-0066.
- **The prose docs now speak the current tool vocabulary.**
  The task-lifecycle and design clusters collapsed into four verbs each
  (`task_create` / `task_claim` with `step` / `task_transition` with `to` /
  `task_query` with `view`, and `design_register` / `design_decide` /
  `design_promote` / `design_query`), but USAGE, SPEC-INTENT, WALKTHROUGH,
  QUICKSTART, ARCHITECTURE, TOOLS, SPEC-CONSTITUTION, AGENTS, and the extension
  README still called the retired names — actively misdirecting any agent
  reading them. Every worked flow, the §9 wire-tool contract, and the inline
  references now name the current verb and its real argument shape (e.g.
  `complete_task(...)` → `task_transition(task_id, to="complete", ...)`,
  `next_task()` → `task_query(view="next")`, `renew_lease(...)` →
  `task_claim(task_id, step="renew", ...)`). References to the Rust **facade
  methods** (e.g. `Lodestar::complete_task` in ARCHITECTURE) and to `task_query`
  **view** names are unchanged, because those are current — only the client-facing
  MCP tool names had moved.
- **The README layout diagram no longer states a per-server tool count.**
  It read `mindleak-mcp (23 tools)` / `lodestar-mcp (49 tools)` — both stale
  after the cluster collapses (ADR-0059) and the default/full profile split
  (ADR-0059 rule 2), where a single number is neither current nor meaningful (17
  in the default profile, more under `LODESTAR_TOOL_PROFILE=full`). A layout
  diagram is not the place a count can be kept honest, so it no longer claims
  one; `docs/TOOLS.md` and `scripts/measure-tool-surface.mjs` are where the
  surface is stated and measured.
- **`record_knowledge` now says what evidence must carry, before the record is
  written.** The conformance advisory matches recorded knowledge on referenced
  nodes and nothing else, so evidence without a `nodes` array produces a record
  that is stored, counted, decayed on schedule, and can reach nobody. The
  schema described that field as "JSON provenance" — accurate, and silent about
  the one thing that decides whether the lesson ever arrives. It now names the
  `nodes` array, shows the shape, and states the consequence of omitting it.
  This is where the caller decides what to send; the `surfaces` warning added
  in the reply is the backstop for getting it wrong anyway, and it necessarily
  arrives after the record already exists. Measured when this landed: 67 of 170
  active records, 39%, name no nodes. Among them are the lessons most worth
  having — that skipping the ADR-0029 pre-flight causes the drift verdicts
  people then blame on goal bindings, and that testing a facade method proves
  the logic and says nothing about the MCP wiring, which is exactly how
  `merge_evidence` shipped refusing every caller. Both were written so the next
  agent would not repeat the mistake, and neither could be delivered to anyone.
  `consolidate` was never affected: it requires `evidence_node_ids` outright,
  which is why only the free-form path grew a backlog. A test pins the
  description so the guidance cannot quietly regress to something true and
  useless.
- **The twenty-six-tool task cluster is now four verbs.** Creating work,
  owning it, moving it through the lifecycle and reading the board each had
  several tool names — twenty-six entries for four things an agent does, in the
  cluster every session touches. They are now `task_create`, `task_claim`,
  `task_transition` and `task_query`, with the act named as an argument
  (`step`, `to`, `view`), following the same rule ADR-0059 applied to the design
  cluster: where a cluster moves one entity through a state machine, the tool
  surface should reflect the machine rather than enumerate it. Every guard is
  now an argument validation carrying the same message, and each refusal names
  the transition that wanted the argument, which the old flat `missing required
  string arg: reason` could not. The twenty-six old names answer for one minor
  version and each reply names the call to make instead; removal ships with the
  release train ADR-0059 names.

  Two guidance strings changed with them, because they name a call an agent is
  expected to make next: a lost claim now says `task_claim with step="recover"`
  can take it over, and an expiring lease says to call `task_claim with
  step="renew"`. Advice that names a verb nobody will find is worse than no
  advice.

- **A deprecated tool name silently lost its argument checking.** Argument
  validation looks a tool up by name to find the schema to check against, and a
  collapsed cluster's old names are deliberately absent from that list — so for
  every caller still using an old name, `validate_arguments` found no schema and
  returned "fine". That is precisely backwards: the callers on the old names are
  the ones most likely to get an argument wrong, and the window lasts a whole
  minor version. The incident this guard exists to prevent — `lease_seconds`
  passed where the tool declares `lease_secs`, silently dropped, the default
  applied, and the claim lapsed mid-work — was reachable again through the old
  name. Deprecated names are now validated against the schema that actually
  answers them, which is also the schema whose argument list the error message
  should be quoting. This shipped broken with the design cluster in the previous
  release and is fixed for both.

- **The three collapsed clusters now share one deprecation implementation.** The
  rename table, its "call this instead" notice, and the two argument helpers
  every collapsed tool needs (`one_of`, and the conditional-requirement message
  that names which transition wanted the argument) were written for the design
  cluster and about to be copied a third time. They live in `tools/mod.rs` now,
  with each cluster owning only its own table of names — which is also what lets
  argument validation find every rename from one place instead of asking each
  module in turn.
- **The claim-transfer archive is closed to writes, and kept.** ADR-0064
  decision 5, second half. `recover_claim` no longer writes
  `task_claim_transfers`; a recovery is recorded once, as a `claim_recovered`
  event in the task log, beside every other transition it has to be read
  alongside. Two records of one act could disagree, and the disagreement would
  surface exactly when the ledger was being used to settle who held what.
  The table is **not** dropped. It holds ownership recoveries that happened
  before the log existed — real, accurate, and nowhere else — so
  `claim_transfer_history` now reads both and each row carries a `source` of
  `archive` or `log`. That is stated rather than implied because `id` means
  different things in each: an archive row id, or a position in the task log.
  The prior claim's window is reconstructed from the after-image of the event
  the recovery interrupted, so it is read back from the log rather than copied
  into a second table.
- **Named what actually records commit provenance, and what it silently skips.**
  A Known gap recorded the question as unexplained: commits were getting
  provenance with no post-commit hook installed, and two guesses at the cause —
  that the hook ran and timed out, and that `canonical-push` ingests — were both
  wrong. The answer is the VS Code extension's passive git sensor:
  `editors/vscode/src/gitSensor.ts` watches `repository.state.HEAD` and calls
  `ingest_commit` with `commit.hash` and the commit's own date. That also
  explains why re-ingesting a commit by hand returns `nodes_created: 0` — the
  sensor got there first.
  The more useful half is what it does *not* record. It ingests only when the new
  HEAD is a child of the previous one, so a branch switch, a checkout, or any
  non-linear HEAD move records nothing. In a fleet where every unit of work gets
  its own branch that is the common path rather than an edge case, and it is the
  true cause of the empty evidence bundles that were repeatedly read as
  ingestion being broken. Provenance also depends on the workspace being open in
  an editor at all: an agent working through a terminal in a worktree nobody has
  open records nothing, silently.
  Documentation only. Whether that skip is the right behaviour is a separate
  decision and is deliberately not settled here.

### Fixed
- **The Design Board reads the ADR record from main, not from the open
  checkout.** `readWorkspaceAdrMetadata` globbed `docs/adr` on disk and
  registered whatever it found, so the decisions the ledger was told about
  depended on which worktree the window happened to have open. Under ADR-0038
  that is a different subset per window: measured across 84 attached worktrees,
  `origin/main` held 75 ADRs and the union across all 196 remote branches was
  also 75 — main is the complete record and nothing is ever branch-only — yet 65
  worktrees were missing between 1 and 26, and the checkout this extension reads
  held **49 of 75**. Registering that subset tells the ledger that 26 decisions
  do not exist, silently. The record now comes from `origin/main` through a
  single `git cat-file --batch`, one process rather than one per ADR. A folder
  whose ref cannot be resolved — a fresh clone with no remote — still falls back
  to its working tree, but says so in the output channel and a warning, because
  a partial record that reports itself as complete is indistinguishable from a
  good one. The git read lives in `adrRecord.ts`, free of `vscode` so it is
  directly testable; its content is parsed by byte length, since git's framing
  counts bytes and one multi-byte character would otherwise shift every ADR
  after it.
- The reclaimer no longer deletes the worktree it is running in. Found on the
  first live run of `scripts/worktree-reclaim.mjs` against 94 real worktrees and
  shipped in the previous change without it: the tool's own worktree was merged,
  clean, idle, owned by this session and not mid-build, so every rule said
  reclaim it. Acting on that would have deleted `target/` out from under the
  running process and then called `git worktree remove` on the checkout it was
  executing in.
  It did not happen only because the report prints before anything is deleted
  and was read before `--reclaim` was passed. That is the reporting default
  doing its job rather than a guard, and it protects nobody who trusts the tool
  enough to pass `--reclaim` immediately — which is what AGENTS.md now tells
  every agent to do once their PR merges. The instruction and the defect shipped
  together.
  The refusal names its reason like every other rule, because a worktree that
  simply vanished from the report would read as one the tool failed to see.
  Compared on resolved paths, and decided before the cheaper exits, since the
  tool can equally be run from the bare primary or a detached worktree.
- **A skipped commit-ingest now says so instead of losing provenance in
  silence.** The post-commit hook could give up without a word: on timeout, on
  an unstartable server, or on a transport error. The symptom — an empty
  evidence bundle — is indistinguishable from an agent who simply forgot to
  ingest, which is the exact failure the hook exists to eliminate, so the
  diagnosis lands on the wrong cause. It still never fails a commit; it now
  names the sha, says the commit succeeded, and says to backfill with the
  commit's *own* timestamp, because a node keeps whatever timestamp it was first
  given. The budget is configurable via `MINDLEAK_INGEST_TIMEOUT_MS`, and an
  unstartable server no longer throws an unhandled `error` event at the
  committer. Never blocking and never reporting turned out to be different
  promises; only the first one was load-bearing.
  Separately and more seriously, the hook is **not installed** in environments
  set up before `default_install_hook_types` was added, because that setting
  only takes effect when `pre-commit install` is re-run — so it reports nothing
  because it never runs. Recorded in Known gaps; the fix touches the shared
  hooks directory and therefore every agent at once, so it is not applied here.
- Three script test headers advertised `node --test scripts/`, which fails on
  Node 24 — the portable runner `node scripts/script-tests.mjs` already existed
  and already documented that trap.
- **A migration no longer re-owns a live claim (ADR-0063).** The ADR-0054
  identity collapse rewrote `tasks.owner` for every labelled row and re-fired on
  every database open, because its idempotence was by *pattern* — "rewrite
  whatever still looks unmigrated" — which holds only while nothing else creates
  such rows. In a fleet sharing one `spec.db`, a *running server process older
  than the file it was loaded from* was doing exactly that, so each open by a
  newer binary re-owned whatever the older one had just claimed. Nothing stale
  was deployed: every binary on disk returned the collapsed id when driven
  directly, and only the live extension-hosted processes returned the labelled
  one. Restarting them was the whole remedy, and nothing in the system could say
  so. Observed on `task:f6daad456855`: one
  session, one token, `open_session` returning `session:v1:copilot:b4baf280…`
  while the board reported `session:v1:b4baf280…`, flipping between consecutive
  reads with no claim in between. The holder could not prove its work
  (`check_conformance` → "evidence agent does not own the task"), could not park
  the task to explain, and read as a different owner on re-claim — opening a
  fresh evidence window and orphaning the commit it had already made. Three
  changes: `tasks.owner` is treated as live state and never rewritten while a
  claim is held; identity migrations are recorded once per database in a new
  `schema_migrations` table so they cannot fire twice; and `ask_question` now
  reports *why* a park was refused instead of returning `needs_input: false` for
  every reason at once — the silent rejection that left an agent with no way to
  explain itself.
- **Accepting an `in_review` task now records who accepted it.** `resolve_task`
  validated the `human` identity and then threw it away — the store call was
  `resolve_in_review(id, now)` — so the one act in the system that can overrule
  an evidence-backed verdict was the only act leaving no trace. Measured over
  this repository before the fix: **57 of 101 `done` tasks rested on a
  `drift`/`needs_human`/zero-node receipt**, and who accepted any of them was
  unrecoverable from the ledger. ARCHITECTURE.md calls the conformance chain
  "the only trustworthy proof that the agents did the sanctioned work — every
  other signal is narration an agent can fabricate"; the override that outranks
  it was narration. Resolution now writes `resolved_by`, `resolved_at`, and
  `resolved_conformance_id` (the verdict being overruled, pinned by id so a
  later check cannot make it ambiguous), and appends a note naming what was
  overruled to the task's append-only thread. Existing rows keep `NULL`: those
  acceptances were not recorded and inventing a resolver for them would be the
  same defect in a new coat.
- A binary built *ahead* of the checkout is no longer reported as a stale build.
  The notice compared build sha against `HEAD` with a plain string inequality and
  no ancestry check, so any difference in either direction produced the same
  advice: "Rebuild and restart". Measured on 2026-07-30, the checkout the fleet's
  servers are compared against sat 599 commits behind `main`, so a binary built
  from `main`'s tip was reported stale on every `open_session` — and following
  that advice would have rebuilt from the older checkout and reverted an ingest
  guard merged minutes earlier. A warning whose remedy undoes the fix is worse
  than silence, because it gets followed.
  Staleness now requires evidence that the build is actually behind: when the
  build has `HEAD` in its history, the notice says the checkout is behind the
  binary and to update the checkout instead. A build genuinely behind `HEAD`
  still warns exactly as before, and an unanswerable lineage — git unavailable,
  or a commit this checkout does not have — is treated as ignorance rather than
  as proof the build is fine, so it keeps warning. Both cases still name the
  build sha, because "which build is answering" is the question the notice
  exists to answer.
- **A Known gap that reported 8 governed nodes now reports 161, and says when
  each figure was taken.** The entry claiming the conformance chain governed 8
  code nodes — none of them Rust — with 127 of 131 receipts covering nothing was
  measured at 03:37Z and was already wrong by 09:29Z: 161 governed nodes, 133 of
  them `.rs` under `crates/`, and 72 of 172 receipts covering zero nodes. The
  gap was real and another agent closed it within the day.
  Corrected rather than deleted, because the shape of the mistake is the useful
  part: the number was right when taken and stale within hours, and a Known gap
  that records a measurement without its timestamp keeps being read as current
  long after it stops being. Both measurements are now shown side by side with
  their times.
  It also names `scripts/binding-audit.mjs` as the way to re-measure, which
  already existed and which the original entry did not mention — leaving the
  next reader to rebuild an audit that was sitting in `scripts/`. As of 09:29Z
  it reports 131 of 136 source files bound and names the five that are not,
  every one of them recently added. That is the residual gap worth watching: a
  binding is applied to the tree as it was, so a new module arrives ungoverned
  and nothing says so.
  The half of the entry that is still true is re-verified and kept:
  `conformance-gate.mjs` still cannot run, because `.gitignore` still excludes
  the manifest it reads and nothing is tracked under `.lodestar/`.
- **A conflicted merge keeps the provenance of what it resolved.** Every merge
  commit was treated as noise on the grounds that its content already arrived on
  the branches it joins — true of a clean merge, and false of a conflicted one,
  where the resolution is genuinely authored. The cost was that reconcile-shaped
  work could not be certified at all: a reconcile's entire product *is* the merge
  commit, so the evidence window came back empty however much conflict
  resolution it contained, and `check_conformance` had nothing to judge.
  Git already draws the line in the right place. `git show --name-only` on a
  merge reports the combined diff — only files differing from *every* parent,
  which is exactly what the merge itself introduced. Measured across 25 merge
  commits in this repository, that set matched "differs from every parent" in
  25 of 25 cases and was empty for all 18 clean merges, so the parent count was
  never needed: an empty changed-file list already means the commit authored
  nothing. A clean merge is still skipped, one git call disappears from a hook
  that runs on every commit, and the claim about git's behaviour is now covered
  by a test against real git rather than a fake, because a clean merge wrongly
  ingested would attribute another agent's whole branch to whoever ran it.
- **The script test runner no longer hands git's hook environment to the tests
  it runs.** Git exports `GIT_DIR`, `GIT_INDEX_FILE` and friends to its hooks,
  and this suite runs from pre-push. Inherited by a test that drives git inside a
  temporary directory, those variables outrank `cwd`: git reads the fixture's
  files and writes to the *real* repository. A test doing exactly that committed
  its fixtures onto the branch being pushed and left the worktree checked out on
  a branch named after the fixture — with every symptom pointing at the test
  rather than at the environment. The runner now scrubs them once, so a test
  written later gets this for free; remembering per test is the discipline that
  failed.
### Fixed

- **A covering task is recognised again after a constitution amendment.** Every
  task touching governed code completed as `drift`, however correct it was, and
  `claim_task` / `governing_for_task` reported that nothing governed the change
  — so an agent was waved through on governed code and only found out at
  completion.

  A clause carried into a new constitution is re-issued as
  `goal:<slug>@constitution:vN`, while a task keeps naming the bare
  `goal:<slug>`. Coverage was decided by string equality on those two ids, and
  binding lookup by exact goal id, so from the *first amendment onwards* neither
  could ever match. Both now compare by slug — the identity a clause keeps
  across versions, which the amendment carry-forward and `diff_clauses` already
  use.

  The empty binding list was the worse half: it is indistinguishable from "this
  goal governs no code", so the failure reported itself as a clean bill of
  health. A verdict that comes back the same for every input has stopped
  carrying information, which is the failure mode this project keeps finding.

  Regression tests cover a clause that has actually been through an amendment
  (a freshly adopted v1 clause id is bare, which is why this stayed hidden until
  v2), and the mirror case: a clause from a different goal is still not
  coverage, so the fix cannot pass everything.
### Fixed

- **A delivered branch can be reconciled again.** Completing a task releases its
  claim, and publication requires one — so once a task was done, its pull
  request could never be brought up to date. `main` moved, the branch went
  stale, and the delivery queue stepped over it forever. #168 needed hand
  rescuing three times, and each rescue invented a throwaway task purely to get
  past the gate. Minting a task per republish is exactly how six duplicate tasks
  reached the board.

  A task now records the branch it was claimed on, so a delivered branch is
  already attributed; re-attributing it to a fresh task records a fiction.
  `canonical-push` publishes it as a reconciliation and says whose work it was.

  Deliberately narrow: **every** new commit must be a merge. A reconciliation
  merges the base in and nothing else, so this cannot decay into "finish a task,
  then push anything to that branch forever" — the moment real work appears, a
  claim is required again. That case is tested, because an exemption without one
  is a bypass wearing a fix's clothes.
- A file saved in any worktree of this repository now reaches the graph under its
  canonical repository-relative id, instead of being refused because it came from
  a checkout the server was not rooted at. Measured 2026-07-30: of the 291
  `ingest_file` calls after the ingest guard landed, **203 were refused (69.8%)**,
  naming paths like
  `c:/Users/.../MindLeak-rustimports/scripts/silent-knowledge.mjs`. The graph was
  clean of duplicate identities partly because those files were not arriving at
  all — the guard had converted silent corruption into visible loss, and the loss
  was larger than the corruption.
  Every worktree of a repository shares one graph (ADR-0038), so a path under any
  of its worktrees is unambiguously the same file. The server now resolves those
  roots with `git worktree list` and treats every one of them as a candidate when
  placing a path; the longest match wins, and a root only matches on a path
  boundary, so `.../MindLeak` never swallows a path under `.../MindLeak-build`.
  Commit and execution sensors place their changed files the same way, so a
  commit touching a sibling checkout no longer drops those files either.
  Rooting each window at its own worktree (ADR-0073) remains the cheaper, more
  direct fix. This makes the answer the same whichever window did the saving,
  rather than leaving correctness to an operational habit. A path under no root
  of this repository is still refused, and when git cannot answer the behaviour
  degrades to the previous single-root placement rather than to a wrong id.
- **A green local test run meant half of what it said.** `script-tests.mjs` runs
  `scripts/*.test.mjs` under `node:test`. A second suite —
  `editors/vscode/scripts/*.test.mjs`, run by vitest from the extension job —
  covers the same scripts, and the runner neither ran it nor mentioned it. A
  full green run therefore reported success over 18 of 33 test files while
  naming none of the gap.
  It was acted on: the claim-gate and completion-offer guidance fix passed every
  local assertion, then failed CI on the mirrored ones — twice, across two pull
  requests, on work that was correct. The mirrors asserted the retired verbs in
  the message text, and one of them builds a fake MCP server that answers by
  tool name, so a collapsed verb reaches a fixture that replies to nothing.
  The runner now names what it does not run and the command that does run it.
  Failing instead was considered and rejected: driving vitest from here would
  make a pre-push hook depend on the extension's `node_modules`, which is not
  always installed. The defect was never the missing execution — it was a green
  result that quietly meant "half", and one honest line repairs that.
  The check moved into `scripts/script-suites.mjs` so it can be tested at all:
  importing the runner executes it, which is why this had no test and why it
  went unnoticed. A test also asserts the mirror is still discoverable, so if it
  moves the notice cannot silently stop appearing — the same rot it exists to
  report.
- **Every lesson an agent recorded on completing a task was invisible, and the
  tool that could have shown you was never advertised.** `complete_task` stored
  its `learned` note with the bare string `task:{id}` as provenance. That is not
  JSON, so it parsed to no referenced nodes — and referenced nodes are the only
  thing `apply_knowledge_advisory` matches on. Every lesson was written,
  counted in `lodestar_stats`, and delivered to nobody. Measured on this
  repository the moment it became measurable: **34 of 35 active knowledge
  records referenced nothing.** The note now carries the nodes the work changed,
  so a lesson learned while changing a file reaches the next agent who changes
  that file — the moment it is useful, and the moment it was written for.
- **`active_knowledge` is now advertised, and reports whether each record can
  ever surface.** The tool dispatched all along and appeared in no `definitions()`
  list, so from the tool surface the knowledge base looked write-only: record,
  promote, reconfirm, prune, and no way to see what was already known. It now
  takes an optional `node` (what is known about the thing you are about to
  change) or `contains` filter, and every entry reports `surfaces`, because a
  record that names no nodes is stored and silent and that should not have to be
  inferred from an empty array.
- **A tool the server answers to must be a tool it advertises.** A guard walks
  every dispatch block and fails on any name absent from the advertised list.
  This is the mirror of the undeclared-argument guard: there the contract asked
  for something it never mentioned, here the server answered to something it
  never mentioned, and both fail the same quiet way — the code is right, the
  advertisement is wrong, and nothing breaks loudly enough to be found.
- **A lesson that names no code now reaches the goal it was learned under.** The
  conformance advisory matched recorded knowledge on referenced nodes and
  nothing else, so a record whose evidence carried no `nodes` array was stored,
  counted, decayed, and structurally incapable of reaching any agent. Measured
  over the whole ledger: 191 records, 124 carrying nodes, **67 silent**. They
  were not marginal notes — several were among the most expensive lessons the
  repository had, and were then re-learned from scratch, at length, by agents
  with no way to know they existed.

  The reach is recovered by reading the provenance those records already carry,
  not by rewriting them: 55 of the 67 still name the goal they were learned
  under, or a task from which the goal is reachable. The advisory gained a
  second, narrower matching dimension for exactly that case. Nothing is copied
  forward and no possibly-stale claim is restated, which is what made rewriting
  the records the wrong repair.

  Three details decide whether this helps or merely adds noise:

  - It is **capped** at three lessons per check, ranked by effective weight.
    ADR-0072 established that an advisory firing on almost every task carries no
    information, and a goal accumulates everything ever learned serving it — 20
    and 18 silent records sit under the two busiest goals here.
  - Goal identity compares by **slug**, so a constitution amendment does not
    sever a lesson from the intent it belongs to.
  - Task provenance is read in every spelling it was written in — a JSON field,
    a nested array, or the bare string that is not JSON at all — because a
    reader that understood only one shape would silence the records written in
    the others.

  The advisory still only informs. It adds findings and can never harden a
  verdict, emit a violation, or downgrade an aligned one. No LLM joins the read
  path. Twelve records that name nothing at all remain undeliverable and are
  recorded as still open.
- **A lost claim says why, and what to do about it.** `claim_task` returned a
  bare `won: false`. The reasons a compare-and-swap can miss call for opposite
  responses — wait for a live lease, pick different work because the task is
  finished, unblock a predecessor, or rebuild a stale server binary — so one
  boolean covering all of them is not terse, it is unusable. It also meant
  `scripts/claim-gate.mjs` had to exist: a whole diagnostic written to
  reconstruct, after the fact, what the plane knew at the moment it refused.
  A refusal now names the holder and the remaining lease, points at
  `recover_claim` when the lease has lapsed, distinguishes finished work and
  missing work from contended work, and — the expensive one — recognises a
  claim held under a pre-session identity as a stale binary (ADR-0054) rather
  than a live claim, saying so and warning that re-claiming will not help.
  `owner`, `status`, `lease_expires_at` and `blocked_by` come back alongside,
  so a caller can branch on the outcome instead of parsing prose.
- **The mandatory pre-flight is bounded, so it can actually be read.**
  ADR-0066 put `check_overlap` on the before-you-write checklist and had it
  carry the impact radius. Two later changes landed on top of that — Rust
  `mod`/`use` extraction, which gave `.rs` files real cross-file structure for
  the first time, and a re-ingest pass that populated all of it at once. Each
  was right on its own; together they made the thing every agent is required to
  read too large to read.
  Measured 2026-07-29 on `crates/lodestar-mcp/src/tools/mod.rs`, a single path
  returned **196 nodes over 295 edges — 351 KB**, with 185 of those nodes
  carrying their full text. `impact_radius` traverses at zero minimum weight,
  depth two, with no node cap; that was harmless while Rust files had almost no
  cross-file edges, and stopped being harmless the moment they did.
  A decision aid that displaces the decision fails the same way an unread one
  does, which is the failure ADR-0066 was written to fix.
  The pre-flight now carries the 32 most relevant nodes without their content,
  only the edges among them, and `impact_total` so a trimmed answer cannot be
  mistaken for a complete one — the same reason `unknown` is reported separately
  from an empty impact. Ranking by traversal score keeps the cut meaningful
  rather than arbitrary. Same path, after: **47 KB**.
  The cap matches the existing hard cap on `working_set`, which bounds the same
  kind of view for the same reason. `get_impact_radius` is unchanged for callers
  that genuinely want the whole neighbourhood.
- **A rename left its verbs unbound, and unbound verbs let a caller name
  itself.** `requires_session`, the optional-session list and the heartbeat list
  are keyed by tool name, and the ADR-0059 task and design collapses moved the
  names out from under all three. Ten of twenty-three session bindings pointed
  at tools that no longer existed — `claim_task`, `complete_task`, `pause_task`,
  `resume_task`, `release_task`, `renew_lease`, `recover_claim`, `ask_question`,
  `pending_questions`, `register_design` — so `task_claim`, `task_transition`
  and `constitution_decide` bound no session at all. That is worse than
  unauthenticated: `apply_session_contract` strips `agent` only from a bound
  tool, so all three advertised `agent` for the caller to supply, and the
  server took it. Taking a claim, completing work and changing constitutional
  law could each be performed in another agent's name, which is the one thing
  resolving a session exists to prevent; the ledger's attribution was
  unenforced for the whole window. The heartbeat list broke the same way and
  more quietly: renewal-on-activity (ADR-0052) stopped firing for reading a
  task's scope and for asking or answering a question, so a lease could lapse
  while its owner was working and the next call told the rightful owner the task
  was not held by it. All three tables are now read against the call as it will
  actually be dispatched, so a rename carries its behaviour with it, and the two
  that had to distinguish acts within a collapsed cluster name the act rather
  than the tool. Two guards make the class un-repeatable: no advertised tool may
  declare `agent`, and every tool named by a server-side table must be one that
  is actually advertised — so the next rename fails a test instead of silently
  unbinding the verb.
- **Three reports that could not be run now run.** `board-health`,
  `stranded-report` and `design-audit` each resolved the server binary with
  their own release-only path instead of the shared `resolveServer`, which
  honours `LODESTAR_MCP_BIN`, accepts a debug build, and returns nothing rather
  than a path that is not there. On a debug build — the normal state for a
  developer — `board-health` died with an unhandled `spawn ENOENT` and
  `stranded-report` with it. So the two reports written to explain the board's
  stranded claims could not be executed by most of the people they were written
  for, which is why the board's state kept being reconstructed by hand. They now
  resolve like every other script and say plainly when no binary exists. Running
  them immediately separated four lapsed claims into two whose shipping commit
  can be named and two that look genuinely unfinished — a distinction that had
  been made by guesswork.
- **A guard was watching two names the server had stopped answering to under
  their own contract.** The test that proves the session envelope is tolerated
  rather than validated as a tool argument — the regression that once made the
  VS Code extension report `disconnected` instead of `ready_empty`, and which
  only the Extension Host smoke noticed — asserted over `board` and
  `design_board`. ADR-0059 retired both. The server no longer advertises them
  and answers them only through the deprecation table, so the guard was
  exercising the *aliases* while the advertised readiness path (`task_query`,
  `design_query`) went unwatched: the same regression could have returned
  through the new names without failing anything, and when the aliases are
  removed the guard would have vanished with them.
  Its own comment had predicted this — *"a whitelist entry is easy to drop in a
  refactor, and the unit that catches it must be the call the client actually
  makes"* — and the whitelist had quietly stopped being that call. It now
  asserts each name it mentions is a tool the server actually advertises, so
  the next rename fails here instead of hollowing the guard out in silence.
  This is the same defect class as `requires_session`: a list keyed by tool
  name that a rename left pointing at nothing. Finding it twice by hand is what
  the fence is for.

- **Two more retired names were still in live guidance and a second guard.**
  Reconnecting with paused work advised the owner to *"Call resume_task"* — a
  verb the server no longer advertises, offered to an agent at exactly the
  moment it is trying to get back to work. It now names `task_transition` with
  `to="resume"`, and says which argument answers a `needs_input` task instead.
  The guard proving an offered session sharpens the overlap read while its
  absence never refuses (ADR-0024) was driven through `check_overlap`; it now
  goes through `task_query` with `view="overlap"`, because the alias resolves
  to the same handler and so proved only that the deprecation table works.
- **A sibling worktree is not a foreign path, and structural ownership now
  follows a merged identity.** Two defects, one symptom: 43 of 247 tracked files
  could not be re-ingested at all, so every future extractor improvement would
  have missed them silently.
  Repair was prefix-scoped, which assumes every worktree eventually hosts a
  server that heals its own ids. A worktree an agent works in without ever
  starting a server there leaves its ids orphaned permanently. Since ADR-0038
  gives every worktree of a repository one shared graph, an absolute id written
  from a sibling checkout names the *same file* as the repo-relative id — so it
  is now merged into it, whichever checkout spelled it.
  The warrant is evidence, not a guess about where a checkout begins: the merge
  target must be a repo-relative id **the graph already holds**, taken as the
  longest matching suffix so a full path always beats a bare filename that
  happens to collide. A path with no such twin is still left exactly alone, so
  repair never invents a file — the rule the prefix pass was protecting, and the
  existing `repair_is_idempotent_and_leaves_foreign_paths_alone` test, both
  stand unchanged.
  The second defect only surfaced when the first was fixed and the files stayed
  blocked. `edges.owner_id` records which artifact owns a structural snapshot
  (ADR-0007), and merging a node rewrote the edge *endpoints* but never the
  *ownership* — a hole that predates this change and affected same-root repairs
  too. Ownership is not an endpoint, so it survives the node it names being
  deleted, and `replace_structure` then refuses every later ingest of that file
  with "structural edge is owned by <absolute id>, not <relative id>". With the
  absolute node already collapsed there is nothing left for a node-level repair
  to find, and the file is permanently un-re-extractable. Ownership is now
  carried across a merge, and reclaimed separately by scanning ownership rather
  than nodes, so the already-orphaned state heals too.
  Repair also no longer needs a declared workspace root to do this. The prefix
  pass still does and is still skipped without one; collapsing ids whose twin
  the graph already holds does not, and a server that never declares a workspace
  was exactly the case that left sibling ids stranded, because no root ever
  matched them.
  Verified against the live graph: all five sampled blocked files now ingest.
- **A stale server now says so to the agent using it, not only to a log the
  agent cannot see.** `build_notice` already decided this correctly and
  `BuildNotice` already carried a `stale` flag — but the answer went to stderr
  at startup, and an MCP client shows that to nobody. The comment above the
  check had already recorded the same failure one level up: the version *"has
  always been reported at `initialize`; nobody compared it, and a two-day-old
  local build cost a night of misdirected debugging"*. Moving it to stderr fixed
  where it was written, not whether it was read.
  Measured over a full session on 2026-07-29: every call ran against a binary
  built from `f9a549c4211a` while the checkout had moved on, and nothing in the
  tool surface said so. The consequences were diagnosed as tool defects rather
  than as a stale build — `propose_amendment` and `amend_constitution` were
  "missing from the tool list" because the binary predated them, and the sha
  validation that refuses a fabricated commit id was not running at all while
  every call still looked normal.
  `open_session` now carries the notice, on both planes, because it is the one
  call every agent already makes before anything else — the same reasoning that
  put commit provenance on the commit rather than on remembering to record it.
  Only when the binary is genuinely behind the checkout it serves: a current
  build says nothing, so the field keeps its meaning instead of becoming a line
  to scroll past. It reports and never refuses, because a server that stopped
  serving because it was behind would halt a fleet mid-flight, which is worse
  than the staleness it is complaining about.
- **A test server that raced its client turned `main` red on Windows only.**
  `response_decode_failure_opens_the_circuit` checks that a 200 response with a
  body that is not JSON opens the circuit breaker. Its fake endpoint issued a
  single `read` of the request and then let the socket drop. Both halves are
  wrong, and only on Windows does it show. One `read` can return just the
  headers, because a POST body may arrive in a later segment; and dropping a
  socket that still holds unread inbound data makes Windows answer with RST
  rather than FIN, discarding the response the client has not yet read. The
  client then reported a transport failure — `An existing connection was
  forcibly closed by the remote host (os error 10054)`, or `10053` — instead of
  the decode error the test names, so the assertion failed for a reason that
  had nothing to do with the behaviour under test. Measured before the fix: 4
  failures in 12 local runs, green on the ubuntu CI leg and red on
  windows-latest, three consecutive red builds on `main`, and unrelated pull
  requests blocked behind it. The endpoint now reads the request in full,
  honouring the declared `Content-Length`, writes and flushes the response, and
  shuts the write side down before dropping, so the client always gets to read
  what was sent. Verified by running the test 30 times rather than once: 30
  passes, against 8 of 12 before. A single green run cannot tell a fix from
  luck, and for a race that is the only evidence that means anything.
  Production code is untouched — the change is confined to the `#[cfg(test)]`
  helper, so what the breaker does in the field is exactly what it did before.
### Fixed

- **A worktree created after a server started is no longer invisible to it.**
  PR #239 made every worktree root a candidate when placing a path, which
  stopped most ingest calls being refused — but the root set was resolved once,
  at engine construction, so it was frozen for the life of the process.

  This fleet creates worktrees hourly, so a frozen set decayed from the moment
  it was resolved, and fastest exactly when the fleet was busiest. Observed
  2026-07-30: servers started at 03:55Z refusing paths from four worktrees that
  appeared later in the same session.

  A path that lands outside every known root now re-resolves the set once and
  retries the placement, so a worktree born after startup is picked up without
  restarting the server. The refresh rides on the failure that needs it rather
  than a timer, and is bounded — at most one per interval, never more than one
  in flight — so a genuinely foreign path from a misconfigured sensor cannot
  make every refusal pay for a git subprocess.

  A path under no worktree of this repository is still refused: the retry
  changes *when* the answer is computed, never what counts as belonging. The
  refresher is injected, like the roots themselves, so the core still does not
  spawn git and a test can make a new root appear without one.
- **ADR-0059 now says what the ledger decided nine hours earlier.**
  `design:0059-the-tool-surface-is-a-vocabulary` was accepted by `monk-eee` and
  materialized — it is what spawned the four tool-collapse tasks — while the ADR
  file still read `Status: Proposed`. Nothing reconciles those two directions:
  the Design Board sync reads files into the ledger, and no step writes a ledger
  decision back into the file.
  That gap stopped work, not just confused a reader. Surveying the board the
  same day, an agent read the file, concluded the four collapse tasks were
  implementing an undecided design, and declined to claim any of them. The file
  is now `Accepted` and names the decision that made it so.
  Known gaps also records what the sweep behind this found: **69 ADR files on
  main, 63 registered as design items.** `0063`, `0064`, `0066`, `0067`, `0068`
  and `0069` have no design item, which is why `design_board` reads empty — the
  undecided ADRs are precisely the ones missing from it. Four of those six
  assert `Status: Accepted` in the file without a recorded decision, so
  registering them is a maintainer's call rather than a sweep.
- **The resolvable ADR cross-links reach the right file again.**
  Renaming an ADR orphaned every inbound `ADR-00NN` link — the href kept the
  target's `00NN-old-slug.md` name as it was when written, and 404'd once that
  name changed. Twelve such links across nine ADRs now point at the real `00NN-*.md`
  file for their number (href only; the decisions and the parenthetical
  descriptions are untouched), and ADR-0031's `ARCHITECTURE.md` link becomes the
  correct `../ARCHITECTURE.md`. Three references to a phantom `ADR-0045 "armed
  means finished"` are left as-is and flagged for a maintainer: no ADR ever bore
  that name (0045 is "a fleet is a distributed system"), so pointing them
  anywhere would fabricate a target — that needs the author's intent, not a
  mechanical rewrite.
- **The ADR files and the design ledger agree again.** `make design-audit`
  reported seven items of drift: five ADRs the ledger had never seen (0054,
  0055, 0060, 0061, 0062) and two whose file said `Proposed` while the ledger
  recorded them accepted by a person (0057, 0058). An unregistered ADR is a
  decision the ledger cannot reason about — it never appears on the design
  board and cannot be reconciled — and a file that disagrees with the ledger
  means one of the two is lying about whether a decision was made. All five are
  registered and the two status lines now match the ledger. Two remain
  deliberately unresolved: 0054 and 0055 claim `Accepted` in their files with no
  decision recorded, and inventing a decider for them is exactly what the check
  exists to prevent.
- **Advertised task verbs keep their session contract.** The task-cluster
  collapse renamed twenty-six tools to `task_claim`, `task_transition` and
  `task_query`, but the server policies that resolve session identity and renew
  an active lease still recognized only the deprecated names. The replacement
  calls therefore reached dispatch without their registered agent:
  `task_claim` could not claim anything, session-bearing transitions such as
  `complete` were refused, and overlap queries lost their branch context. The
  server now canonicalizes deprecated calls before applying one
  operation-aware policy. Every ownership step is session-bound; only the
  transition and query variants that need identity require it; overlap remains
  an anonymous advisory when no resolvable session is offered; `answer` remains
  open to anyone as ADR-0046 requires; and heartbeat behavior is identical
  through either vocabulary during the deprecation window. The advertised
  schemas expose `session_id` without accepting a caller-asserted `agent`.
- **`advise` no longer renews a lease, so ADR-0029 and ADR-0052 stop
  contradicting each other.** Renewal-on-activity (ADR-0052) lets any call that
  names a task prove its owner is still working, which is what stops a claim
  expiring during a long build. Its own consequences section flagged one member
  of that list as unsettled: *"`advise` should be excluded, or ADR-0029 amended —
  this decision does not get to quietly redefine another one."* The
  implementation included it and ADR-0029 was not amended, so `advise` — which
  ADR-0029 documents as evidence-free and state-free, recording no verdict and
  changing **no task state** — was writing `lease_expires_at`. Both ADRs still
  read as authoritative while the code could only satisfy one. `advise` is now
  excluded, answering the open question in the direction that keeps the existing
  contract intact; the cost is negligible, because an agent calling `advise`
  before it edits is about to call something task-bearing anyway. A test guards
  the array, so re-adding `advise` fails until someone amends ADR-0029 on
  purpose and with reasoning.
- **An advisory nudge now says that it is the reason.** Learned knowledge may
  attach an advisory and move an otherwise-aligned verdict to `needs_human` so a
  person looks — ADR-0022 §4, deliberately bounded so a decaying regularity can
  never hard-fail correct work. But the nudge changed the verdict and left only
  lines labelled `advisory:`, which read as information rather than as the
  cause. Every other route to `needs_human` pushes a finding naming its own
  reason; this one did not, so a receipt whose every other signal was positive —
  coverage confirmed, provenance intact, no drift, no lapse — reported an
  inexplicable failure, and the honest reading was unavailable to whoever held
  it. The nudge now records itself, naming the knowledge ids responsible and
  stating that nothing else in the evidence is a problem signal. The rule is
  unchanged; only its silence is fixed. A verdict knowledge did **not** move
  still carries no nudge line, so a drift is never blamed on an advisory.
- An amendment now records where each clause went and carries the work with it.
  Superseding a clause used to leave `superseded_by` NULL, and because an
  amendment renames every clause it carries forward (`goal:{slug}@{version}`),
  nothing could follow the rename: code bindings and open tasks kept naming a
  clause no active constitution contained. `amend_constitution` now names the
  successor by slug and moves goal/code bindings and non-terminal tasks onto it
  in the same transaction. Terminal tasks keep their original clause, because a
  finished audit must keep naming what it was judged under.
- A migration reconnects clauses already stranded this way. On this repository
  that moved 156 bindings and 53 tasks onto the active constitution and recorded
  26 successors, leaving all 178 finished tasks untouched. A task held under an
  unexpired lease is skipped: its goal is what conformance judges the holder's
  evidence against, so moving it mid-claim would change the rule beneath someone
  doing the work (ADR-0063). Those move at the next amendment instead. Recorded
  in ADR-0068.
- **Conformance had stopped affirming anything, and the receipts said so if you
  counted them.** Consolidated knowledge could nudge an otherwise-`aligned`
  verdict to `needs_human` whenever any active knowledge node merely
  *referenced* one of the evidence's changed nodes (ADR-0022 §4). Knowledge only
  accumulates, so the referenced set only grows, and the nudge became
  unconditional.
  Measured on the live board on 2026-07-30: of 190 `done` tasks only **58 (31%)**
  carried a receipt that affirms the work — and the shape is a collapse, not
  accumulated debt. The fleet affirmed **28 of 28** completions on 23 July and
  **1 of 13** on 30 July. That one survivor earned it because `advise` reported
  *"no active clause governs this change"*, so nothing could fire: **the only
  reliable way to get an affirming receipt had become changing code that nothing
  governs.**
  It was not cosmetic. A `blocked_by` successor opens only on an *aligned*
  completion, so a permanent cap froze dependent work. And a receipt whose only
  substantive finding is positive, capped anyway, teaches every reader that the
  verdict does not track the work — which is how a gate stops being read.
  The advisory findings still attach, because showing an agent the relevant
  lesson at the moment of change is the whole value of ADR-0022. What is gone is
  the claim that relevance is evidence of a problem. ADR-0060 item 2 already held
  the line this crossed: only a positive signal of a *problem* may downgrade a
  verdict. Recorded as ADR-0072, amending ADR-0022 §4, with the measurement as
  its evidence.
  The trade is deliberate and worth naming: a second look is no longer forced. A
  class of knowledge that genuinely should stop a completion now has to say so as
  a constraint or invariant clause with a declared consequence — machinery that
  already exists and that carries an attributable human decision.
- **Amending the constitution no longer disarms the controls enforcing it.** A
  clause copy takes a new id (`goal:{slug}@{version}`), and controls store the
  clause id they were registered against, so every amendment used to leave its
  controls pointing at a row that had just been superseded. Nothing refused
  that. The orphaned control went on accepting observations and went on
  answering `pass` and `fail` — only the effective consequence quietly
  collapsed to `advise`, and `clause_controls` reported the live clause as
  unguarded. A control that has stopped enforcing reads exactly like one that
  works, and it happened at the moment somebody was strengthening a rule, which
  is the worst possible time to silently stop enforcing it. Active controls are
  now carried across with the clause inside the amendment transaction, matched
  on slug rather than on the outgoing version's ids — so a control stranded by
  an earlier amendment is adopted by the next one rather than staying orphaned
  forever. Retired controls are deliberately left where they are: they are a
  record of what once enforced a rule, and moving them would rewrite that record
  onto a clause they never guarded.
- **The merged-branch audit failed on work that had fully landed.** It compared
  ancestry, so a squash or rebase merge — which lands every line under a new
  commit id — was indistinguishable from a branch whose commits never merged at
  all. It then reported the work as lost and instructed the reader to open a
  follow-up pull request for changes already on `main`, which is not something
  anyone can do. That is the failure mode worth naming: an audit with no green
  move available gets switched off, and switching this one off would take the
  check that catches genuinely lost work with it. It also cost real time before
  it was fixed — PR #205's work was recorded in durable knowledge as lost, and a
  follow-up to restore 245 lines that were already on `main` was queued against
  the one file three agents were editing. `git merge-base --is-ancestor` answers
  a question about commit identity, not about whether the work arrived. The
  audit now uses `git cherry`, which compares patches: a commit whose changes
  never reached the base still fails the build, while one that landed under a
  rewritten id is reported as landed-but-rewritten and does not. Merge commits
  are in neither list, since merging the base into a branch carries no work of
  its own and reporting it as lost was noise obscuring the one real finding.
  Nothing weakens: the report says plainly that a squash or rebase merge
  destroyed a commit identity AGENTS.md asks to keep, and points at the
  repository setting that prevents it, because the only durable fix is at the
  merge button rather than in an audit that runs afterwards. The existing suite
  in `editors/vscode/scripts/merge-audit.test.mjs` gains the squash case and the
  merge-commit case, and its four original tests are kept unchanged so the
  behaviour that was already correct is proven not to have regressed.
- Ingesting a file whose path cannot be made repository-relative is now refused
  instead of quietly creating a second identity for it. Every worktree of a
  repository shares one graph (ADR-0038), and `repo_relative` returns a path it
  cannot place untouched — correct for a helper, wrong for a node id — so a file
  saved in a sibling checkout arrived still absolute and became
  `artifact:c:/Users/.../MindLeak-build/crates/x.rs` alongside
  `artifact:crates/x.rs`. Splitting a file's identity splits everything derived
  from it: reinforcement decays corroborated signal like a one-off (ADR-0005),
  `check_overlap` never collides two agents on one file, a governance binding
  covers only one spelling, and recall returns the same file twice.
  Measured on the live graph on 2026-07-29:
  `crates/lodestar-mcp/src/tools/mod.rs` held 117 structural edges under its
  absolute id and 43 under its relative one. Those rows were unreachable rather
  than merely stale — `replace_structure` matches on the relative `owner_id`, so
  re-ingesting the file could never see them. `repair_workspace_paths` still
  merges duplicates that already exist; this stops new ones being made.
- **A won claim now reports the evidence window it opened.** `complete_task`
  refuses evidence whose `started_at` precedes the claim's `claim_started_at`
  with *"evidence interval falls outside the live claim"*, and no tool returned
  that value — `claim_task` gave back only `won` and `governing`, and
  `task_scope` only `paths` and `symbols`. The one number needed to construct
  acceptable evidence was unobtainable, so an agent had to guess a `started_at`,
  and a wrong guess read as a policy refusal rather than a missing accessor.
  `claim_task` now returns `claim_started_at` and `lease_expires_at` on a won
  claim, and reports neither on a lost one (ADR-0060 decision 4).
- **Eleven closed Known gaps no longer read as open work.** Current code and
  regression coverage already prove the design-status, ADR parsing, duplicate
  goal, Design Board isolation, task lifecycle, embedder thread-safety, and
  actionable-task fixes. The optional recall failure was repaired operator
  configuration, not a product defect. A fresh valid `graph_multi_hop_query`
  also cleared that tool's stale failure state in telemetry. Their fragments
  are deleted from the open-gap source of truth; unresolved limitations remain.
- The Known Gaps validator now rejects terminal `FIXED`, `RESOLVED`, or
  `CLOSED` headings unless they explicitly name an `OPEN` residual. The catalog
  no longer presents completed work as actionable debt: fully closed fragments
  were deleted, while partial fixes were retitled around the limitation that
  still remains.
- **Duplicate node identities are collapsed, and the survivor keeps the history
  both halves earned.** Making paths repo-relative stopped new splits; it did
  nothing about the 590 files already living under two identities. A repair pass
  now rewrites node ids that spell their path absolutely under the served
  checkout onto the repo-relative id, **merging** rather than choosing a winner:
  reinforcement counts add, weight carries the write path's own `+0.05` per
  reinforcement, the earliest `first_seen` and latest `updated_at` survive, and
  the longer-lived half-life follows the more recent edge. Picking a winner
  would have been the expedient choice and would have silently discarded real
  corroboration — the thing signal-weighted decay (ADR-0005) exists to reward.
  Measured on this repository: **871 absolute ids across 8 worktrees collapsed to
  0, and 590 duplicated files to 0**, taking the graph from 6,144 nodes to 5,106
  without losing an edge's history. The pass runs at startup, is idempotent, and
  is scoped to the checkout the process serves, so each worktree heals its own
  ids and the graph keeps healing if any producer ever regresses — one worktree
  running an older binary during the migration was found and healed exactly this
  way. A repair failure logs and never blocks startup: a graph with split ids is
  still usable, and refusing to start would be the larger outage.
- **Creating an identical task in the same second now returns a typed domain
  error instead of leaking SQLite.** Task ids are derived from goal id, title,
  and a whole-second timestamp. Two identical creates inside that second used
  to let the second `INSERT` hit the primary key and return `UNIQUE constraint
  failed: tasks.id` — an implementation detail for what is plainly a duplicate
  request. `create_task_after_on` now checks the derived id before dependency
  validation or insertion and returns `LodestarError::Invalid`, identifying the
  existing task and telling the caller to reuse it or choose a distinct title.
  The first task remains unchanged and no second row is written. A focused
  regression test was proven red against the previous implementation and green
  with the pre-check.
- **`evidence_for` refuses an empty window instead of returning nothing as
  evidence.** Measured on the board: forty audits carried *"evidence contains
  no provenance-bearing mutation"*, and sixteen of them were raised **after**
  the argument guard that was supposed to have fixed that cause — by two
  different agents. Every one of those bundles was completely empty: no
  commits, no changed nodes, no executions, no provenance. The misspelt
  argument was *a* cause, not *the* cause; the dominant one is asking for
  evidence over a window nothing was ingested into, receiving a well-formed
  envelope containing nothing, and submitting it. Conformance then records
  `needs_human`, which reads as "a human must judge this" when in fact nobody
  can — the work was never recorded, and no amount of adjudication will
  conjure it. The call now fails with the window, the agent, and the remedy
  (`ingest_commit` with `changed_files`, or `ingest_execution`) rather than
  succeeding emptily. A window that caught real work is unaffected.
- **The VS Code extension no longer depends on Lodestar's deprecated tool-name
  compatibility window.** Design workflows now call `design_register`,
  `design_decide`, `design_promote`, and `design_query`; task workflows call
  `task_claim`, `task_transition`, and `task_query`, with the former verb
  encoded explicitly as `step`, `to`, or `view`. The migration covers board
  refresh, evidence completion, question handling, lease changes, overlap
  checks, and every Design Board operation while leaving MindLeak's separate
  `check_overlap` tool unchanged.
  A TypeScript-AST regression audits every Lodestar `callTool` site, rejects
  retired aliases and dynamically constructed tool names, and verifies that
  each clustered call carries its discriminator. This includes the former
  runtime-only `` `${action}_task` `` pause/resume path that literal searches
  could not see.
- **Passive Git capture now distinguishes checking out work from authoring it.**
  The sensor inferred intent from ancestry: if the new HEAD named the previous
  HEAD as a parent, it was ingested. That misattributed a checkout to a
  descendant branch as a commit by whoever viewed it, while event ordering
  could lose an explicit non-linear commit such as amend behind an in-flight
  state refresh.
  The sensor now tracks branch name as well as commit and consumes the built-in
  Git API's explicit `onDidCheckout` and `onDidCommit` events. A checkout is
  remembered without attribution; the next real commit on that branch is
  captured. An explicit commit upgrades a state refresh already in flight, so a
  non-linear commit is not dropped as a duplicate. A state-only non-linear move
  remains un-attributed and becomes the next baseline, because it may be reset,
  rebase, or checkout and attaching it would be worse than a visible gap.
  Eight focused tests cover descendant checkout, first commit after checkout,
  coalesced branch creation and commit, amend/event ordering, terminal-style
  linear advance, and state-only non-linear movement. The new tests fail four
  of eight against the previous implementation and pass eight of eight with the
  fix.
- **An installed binary now says which build it is.** The startup notice only
  ever spoke when the running binary lived inside the workspace it served, on
  the reasoning that an installed release is not built from that checkout so a
  difference means nothing. That rules out calling it *stale* — it does not
  excuse saying nothing at all. The VS Code extension binaries are the ones the
  fleet actually runs, and they reported no identity, so three servers served a
  build predating a merged fix for most of a day, deciding conformance verdicts
  with it, while every surface read healthy. An out-of-workspace binary now logs
  the sha it was built from and explicitly does not claim staleness; a binary
  inside the workspace keeps the existing comparison. `stale_build_notice` is
  renamed to `build_notice` and returns a `BuildNotice` carrying a `stale` flag,
  so a genuine staleness warning stays a warning and identity is logged as
  information — a notice that cries wolf is how the real one gets ignored.
- **Knowledge that can never be read now says so when it is written.** The
  conformance advisory matches recorded knowledge on referenced nodes and
  nothing else, so a record whose evidence carries no `nodes` array is stored,
  counted, and permanently unreachable — it can never arrive in front of the
  agent it was written for. Nothing reported that at the point it happened.
  `active_knowledge` already exposes a `surfaces` field, but reading it requires
  already suspecting the problem, and an agent recording a lesson for a
  colleague has no reason to suspect anything: the call succeeds, returns an id,
  and looks exactly like one that worked. Measured before this landed, 3 of 17
  active records were invisible, among them one recording the cost of skipping
  the mandatory `advise` pre-flight — written precisely so the next agent would
  not repeat that mistake, and structurally incapable of reaching them.
  `record_knowledge` now reports `surfaces` in its own response, and when it is
  false it says which field is missing and what to put in it. Write time is the
  only moment this is worth saying, because it is the only moment the caller
  still has the node ids to hand; afterwards the information needed to fix the
  record is gone along with the context that produced it. The record is kept
  either way and is not refused, since losing an agent's stated lesson to a
  formatting mistake is worse than storing one that cannot yet be matched.
- **A lapsed claim no longer buries the work someone is actually holding.** The
  Work board ranked rows by stored status, so a claim whose lease expired nine
  hours ago sorted identically to one being worked on right now. One session
  left fifteen such rows behind in a day and the board became unreadable: three
  live tasks scattered among twenty-five dead ones, distinguishable only by
  reading a timestamp on every row. An expired claim is claimable — the store's
  compare-and-swap already admits `status = 'claimed' AND lease_expires_at <
  now`, and the row already described itself as "Claim expired · Ready" — so it
  now ranks as ready work, below tasks nobody has started, and live claims sort
  to the top where they belong. Nothing is reaped, rewritten or transitioned to
  achieve it: expiry is a function of `lease_expires_at` and the clock, derived
  at render time the way effective edge weight is derived at query time.
- **Maintenance-runtime tests now wait for worker progress instead of polling
  SQLite against a two-second wall clock.** A test-only event queue on the
  existing activity condition variable reports when the worker is waiting for
  idle, completes consolidation, or completes pruning. Production state and
  scheduling are unchanged.
  The active-request regression now proves the worker observed the held request
  before release, and the prune-cadence regression holds a request active for
  the entire prune pass. A centralized 30-second wait remains only as a
  deadlock guard when expected progress never arrives, rather than as the
  behavior under test.
- **The Memory Plane refuses an argument it does not declare, instead of
  dropping it.** The Intent Plane gained this guard when a misspelt
  `lease_seconds` produced a silently defaulted lease; the Memory Plane went
  without it, and the cost was concrete. `ingest_commit` takes `changed_files`; an agent passed `files`; the argument was dropped in silence.
  No `refactored` edges were written, so `evidence_for` counted zero commits, so
  conformance reported "no provenance-bearing mutation", so `complete_task`
  returned `needs_human` and the task never reached `done` — thirteen claims sat
  lapsed-but-still-held on the work board, and nothing in the symptom pointed
  within a mile of the typo. The same mistake on the Intent Plane is caught in
  seconds, because it names the argument, names what the tool actually accepts,
  and says that a misspelt argument is dropped rather than defaulted. Envelope
  keys the server injects, and the `session_id` every client sends on every
  call, are not treated as the caller's mistake.
- **`merge_evidence` could not succeed for anyone, in three independent ways.**
  ADR-0058's promise is that a merge which passed review and CI is stronger
  evidence than a hand-assembled bundle. The verb shipped unable to accept a
  single one. Found by being its first user, trying to certify five tasks that
  were parked precisely because their work had merged and could not be proven.
  1. **It compared the raw session token against a resolved agent id.** The
     dispatch read `req_str(args, "session_id")` and handed that straight to the
     facade as the agent, while `resolve_agent` is a pass-through and
     `task.owner` is a `session:v1:` id. The comparison could never match, so
     every caller was refused — and the message accused the rightful holder of
     claiming credit for someone else's work. `merge_evidence` is now in
     `requires_session`, so `bind_session` resolves the token and injects the
     agent the facade actually compares.
  2. **It measured reachability against the local `main`.** Under ADR-0038
     nobody checks `main` out, so it sits wherever the clone left it — measured
     294 commits behind here, which refused a commit that was demonstrably on
     the integration branch. The ref is now resolved, preferring `origin/main`
     and falling back to `main` where there is no remote. That is still not the
     "whatever branch I am on" trust ADR-0058 removes: it is the protected
     branch's remote-tracking ref, which is the thing the ADR is about.
  3. **It could not see what a merge changed.** `git show --name-only` prints
     nothing for a two-parent commit unless asked with `-m`, `-c` or
     `--first-parent`, so the verb whose whole premise is "a merge is evidence"
     read an empty file list for exactly the commits it exists for, and rejected
     them as touching nothing in scope. Now `diff-tree -m --first-parent`, which
     is also correct for an ordinary commit, so one command serves both shapes.
  The tests missed all three for one reason worth keeping: the fixture built a
  real merge but captured the *feature* commit and named it `merged`, while its
  doc comment called it "the merge commit on `main`" — the shape the tool asks
  callers for was the one shape never exercised. The fixture now returns both,
  and a test asserts the merge really has two parents before verifying it.
  Verified end to end against the live ledger: `merge_evidence` now returns a
  bundle naming the merge and all five changed nodes.
- **A merge driver can no longer return to `.gitattributes`.** A driver runs
  only in a local checkout. GitHub's "Update branch", the merge queue, and the
  merge itself all run server-side with no driver configured — so an attribute
  promising "keep both sides" silently does not apply, and the branch reports a
  conflict in the very file the driver was supposed to keep conflict-free. That
  is worse than having no driver: an ordinary conflict is expected and resolved,
  while this one contradicts the repository's own configuration, in a file
  everybody edits, and invites you to distrust the merge rather than the
  attribute. `merge=union` on `CHANGELOG.md` cost an evening of phantom
  conflicts before the cause was found, and deleting it was only half the fix —
  nothing stopped the next reader from acting on the same "keep both" wish.
  A pre-commit guard now refuses any `merge=` declaration, naming the file and
  line, and points at per-change fragment files instead: two changes never write
  the same path, so they never collide. Comments are exempt, so `.gitattributes`
  can still record why the rule exists.
- Conformance no longer fails work whose product is not code (ADR-0060). Evidence
  that touches no code bound to the task goal now resolves to `aligned` with the
  fact recorded as a finding, matching the verdict the same evidence already got
  when no task was attached. Previously the presence of a task made the verdict
  worse, so a task delivering an ADR, documentation, a benchmark or a changelog
  fragment could never reach `aligned` and parked in `in_review` awaiting a human
  who had no queue to watch. Only a positive signal of a problem — drift, a
  `forbid_change` lock, missing provenance, or governed code changed without a
  covering task — may downgrade a verdict. An audit no longer claims "evidence
  covers task goal" when it did not.
- `ingest_commit` refuses an abbreviated commit hash instead of minting a second
  intent node for a commit already ingested under its full hash. The node id is
  derived from the hash, so `intent:007835a` and `intent:007835a1c979...` were
  two nodes competing to represent one event, inflating commit counts in
  conformance evidence and duplicating provenance edges. Pass all 40 (or 64) hex
  characters; the error names the fix. Hash case is normalised for the same
  reason. Ingestion cannot expand an abbreviation itself, because it never shells
  out to git.
- `MindLeakError::InvalidArgument` distinguishes a caller-supplied argument
  problem from the `Other` catch-all.
- **A file no longer splits into a separate node per worktree.** Node ids are
  repo-relative by contract, but `normalize_path` only settled separators — it
  never made an absolute path relative. Editor sensors report absolute paths, and
  every worktree of a repository shares one graph (ADR-0038), so a single file
  could occupy a different identity in each checkout. Measured on this
  repository: **871 of 6,144 nodes carried absolute ids across 7 worktrees, and
  590 files existed under two identities at once** — `AGENTS.md` and
  `.pre-commit-config.yaml` among them. The damage is quiet and broad: edits
  split across identities so genuine reinforcement decays like a one-off,
  `check_overlap` cannot see two agents editing the same file from different
  worktrees, governed bindings cover only one spelling of the path, and `recall`
  returns the same file twice. The process now declares the checkout it serves
  (`MindLeak::with_workspace_root`, wired from the resolved workspace), and paths
  inside it are made repo-relative at every entry point that accepts one —
  ingestion, deletion, reconciliation, and `check_overlap`. A path genuinely
  outside the checkout is left alone rather than forced into a relative form that
  would name a file that does not exist.
### Fixed

- **One push now runs both test runners.** `scripts/*.test.mjs` are `node:test`
  suites and ran before every push; `editors/vscode/scripts/*.test.mjs` are
  vitest suites importing the very same modules through `../../../scripts/`, and
  ran only in CI. So renaming an export or a guidance string passed every local
  check and failed after publishing, on assertions the author had no reason to
  run.

  It was not hypothetical: three pull requests were blocked on exactly this at
  once — `droppedCommits` → `classifyCommits` in the merge audit, and
  `claim_task` → `task_claim` in the claim gate — each author discovering their
  own rename from a red build, while `main` stayed red behind them.

  Measured against the real rename: `script-tests` reports **139 passed, 0
  failed**, completely blind to it, while the hook reproduces CI's failure
  exactly, down to the assertion text, in **12 seconds**.

  Targeted rather than wholesale, and the difference matters. The full extension
  suite takes ~120s here and reports vitest worker timeouts under fleet load; a
  gate that intermittently blocks a push teaches people to reach for
  `--no-verify`, which is worse than no gate. The hook receives the changed
  files and runs only the suite covering a module that actually changed, so a
  Rust or docs push pays nothing — and says so, rather than passing in silence.

  It refuses rather than skipping when the extension's dependencies are absent:
  a silent skip is indistinguishable from a green suite, which is the failure
  this exists to prevent.
- Optional HTTP circuit breakers now count failures that happen during endpoint
  resolution or response decoding, so a broken embedding or consolidation
  endpoint fast-fails after the configured threshold instead of repeatedly
  consuming its timeout. Embedding responses now reject non-numeric,
  non-finite, and inconsistent-dimension vectors before any index rows are
  written, preventing malformed model output from silently disappearing from
  semantic recall.
- **Prior work is found across a constitution amendment.**
  `existing_work` matched goals with an exact `goal_id` compare, but an
  amendment re-issues every clause as `goal:<slug>@constitution:vN` while tasks
  go on naming whichever form they were created under. A retry created under
  the bare slug therefore could not see work already finished under the
  versioned id, and `task_create` answered `already_serving_this_goal: 0` for
  work that plainly existed — exactly when that question is being asked in
  order not to repeat it. Measured on the live board: 11 titles had been
  created more than once across 29 tasks, and 5 of those spread their attempts
  across ids sharing a single slug. The worst, "Carry controls across an
  amendment", was created six times: the attempt that finished sat under
  `@constitution:v2`, and all five abandoned retries under the bare slug, every
  one of them blind to the completed work. Goal matching now reuses
  `goal_slug` — the same rule `store::goals` and the clause binding already
  use — so the versioned and bare forms find each other.
- **Publishing now records its own evidence, so work published through the gate
  can be certified (ADR-0009).** `canonical-push` already refused to publish
  without a live Lodestar claim, but wrote nothing to the Memory Plane — so
  `evidence_for` returned an empty bundle for work that had just been validated
  and pushed, `check_conformance` answered `needs_human` on
  *"evidence contains no provenance-bearing mutation"*, and `complete_task`
  refused. Measured on this repository, **18 of 21 human-blocked tasks stopped
  for exactly that reason**: one defect wearing eighteen hats, each one costing
  a human decision that had nothing to decide. The push is the right place to
  record — it is where a commit stops being a draft and becomes a fact about the
  world, and it is already the one path where the ledger is not optional. It
  ingests the published sha, subject, and changed files under the same session
  that holds the claim, after the push and never before. An unreachable graph
  warns and does not fail the push: the commit is already on the remote by then,
  and turning a missing record into a failed publication would trade one problem
  for a worse one.
- **The delivery queue no longer loses a minute on every merge.** Immediately
  after a merge GitHub recomputes mergeability and every queued pull request
  reads `UNKNOWN` for a few seconds. That was indistinguishable from a quiet
  queue, so the tick did nothing and slept the full interval — once per merge,
  on every delivery, which across a twelve-branch drain is roughly a fifth of
  the elapsed time spent waiting for an answer GitHub already had. The state is
  now named `settling` and the watcher comes back in five seconds instead of
  sixty. It is safe to return early precisely because a settling tick has, by
  construction, done nothing. One resolved entry is enough to stop settling and
  take a turn, so the queue cannot sit in a recompute loop.
- **A real source file can replace the import stub created before it.** When
  importers were ingested first, their unresolved Rust candidates could leave the
  eventual module root behind an alias. The real file then promoted that alias
  before writing its own structural edges, so an edge still targeting the deleted
  alias failed the transaction with a SQLite foreign-key error. Alias promotion
  now runs after the real file's edges are inserted, allowing the same transaction
  to retarget them safely. A fresh repository-wide re-ingest now processes all
  280 extractable tracked files with zero failures; the previous ordering failed
  on four `lib.rs` / `mod.rs` roots.
- **Semantic recall was answering from a three-day-old snapshot, and nothing
  said so (ADR-0008).** The embedding index is populated by `index_nodes`, an
  explicit offline pass — and nothing ever called it. The maintenance worker ran
  `autonomous_prune` **423 times** and the index pass **once, ever**, three days
  earlier, by hand. Measured on this repository: **5,443 nodes carried no vector
  at all**, and every `recall` result predated that single run. Asked *"ingest a
  git commit as an intent node linked to changed files"*, recall returned nine
  shell invocations of `git commit` and zero code symbols; after a refresh the
  top hit is `ingest_commit` in `ingest/git.rs` at **0.770**. The graph knew; the
  index could not see it.
  A stale index is worse than a missing one. Without an embedding server recall
  errors cleanly and you know where you stand; with a stale one it answers
  confidently from whatever the last manual pass happened to cover, and nothing
  in the result says how old that is. This is also why the recorded "recall floor
  cannot rank" measurement was misleading — it compared score ranges drawn
  entirely from stale nodes.
  The worker now runs the index pass on the prune's activity-independent
  cadence, in bounded batches, recording `autonomous_index` telemetry and
  degrading cleanly when no embedding server is reachable. Indexing is now its
  own switch, `MINDLEAK_AUTONOMOUS_INDEX` (default on, cadence
  `MINDLEAK_INDEX_INTERVAL_SECS`, batch `MINDLEAK_INDEX_BATCH`), separate from
  `MINDLEAK_AUTONOMOUS_CONSOLIDATION`: the two shared one default-off flag, which
  conflated cheap local embedding with expensive generation and is precisely why
  the pass never ran.
- **`recall` ranks with the graph instead of by cosine alone, and can tell
  background from an answer.** Two measured defects, one cause: ranking asked
  the embedding model to be the whole answer, which is the vector-only memory
  the engine exists to replace.
  - _A plausible stranger read exactly like a hit._ Cosine similarity is not
    comparable across queries — embedding spaces are anisotropic, so every text
    carries a baseline resemblance to every other text. Measured against this
    repository's own index, the nonsense query `zzzzqqq wibble flarp` scored
    0.54, above the 0.5 default floor, because the whole field scores about
    that for any question at all. Raising the floor is measurably worse, not
    better: recorded conclusions scored 0.553-0.790 and structural nodes
    matched on shared vocabulary scored 0.527-0.667, so every constant that
    excludes the worst stranger also excludes real conclusions. Recall now asks
    whether a candidate stands out from its own query's field, so a question
    with no answer in the index is answered with silence.
  - _A function name outranked a recorded conclusion._ One query returned the
    right conclusion at 0.651 and an unrelated `merge_import` symbol at 0.626,
    a gap no threshold can separate. The graph can separate it: one is a
    recorded conclusion, the other is a symbol sharing a word. Ranking now
    weights similarity by node kind, as the governing goal requires when it
    says embeddings may only seed graph traversal. The weighting is a
    tie-breaker rather than an override, so a genuinely closer symbol still
    wins and structural questions keep working.

  The similarity floor keeps its original job (ADR-0053) and its default is
  unchanged. A field too small to have a shape is still judged by the floor
  alone, so a young index is never silenced by statistics it cannot support.
  The reported score stays the raw cosine rather than the internal ranking
  composite. No LLM call joins the read path and the zero-token write path is
  untouched. Recorded as ADR-0075, including the two rejected alternatives
  (raise the floor; change the embedding model) and the measurement that
  rejects each, so the next reader does not re-derive them.
## Fixed

- Release and archive-installer smoke tests now disable the default-on
  autonomous recall indexer alongside pruning and consolidation. This keeps the
  intentionally in-memory smoke database valid while still exercising MCP
  initialization, tool discovery, and session registration.
- **Pushing a release tag works again, which the documented release step
  required and the guard had made impossible.** `canonical-push`'s pre-push hook
  judged every push as a branch publication, so `git push origin vX.Y.Z` —
  step 3 of the release procedure in `DEVELOPERS.md` — was rejected three ways
  at once: `symbolic-ref` fails when tagging from a detached HEAD, tagging while
  on `main` tripped the protected-branch refusal, and the publisher flag is only
  set when the script pushes a branch. v0.1.3 could only be cut by setting an
  undocumented environment variable, which is folklore rather than a procedure.
  A tag is now judged on its own terms and against a single invariant: it must
  name a commit already on `origin/main`, because tagging is how a release is
  chosen and an unmerged commit would ship code that never passed review. Branch
  pushes are unaffected and still require a live claim.
- **A server now says when it is a stale build of the checkout it serves.** Both
  servers have always reported `<version>+<git-sha>` at MCP `initialize`, and
  comparing that against the checkout was left to whoever thought to look.
  Nobody did. A binary in `target/release` built two days earlier served every
  session in one workspace: it resolved a pre-ADR-0054 forked identity, so
  `renew_lease` returned a silent `false` and two claims lapsed, and the symptom
  was blamed on the VS Code extension **four separate times** across a session.
  The extension was correct throughout; the binary being accused was never the
  one answering, and no surface said which build was.
  Startup now compares the compiled-in sha against `HEAD` and warns with both
  values when they differ. The comparison is deliberately made only when the
  binary lives inside the workspace it is serving: an installed release running
  against an arbitrary repository is *expected* to differ, and warning about
  that would be noise that teaches people to skip the line that matters.
- **The settings guard now sees every way a setting is read.** A test already
  failed when the extension read a setting the manifest never declared — an
  undeclared setting silently returns its inline fallback, so it cannot be found
  in the settings UI and appears to do nothing when a user sets it in JSON. But
  the scrape matched only reads through a `config` variable, and the extension
  also reads settings inline off `getConfiguration("mindleak")`. Two of the
  eighteen — `captureCommits` and `snapshotLimit` — were therefore invisible to
  it, so for those the guard could not have caught the mistake it exists to
  catch. Both call shapes are now scanned. The "guards the guard" check was a
  floor of "more than five", which 16 of 18 satisfied while two went unchecked;
  it now names a read of each shape, so the blind spot cannot quietly reopen.
- **Symbols now embed their declaration and doc comment, so `recall` finds code
  instead of the tests that exercise it (ADR-0008).** A symbol node stored
  `path:line` as its entire content, so the only thing an embedding could see
  was the symbol's *name*. Terse implementation names (`effective_weight`,
  `prune`, `recall`) embedded as near-noise, while long descriptive test names
  embedded richly — so recall systematically returned the tests instead of the
  code under test. Measured: asking *"how is effective edge weight computed from
  half-life decay"* returned six test functions and not `effective_weight`, and
  querying the literal identifier `effective_weight` did not return
  `effective_weight` either. After the change the same question returns
  `decay.rs:effective_weight` at **0.801**, top hit, with its doc comment
  included in the result — the answer arrives without opening the file.
  Extraction stays deterministic and zero-token: the declaration line and any
  doc comment directly above it are text already parsed off disk, never a model
  call. Bounded to 8 comment lines and 400 characters so a licence header never
  becomes a symbol's meaning, and Rust attributes between a doc comment and its
  declaration are stepped over rather than treated as the end of the comment.
- **The ADR record is read from main, not from whichever worktree asked.**
  `readAdrFiles` listed `docs/adr` on disk, so the design record it reported was
  whatever the asking checkout happened to hold. Under ADR-0038 that is a
  different subset in every worktree, while the design ledger it is compared
  against is one shared per-repository database — so `design-audit` manufactured
  drift that did not exist, reporting every ADR present on main but absent
  locally as a ledger row with no file. Measured across 84 attached worktrees:
  75 ADRs on `origin/main`, and the union across all 196 remote branches also
  75, so main is the complete record and nothing is ever branch-only. Yet 65 of
  those worktrees were missing between 1 and 26 ADRs, and the checkout the
  extension itself reads was 904 commits behind and held 49 of 75 — a third of
  the design record absent, with no error of any kind. `design-audit` now reads
  the record from `origin/main` and names the source in its output. It falls
  back to the working tree only when the ref cannot be resolved, as in a fresh
  clone with no remote, and says so when it does: falling back silently is the
  failure being fixed, because a partial record that reports itself as complete
  makes every tool downstream state confident nonsense. `adr-index` deliberately
  still reads the working tree — it generates the index for the commit being
  made, so a newly authored ADR must appear in it. The blobs are read through a
  single `git cat-file --batch` rather than a `git show` each: the obvious
  spelling costs one process spawn per ADR and measured 10.7s for 75 ADRs
  against 0.36s to read them from disk, which would have made the record correct
  and the tools unusable. Batched, it is 0.33s.
- The design audit's remediation advice now names verbs the server has, and the
  right one. For a row the ledger accepts but credits to nobody it said
  "reopen_undecided_design then accept_design": both names were retired when the
  design cluster collapsed to `design_register` / `design_decide` /
  `design_promote` / `design_query`, and the remedy was wrong as well. ADR-0051
  added `attribute` for exactly that row — it records the decider and leaves
  status, reason and promotion state untouched, and it takes precisely the rows
  `reopen` refuses. Following the old advice would have discarded six
  acceptances that already held and sent them back to proposed, which is a
  bigger act than the defect being repaired. Supersession advice and two stale
  `list_designs` references are corrected the same way.
  A test now scans the audit for every retired design verb, so advice naming a
  tool nobody can find fails the suite instead of misleading the next reader.
- **The agent-loop benchmark stopped counting the thing it exists to measure.**
  `summarizeEvents` classifies each tool call the agent under test actually
  made, and for the Intent Plane it matched
  `/(constitution|board|next_task|active_knowledge)/`. ADR-0059 collapsed that
  vocabulary, so the server the agent talks to now advertises `task_query`,
  `task_create`, `task_claim` and `task_transition` — names the classifier
  could not match. Every coordination call the agent made was silently dropped
  from the exploration count.
  Nothing failed, and nothing could: a name-keyed classifier has no way to
  report that it stopped matching. It returns `false`, the run completes, and
  the exploration and cost figures simply come out lower and look ordinary.
  Every agent-loop run since the collapse under-counted the **mindleak+lodestar
  arm** — the one arm the benchmark exists to justify.

  The classifier now recognises the collapsed verbs **and keeps the retired
  ones**. That is deliberate: `benchmarks/results/2026-07-22-agent-loop-outcome.json`
  was measured before the collapse, when the agent could only call the old
  names, so dropping them would have re-defined the metric rather than repaired
  it. Keeping the change a superset is what makes this a fix to a counter
  instead of a silent re-baselining, and it means the published result stays
  comparable to future runs.

  The classifier moved into `scripts/agent-loop-events.mjs` so it can be
  tested at all — `evaluate-agent-loop.mjs` spawns the Copilot CLI at import
  time, so any test importing it would have started a real four-arm evaluation,
  which is why this code had no test and why the rot went unnoticed. A
  synthetic event stream in the current vocabulary now asserts those calls are
  counted; restoring the old pattern fails it, naming `task_query`.
- The Design Board now loads every ADR, including superseded ones. Three of the
  75 files under `docs/adr` were being dropped on load: the generated
  `README.md` index, and ADR-0018 and ADR-0032 because their status is
  `Superseded by ...`, which the extension's parser could not map. Two real,
  decided architectural decisions were therefore absent from the board and
  could not be shown at all.
  A superseded design is **accepted**. ADR-0050 was explicit that superseding
  keeps the row accepted and adds a link to its successor, so a live design is
  one with no `superseded_by`. Registering these rows as accepted is also the
  precondition for ever recording their supersession, which is guarded on an
  accepted row — until now they could not be superseded in the ledger because
  they were not in it. The successor link is deliberately not derived from the
  file: supersession is an attributed human act needing a recorded decider and a
  registered replacement, and a scan that asserted it would repeat the mistake
  ADR-0042 refuses when it declines to auto-retire a design whose file is merely
  absent from the current checkout.
  A `Status:` value that wraps onto a continuation line is now read whole.
  ADR-0032 puts its successor on the next line, so the previous single-line
  pattern read a bare "superseded by" and lost the reference. The ADR files were
  not edited to suit the parser — they are historical records.
  `docs/adr/README.md` is no longer scanned. It is generated by
  `scripts/adr-index.mjs` and has no `Status:` line, so it produced a skip on
  every load that inflated the count and made a working warning read like a
  defect.
  Root cause worth naming: `scripts/adr-files.mjs` already handled all of this,
  and says in its own header that a second parser would drift from it the moment
  either learned a new status. The extension is that second parser and it had
  drifted. A test now reads the real ADR corpus and fails if any numbered ADR
  becomes unreadable, so the two cannot silently diverge again.
  This adds two `accepted` rows carrying no decider, which is the defect already
  recorded in `gaps.d/accepted-design-rows-carry-no-decider.md`. That is stated
  rather than hidden: an unattributed acceptance that is visible and countable is
  better than a decision the board cannot show. No deciders are backfilled,
  because assigning one would record a decision nobody made.
- **The Design Board shows what needs a decision, and asks once.**
  It read `design_query view=ledger` — the durable record, including every
  historical and materialized item — and then issued one further
  `view=promotion` call per materialized design to decorate rows nobody is being
  asked to act on. Measured against the live ledger: one refresh rendered 75
  rows of which 5 were actionable, at **70 MCP calls**, 69 of them fetching
  detail for finished work. The refresh is wired to a file watcher over
  `docs/adr`, so every ADR touch paid it again — which is most of why the server
  felt slow while the board felt cluttered. `design_query` already named the
  right question: its own description defines `view=board` as "actionable items:
  proposed ADRs awaiting a human decision plus accepted designs awaiting or
  retrying promotion", which is the board this view is named after. The board
  now reads that view: **1 MCP call, 5 rows**. The promotion fan-out is
  unchanged in kind but bounded by what is actually shown, and a test pins both
  the view and the call count, because a fan-out is invisible from the UI and
  would come back unnoticed. The durable record is still `view: "ledger"` for
  anything auditing it.
- The editor no longer sends the server a path it cannot place. `asRelativePath`
  returns its input *unchanged* when a file sits outside every workspace folder,
  and agents routinely edit a sibling worktree from a window rooted elsewhere, so
  an absolute path went on the wire and became a second identity for a file the
  graph already tracked — measured on 2026-07-30 as 34 absolute artifact nodes,
  with one file holding 117 structural edges under its absolute id and 43 under
  its relative one.
  The server has refused such a path since the ingest guard landed, so these
  calls were already failing loudly rather than corrupting; what remained was the
  editor generating a doomed request on every save of an out-of-workspace file.
  A single pure helper now decides whether a raw path is repository-relative,
  mirroring the server's rule so the two agree on what "relative" means, and the
  save, focus, delete and commit paths skip what they cannot place instead of
  asking. A placeable path is sent exactly as before — the rule rejects the id
  shape, not the file.
- The editor no longer watches or searches build output, which is what made this
  repository slow to work in. `files.watcherExclude` and `search.exclude` were
  absent from the committed `.vscode/settings.json` and from user settings, so
  VS Code watched and indexed everything under every open workspace folder.
  Measured 2026-07-30: 88 worktrees, 86 carrying a `target/` directory, 61
  carrying `editors/vscode/node_modules`, and one sampled `target/` holding
  82,891 entries — on the order of seven million watched files that nobody ever
  edits. At the same moment: 7.0 GB free of 55.6 GB, 39 VS Code processes
  holding 17.1 GB across 8 renderers and 10 utility processes, CPU at 52% of 16
  cores. Every `cargo build` rewrites thousands of files inside a watched tree,
  and every window whose workspace contains it is notified.
  The MCP servers were measured too and are not the cause — four processes,
  55 MB, under 40 seconds of CPU between them. Recorded because the obvious
  suspect was wrong, and the measurement is what said so.
  `target`, `node_modules`, `.vscode-test`, `out`, `dist`, `coverage` and the
  local state directories are now excluded from watching and from search. The
  settings file is tracked, so every worktree and every fresh clone inherits it
  rather than each machine being configured by hand.
  `files.exclude` is deliberately not set: it hides entries from the explorer
  rather than reducing work, and hiding a directory someone may need to open is
  a real cost for no measured gain.
  Takes effect for a window when that window reloads. It does not shrink the 88
  worktrees or the disk they occupy, which is a larger and separate problem.
- **The `evidence_for` false alarm did not recur.** The Known gaps entry
  recording it asked to be revisited if it were seen again; it was not. Four
  `evidence_for` calls across four tasks on 2026-07-29, in a session independent
  of the one that raised it, each returned the commits they should.
  The shape they shared is the useful part, and it is now written down: the
  commit ingested by an explicit `ingest_commit` carrying its true author
  timestamp, attributed to the session, and a window opened at
  `claim_started_at` *before* the commit existed. Every case the original entry
  was written about had the window opening *after* the work — which is a
  claim-ordering problem, not an evidence-query one, and points at a different
  fix from the one a reader of the original entry would have reached for.
  Docs only. Nothing is retracted: the disproof already on record stands, and
  this only settles the "until seen again" it left open.
### Fixed

- **The extension-test hook comes off pre-push.** It was added to catch a rename
  landing without its consumer, and its own comment warned that "a gate that
  intermittently blocks a push is worse than none — it teaches people to reach
  for `--no-verify`". That sentence turned out to describe the hook.

  Under fleet load vitest reports `[vitest-worker]: Timeout calling
  "onTaskUpdate"` and exits non-zero **with every test passing** — `14 passed, 1
  error` — and the runner cannot tell that from a real failure. It blocked
  pushes across the fleet on tests that had in fact passed. That is worse than
  the breakage it was added to catch: a missed rename fails one branch's CI,
  this stopped everyone.

  The capability is kept, not the gate. `make ext-test` and
  `node scripts/ext-test.mjs <changed files>` still run it, and CI still runs the
  full suite. It earns its way back to pre-push when it can distinguish a worker
  timeout from a failed assertion, and not before.
- **The repository's own guards are now tested by CI instead of by nobody.**
  Six test files under `scripts/` — covering the conformance gate, the
  merge-driver guard, the claim gate, the publication record, the delivery
  queue and the board health report — carried a header saying `Run with: node
  --test scripts/` and were wired into nothing: no CI job, no Makefile target,
  no hook. Forty-five assertions about the machinery that decides whether work
  is honest ran only when somebody remembered to type the command, which is to
  say they had not run in a long time. Worse, the command in the header no
  longer works: passing a directory to `node --test` fails on Node 24, and the
  glob that replaces it fails on the Node 20 that CI pins, so a developer
  following the instruction got a module-resolution error rather than a test
  run. `make script-test` now enumerates the files and passes them explicitly,
  which works on both versions and on every OS, and it runs in CI, in `make ci`
  and on pre-push. The runner refuses to report success when it discovers no
  test files at all — a runner that quietly finds nothing is indistinguishable
  from a green suite.
### Fixed

- **The pre-push hook no longer contaminates the suites it runs.**
  `canonical-push` sets `MINDLEAK_CANONICAL_PUBLISH` while running the pre-push
  hooks, and the extension-test runner passed the environment straight through.
  So the suite asserting that a *direct* invocation of the publisher is refused
  inherited the flag, saw the direct call allowed, and failed — while passing
  when run by hand.

  A runner whose answer depends on who invoked it is worse than one that does
  not run at all: it makes a real guard look broken, and sends the author
  chasing their own tooling instead of the bug. Caught when the hook blocked its
  own author's push on a suite that passed standalone.

  `MINDLEAK_CANONICAL_PUBLISH` and `PRE_COMMIT_REMOTE_BRANCH` are now scrubbed
  alongside git's `GIT_DIR` family, and the scrub is one exported function with
  its own tests rather than a list copied inline — including that unrelated
  variables survive and the caller's own environment is not mutated.
- **The link checker no longer trips over illustrative links in code.**
  A `[text](target)` written inside inline backticks or a fenced code block is
  documentation about a link — an example, a former filename, a shape — not a
  live link, so its target need not exist. The checker (added in the previous
  change) treated them as real and flagged them, which blocked any PR whose
  changed docs carried such an example. It now skips links inside code spans and
  fenced blocks, while still catching a real link elsewhere on the same line.
- **The pre-flight could not see the coverage the gate reads, and advised
  against the one shape the constitution provides for cross-cutting work.**
  `advise` resolved governing clauses from the task's own `goal_id` alone, while
  `evaluate_base_conformance` resolves them through the task's recorded
  `goal_coverage` (ADR-0041). The shared resolver exists precisely so the
  forward-looking advice and the retrospective gate cannot fork the rule — its
  own doc comment says so — and the `advise` call site forked it by passing no
  coverage.
  So a task that had correctly declared the governing goal in `also_serves` at
  creation was still told its change *"would drift; get a covering task or
  review before acting"*, while the gate it was predicting would have found it
  in scope.
  Wrong in the most expensive direction. An agent that believes the advice
  re-declares the task; the replacement carries the same coverage; the answer
  does not change. `also_serves` is fixed at creation with no verb that adds
  coverage later, so the advice invites a loop with no exit — measured live on
  2026-07-30, where a correctly covered replacement task was told the same
  thing as the one it replaced.
  `advise` now resolves through the same coverage. A task without the
  declaration still reads as drift, so this is coverage-aware rather than a
  blanket softening, and the test asserts both directions.
- **The only sanctioned publish path called tool names the removal train will
  delete.** `canonical-push.mjs` asked for `board` and `check_overlap`;
  ADR-0059 retired both into `task_query` with `view="board"` and
  `view="overlap"`, and they answered only because the deprecation window
  answers them. Because canonical-push runs from a pre-push hook rather than a
  terminal anyone is watching, the removal would not have surfaced as a tidy
  error: publishing would have stopped for **every agent in the fleet at the
  same moment**, as a tool-not-found from inside git, naming neither the cause
  nor the fix. `board-health`, `stranded-report` and `design-audit` followed the
  same path. All four now speak the current vocabulary, proven by publishing
  this change through the migrated push rather than by reading the diff.

- **A guard now refuses a retired name in the delivery scripts, and names the
  file and line.** Migrating five call sites lasts until the next rename; the
  point of the collapse was to stop finding this class by hand, and it has now
  been found by hand four times — dispatch, `requires_session`, a test
  whitelist, and the publish path. The new check reads the delivery scripts and
  fails with `scripts/<file>:<line> calls <name>`, which is the whole value: it
  arrives before the push instead of inside a hook, and it says where.
  It reads **tool-name positions only**, never argument values. A collapsed
  verb takes the retired name *as* an argument — `task_query` with
  `view: "board"` — so a scan matching the bare quoted string would report the
  migration itself as a violation, and a guard that cries wolf is one people
  learn to skip. That is precisely how the guards it replaces went stale, so
  the check also asserts it can still see the live call sites: a scan that
  reads nothing passes, and passing is indistinguishable from working.
- **The delivery queue no longer waits forever on a check that never started.**
  An armed pull request whose head carries no check runs at all answered
  "anything running?" and "anything failing?" exactly as a fully green one did,
  so it read as up to date and idle: the tick returned `wait` with "waiting on
  GitHub to merge it", and nothing aged it out — the stall threshold guarded
  only a branch whose checks were already running. One pull request whose
  workflow never fired could therefore hold every branch behind it
  indefinitely, and the log read like a healthy queue the whole time. An absent
  rollup is now its own state: it is still worth waiting for while it is young,
  because a run can take minutes to appear, but it ages out on the same stall
  threshold and the branch behind it takes its turn. The tick also names it —
  `#N is armed and up to date but no check has reported` — so the queue can no
  longer be silently wedged by the one thing it cannot fix itself. A branch
  that is merely behind is still updated regardless of its rollup, because that
  update is what triggers the run it is missing.
- The silent-knowledge audit counted one of the two ways a lesson reaches an
  agent, so it reported records as dead that were arriving: it called 68 of 210
  unreachable, where 12 are. Reachability now has a single definition —
  `Lodestar::knowledge_reach` — and `record_knowledge`, `active_knowledge` and
  `scripts/silent-knowledge.mjs` all ask it rather than each deciding for
  themselves, which is how three readers of one rule came to be falsified
  together by a single commit. The report now separates records reaching agents
  by the nodes they name from those reaching only the goal they were learned
  under, and says how many of the latter are crowded out by that path's
  per-check cap.
- **A guard that forbids closing a task had stopped being able to fail.**
  ADR-0065 says the completion offer *offers* and never closes, and the test
  asserting it watched for `complete_task` — a name ADR-0059 retired into
  `task_transition(to="complete")`. Proved by probe: with the module made to
  close a task through the new verb, the guard still passed. It now watches
  every verb that could close a task, including the deprecated alias, because
  closing through the alias is equally forbidden while it still answers.
  Only one of the two ADR-0065 assertions was actually vacuous — the other was
  incidentally saved by an exact-call-list comparison that noticed the extra
  call. That is luck, not coverage, and it is worth saying plainly: a guard
  named after a verb dies quietly when the verb is renamed, and the only signal
  is that it keeps passing.

- **The two messages the fleet reads most often taught retired verbs.** The
  claim gate's remediation — printed when a publish is refused, which is the
  moment an agent is most likely to copy an instruction verbatim — said
  *"Claim existing work: `claim_task(task_id)`"* and *"`create_task(goal_id,
  title, acceptance)`"*. The completion offer, printed after every successful
  push, said *"submit explicitly with `complete_task(...)`"*. All three are
  retired names. Nothing was broken, because the deprecation window still
  answers them, which is precisely why nothing noticed: advice is a string, so
  no compiler, linter or type check can see it go stale. They now name
  `task_claim`, `task_create` and `task_transition` with the argument that
  selects the act, and tests assert the verbs rather than the wording so the
  sentences stay free to improve.

- **Two more guards were written against names the surface no longer
  advertises** — the tool-surface benchmark's fixture (`next_task`) and the
  completion-offer assertions above. Recorded in Known gaps: the agent-loop
  benchmark still *drives* the retired vocabulary, so its published results
  characterise the surface agents are being migrated away from, and the
  committed scripts — including `canonical-push`, the only sanctioned publish
  path — still call retired names at 17 sites that the removal train will
  delete.
- **An unreadable model answer no longer masquerades as a semantic verdict.**
  `LlmClient::judge` used to turn a missing `verdict` into `needs_human` and a
  missing `rationale` into empty text, so a protocol failure reached the durable
  receipt as `semantic check needs human review: ` and sent a human to review
  nothing. Missing fields now follow the existing `semantic check unavailable`
  path, an unsupported verdict names the value the model returned, and a real
  `needs_human` answer with a blank rationale says `judge gave no reason`.
- **The merged-branch audit called work in review "lost", and turned `main`
  red.** A commit pushed onto a branch after its pull request merged was
  reported as never having reached `main`, with the instruction to open a
  follow-up pull request — when the commit was already sitting in an open one.
  That instruction cannot be carried out: the follow-up exists, and nothing
  anybody does will satisfy the audit until that pull request merges. It is the
  same defect this audit was rewritten to remove, in a new costume. An audit
  with no green move available gets switched off, and switching this one off
  takes the check that catches genuinely stranded work with it. Measured on the
  live repository: commit `ffab86ea`, held against PR #213's merged branch, is
  an ancestor of the open PR #231, and three consecutive `main` builds failed on
  it. The audit now asks whether any open pull request still carries the commit.
  If one does, the commit is reported as in review, named against that pull
  request, and does not fail the build. If none does, it fails exactly as
  before, so nothing is weakened — proven by a fixture where an unrelated open
  branch does not rescue a stranded commit. Failing to list open pull requests
  degrades to the old, noisier behaviour rather than to a clean bill of health,
  because an unreachable `gh` must not be able to silence the audit. Pushing to
  an already-merged branch is still reported and still a mistake: that pull
  request will never reopen, so the commit survives only for as long as
  something else happens to carry it.

## [0.1.3] - 2026-07-28

### Added
- **`questions_for_a_human`: the other half of ADR-0046's dialogue.** Agents
  could address a question at a peer and list what was addressed to them, but
  there was no way to list what was waiting on a **person** — `pending_questions`
  matches an agent id, and a human has no agent id, because "addressed at a
  human" is the *absence* of an audience. Answering therefore meant walking every
  parked task and reading its thread by hand, which is why five tasks sat
  `awaiting_human` for up to seventy-five hours with no surface that could show
  them. The new call returns each parked task's question, its title, who asked,
  and how long it has gone unanswered, rendered as a readable inbox with the
  structured form still in `structuredContent`. Same posture as its agent-side
  twin: a query, never a queue — reading cannot consume a question, and two
  people reading see the same rows. Waiting time is reported and never judged,
  because a staleness threshold invented here would become a policy nobody
  agreed to.
- **`draft_questions`: the collision you already have, as a question you can send
  (ADR-0055).** ADR-0046 built agent-to-agent dialogue properly and nothing ever
  used it — measured after an eight-hour session, `pending_questions` was empty
  and all five stalled tasks were `awaiting_human`, one of them for seventy-five
  hours. The gap was never capability; nothing surfaced that there *was* a
  question to ask. `draft_questions(task_id)` finds peers whose live claims
  intersect this task's declared scope and returns an addressed draft for each,
  ready to hand to `ask_question`. It records nothing, parks nothing and
  addresses nothing, so a draft nobody sends leaves no trace. The collision is
  found deterministically; only the phrasing is model-assisted and it falls back
  to a template when no local model is reachable, with every draft reporting
  `drafted_by: model | template`. The model is asked to draft, never to
  arbitrate: it may ask about intent and ordering and is forbidden from deciding
  who is right, because a model verdict carries no evidence and ADR-0009 makes
  evidence the basis of every verdict here.
- **`attribute_design_decision`: a decision already made can still be signed
  (ADR-0051).** ADR-0047 named this failure exactly — a row that "asserts a
  decision that can never be attributed" — and then repaired it only for designs
  whose promotion had not materialised work, because reopening one that had would
  leave tasks descending from a decision the ledger no longer shows. Auditing the
  board found the hole that left: of 25 unattributed designs, **18 had already
  materialised work** and were beyond both verbs — unreopenable and undecidable.
  Those 18 are ADR-0001 through ADR-0032, so the repository's founding decisions
  were the only ones permanently attributable to nobody, and the fix for exactly
  that complaint had shipped four ADRs earlier. The new verb records the person
  behind a decision that already stands, writing `decided_by` and nothing else.
  Its guard is the deliberate complement of `reopen_undecided_design`'s, so
  between them every undecided row has exactly one route and neither is a softer
  way of doing the other's job. A `decided_by` already recorded is never
  overwritten — attribution fills an empty field and can never change a full one.
- **`supersede_design` records that an accepted design has been replaced
  (ADR-0050).** The ledger had `proposed`, `accepted`, `rejected`, and no way to
  say "this was decided, it held, and something better replaced it". ADR-0018
  and ADR-0032 declare `Superseded by <ref>` in their files; both sat `accepted`
  in the ledger, so every ledger-driven view showed a withdrawn decision as
  live. Rather than a fourth status — which would discard the fact that the
  design *was* accepted and could not say by what — a design now carries the
  same `superseded_by` link the goal model already has, so there is one
  vocabulary for supersession instead of two. `status` is deliberately
  untouched; a live design is one with no `superseded_by`, and the Design Board
  filters on it. Guarded on a recorded `decided_by`: superseding is a statement
  about a decision that was actually made, so a row carrying an imported status
  with nobody behind it must be reopened (ADR-0047) or retired (ADR-0042)
  instead. The replacement must already be registered, and the link is never
  inferred from an ADR's prose — deriving it would repeat exactly the mistake
  ADR-0047 documents.
- **Publishing declares where it is working, and warns when one identity is in
  two places (ADR-0044, ADR-0049).** The claim gate re-opened the session on
  every push and declared nothing, so it replaced a real declaration with
  silence — `fleet_view` reported `branch: null` for agents that had declared a
  branch minutes earlier. It now declares branch, head, base, a counted
  `behind`, and a clean tree, at the one moment those are certainly true. It
  also warns when an identity publishes a branch it did not declare while
  holding live claims: one agent cannot publish two branches at once, so that is
  the observable signature of several agents sharing a session token and
  resolving to one identity — a failure that ran unnoticed for a whole session
  and silently voided every claim, overlap check and wait cycle keyed on it.
  Advisory, because switching branch with work still claimed is legitimate; it
  names a suspicion nobody could previously have formed at all.
- **`make design-audit` reports where the ADR files and the design ledger stop
  agreeing.** Every drift found so far was found by hand, one ad-hoc query at a
  time, and each was invisible until someone thought to look: an ADR merged
  without ever being registered, a file still saying `Proposed` after the
  decision was recorded, and a row imported as `accepted` with `decided_by`
  empty — a decision nobody made, which `accept_design` then refuses because
  deciding twice is not an undo (ADR-0047). The audit names all four shapes. Its
  first run found 23 undecided rows and one unregistered ADR. It reads the ledger
  through `list_designs` on the release `lodestar-mcp` rather than opening
  `spec.db`: the server already resolves its own per-repository database
  (ADR-0038), so the path rule is not forked, and `list_designs` already omits
  retired records, so a retired row cannot masquerade as an orphan. It is a local
  diagnostic, not a hook — CI has no ledger to read, which is exactly why the
  ADR-index guard can gate and this cannot. `Superseded by <ref>` is reported as
  a note rather than drift: the ledger has no such status, so neither side is
  stale and forcing agreement would throw information away.
- **A question addressed to you now arrives on a call you already make
  (ADR-0046).** `ask_question` could address a peer and `pending_questions` could
  find it, but only if the peer thought to look — and a capability that depends
  on remembering is adopted at the rate the whole intent plane measured while
  participation was optional: zero. `claim_task` and `renew_lease` now carry
  `waiting_on_you` when a peer is waiting on this agent. The heartbeat is the
  one that matters: a question usually arrives *during* the work, long after the
  claim. It is absent when nothing is waiting — no key, no empty array — because
  "no questions" and "this server does not report questions" must not look the
  same to a reader, and it stops arriving once answered, or the delivery becomes
  noise an agent learns to skip past. Nothing is reserved or consumed: it stays a
  read over the durable thread, so two readers still see the same rows and no new
  shared mutable resource is introduced (ADR-0045 clause 2).
- **Publication requires a live claim; the ledger is no longer optional
  (ADR-0049).** The Intent Plane had one real arbiter (`claim_task`) and **zero
  automatic integration points** — nothing in the hooks, the scripts, or CI ever
  consulted it. That is not a plane being bypassed, it is one that is optional by
  construction, and a single night of concurrent work measured the cost: 9 pull
  requests merged with **0 conformance receipts**, 61 done tasks against **61
  abandoned** ones, **2 claim owners across 23 agent identities**, and two agents
  independently building overlapping answers to the same question, discovered
  only when both pull requests were open. `canonical-push` now refuses to publish
  without a live claim owned by `LODESTAR_AGENT`, naming `claim_task` and
  `create_task` as the actions that satisfy it. The gate is at **push, not
  commit**: a commit is a draft, and gating commits makes people invent tasks to
  get past the check — a lying ledger, which is worse than an empty one because
  it reads as governed. An unreachable ledger **refuses**, deliberately unlike
  the auto-merge guard: `gh` being absent is an ordinary condition, but Lodestar
  is local SQLite behind a local binary, so unreachable means broken, and failing
  open would make "the ledger was down" the universal bypass. Overlapping live
  claims on the branch's paths are **reported, never enforced** (ADR-0024) — the
  collision is named at the one moment it is still cheap to act on.
- **Arming auto-merge now means finished, and a merged branch is checked for
  what it left behind (ADR-0045 clause 2).** A pull request's merge decision has
  two writers — the agent pushing commits and whoever arms auto-merge — and
  nothing arbitrated between them. Observed in production: PR #37 was armed at
  07:51:12Z, merged at 08:09:21Z, and the next commit landed **13 seconds
  later**; four commits, including the one that stopped two surfaces
  disagreeing, never reached `main`. Nothing failed. The pull request read
  merged, the branch read ahead, and CI was green on both — the only signal was
  an ancestry check nobody was running. `canonical-push` now refuses to publish
  onto a branch whose open pull request has auto-merge armed, naming the pull
  request and the exact `gh pr merge <n> --disable-auto` that satisfies it: the
  promise to merge is a promise the branch is done, so more work means disarm
  first. When `gh` is absent or unauthenticated the guard permits the push — a
  guard that blocks on its own blindness is unsatisfiable, and an unsatisfiable
  guard teaches bypass. `make merge-audit` is the backstop, reporting any merged
  branch whose commits never reached the base regardless of how it happened; a
  deleted branch reports as *unverifiable* rather than clean, because "we cannot
  tell" and "nothing was lost" are different answers.
- **`reopen_undecided_design` lets an imported status become a real decision
  (ADR-0047).** `reconcile_designs` imports each ADR's declared `Status:`
  faithfully — importing thirty-five settled decisions as `proposed` would
  misrepresent the repository's own record — but reconciliation observes a
  file, it does not witness a decision, so the row lands with `decided_by`
  empty. Deciding is guarded on `proposed`, so that row was then frozen:
  permanently asserting a decision nobody could be named for. Hit on ADR-0045,
  where a reviewer said "I agree with it" and `accept_design` answered
  *already accepted; only a proposed item can be decided*. The new verb returns
  such a row to `proposed`. It is not an undo: a design carrying a `decided_by`
  is refused, because superseding a recorded human act is a new decision rather
  than the erasure of the old one, and so is one whose promotion has already
  materialized work.
- **`stalled_work` and the fleet view now read one wait graph, and the parked
  taxonomy stopped lying (ADR-0046).** These landed as two independent answers
  to "why is this not moving?" and immediately disagreed: `stalled_work` labelled
  every `needs_input` task `parked` with the detail "parked awaiting an answer",
  which after addressed questions was wrong three ways — it called a park on a
  named peer a park on nobody, it sent a reader looking for a human who owed
  nothing, and it rendered a mutual deadlock as two ordinary waits, re-opening on
  a second surface the exact gap the fleet view had just closed. `Parked` is now
  split into `awaiting_human` (a person owes the answer, whether from `in_review`
  or an unaddressed question), `awaiting_agent` (a named peer owes it, and the
  report names them), `deadlocked` (the peer is waiting back, so only an answer
  from outside breaks it), and `paused` (deliberately suspended — nobody was
  asked, so nobody owes anything). Both surfaces take the same `waits()` set
  rather than deriving it twice: `stalled_work` answers per task, `fleet_view`
  answers per agent, and they are now the same fact seen along two axes instead
  of two facts that can drift apart.
- **Append-only lists merge instead of colliding.** In one session seven merges
  of `main` into feature branches produced conflicts, and **every one** was in
  `CHANGELOG.md` or `docs/adr/README.md` — never in source. Concurrent branches
  all append an entry at the same place, so the collision is structural and the
  resolution was "keep both" every single time. Both files now declare git's
  built-in `merge=union` driver, which takes both sides of a conflicting hunk
  rather than writing markers. `DEVELOPERS.md` is deliberately **excluded**
  despite colliding just as often: its prose is revised in place, and union
  never reports a conflict, so two branches rewording the same paragraph would
  silently keep both copies. A file whose lines only accumulate can merge
  itself; a file whose lines change needs a human to look. Union removes the
  mechanical conflict, not the reading — it can still reorder entries or leave a
  duplicate if two branches added the same one.
- **The ADR index is generated, not hand-maintained.** Number, title, and
  status all already live in each ADR, yet the table in docs/adr/README.md
  was edited by hand — so every concurrent branch appended a row to the same
  place and every merge conflicted on it, the same shared-counter shape as ADR
  numbers themselves. It drifted anyway: **ten of forty-five rows were wrong**
  when scripts/adr-index.mjs was introduced, including ADR-0026 listed as
  Proposed while the file said Accepted — the ADR the whole constitution
  backlog gates on. A pre-commit hook now fails if the index does not match the
  files, and make adr-index regenerates it.
- **make worktree-setup prepares a linked worktree.** A fresh worktree failed
  at push time with a module-resolution error from the prettier hook, which
  named nothing about the real cause. Hooks and cargo tools are shared through
  the common .git dir, so a worktree needs only its own extension deps — the
  full make setup would re-run pip install and cargo install for nothing.
- **The fleet view now shows who is waiting on whom, and names the deadlocks
  (ADR-0046).** Addressed questions made a wait cycle reachable: agent A parks
  on B while B parks on A, and both tasks sit in `needs_input` — which every
  surface renders as ordinary parked work. A pair could burn the whole seven-day
  parking grace doing nothing while the board read healthy. `fleet_view` now
  carries `waits` (who is parked on whom, derived from the ledger's own
  unanswered addressed questions rather than declared by anyone) and
  `wait_cycles` (sets of agents each transitively waiting on the others, so no
  member can unblock any other). Each cycle names the tasks that form it:
  answering any one of them breaks it, and the test asserts that it does — a
  finding whose implied remedy does not work is just an alarm. A one-way wait is
  deliberately *not* a cycle, because the addressee can still answer, and
  reporting a legitimate wait as a deadlock is the fastest way to make an
  advisory signal ignored. Longer rings are found by the same rule rather than a
  special case for pairs. Still advisory and capped at `review` (ADR-0034): the
  remedy is a human answering a question, and a view that blocked on its own
  observation would be a control nobody asked for.
- **Agents can now say something to each other, through the durable thread
  rather than to each other (ADR-0046).** Two agents shared a blackboard but
  could address nothing at one another, and two specific things were missing
  rather than merely absent. `block_task` took a predecessor id and no reason,
  so an agent could have work taken off it — blocking clears a live claim — with
  no way to discover why; `pause_task` was the same. That is precisely the
  failure ADR-0045 names, produced by the system that exists to make verdicts
  explicable. And `ask_question` could only reach a human, so an agent needing
  something only a peer knew had to park for a person who would go and ask that
  peer. Both are now answered on the existing append-only thread: `task_qa`
  gains a `note` kind carrying an optional `reason` on `block_task` and
  `pause_task`, and `ask_question` gains an optional `audience` addressing the
  question at a peer agent. `pending_questions` returns what is addressed to
  you. It is a query, never a delivery: nothing is reserved or consumed, so two
  readers see the same rows and reading can never lose a question, and it needs
  no arbiter because it mutates nothing. A mailbox or queue was rejected — it
  would put decisions somewhere the evidence bundle cannot see, add a shared
  mutable resource requiring an arbiter, and introduce a way to lose a message
  that a table read does not have. The park is deliberately identical for a
  human and a peer, so the ADR-0020 parking grace still protects a task from an
  addressee that never replies; anyone may answer, so a human can always unstick
  two agents waiting on each other; and addressing a question to yourself is
  refused, because it parks the task on the only agent that cannot act while it
  is parked.
- **`stalled_work` shows why the board is not moving.** Three tasks once sat
  unfinished for three different reasons and nothing reported any of them: a
  lease lapsed after the work had already shipped, a change landed outside its
  claim window, and a legitimate cross-plane edit resolved as drift. All were
  found by accident, and one blocked task had queued behind them for 78 hours.
  The new read-only tool names each stall — lapsed leases, work awaiting a
  human, blocks behind something no agent will advance, blocks naming a task
  that is not on the board, and parked work — with how long it has been true.
  It **does not decide** whether that is too long: a staleness threshold
  invented in the engine would become policy nobody agreed to, and the honest
  report is the fact plus its age. It records nothing, changes no task state,
  and produces no verdict, so it can never be mistaken for conformance. Waiting
  behind live work is deliberately not reported — a report that flags ordinary
  sequencing trains people to ignore it.
- **`adr-guard` refuses to let a decision record exist in only one place.** An
  ADR is the reasoning behind the code, and losing one is silent — nothing
  fails, the file is simply not there any more. Three near-misses in a single
  session motivated it: one ADR staged but never committed, one committed only
  to a branch that was never pushed, and one never added to Git at all. Each was
  found by chance. `node scripts/adr-guard.mjs` (`make adr-guard`) now reports
  both failure modes across **every attached worktree and every local branch**,
  because under ADR-0038 concurrent work is spread across worktrees and an ADR
  can be stranded in any of them. A pre-push hook runs the working-tree half
  (`--uncommitted`); the unpublished check is deliberately excluded there,
  because it would fail the very push that publishes the ADR.
- **A pre-commit hook refuses an ADR number another branch already claimed.**
  ADR numbers are a shared counter with no coordination: every concurrent agent
  reads "the next number" from its own branch, which cannot see a sibling's
  in-flight ADR. Two agents pick the same number and the collision only surfaces
  at merge, by which point both ADRs are written, cross-linked, and cited in
  commit messages — this repository has already spent two commits renumbering
  after exactly that. `scripts/adr-number-guard.mjs` reads every ref rather than
  the working tree, because the conflict lives in the branch you cannot see, and
  names the first genuinely free number instead of merely the next one.
- **A read-only fleet view, and the two corrections it forced (ADR-0044,
  amending ADR-0035).** `fleet_view` reports who is working where: each live
  session's declared branch, head, and base, how far behind that base it said it
  was, and whether live sessions disagree about their base. Building it surfaced
  two things ADR-0035 asserted but could not deliver. Staleness was defined as
  "commits behind the declared base" while the server is forbidden from reading
  Git, and the declared fields can only show that two commits *differ*, never the
  distance between them — so `open_session` now accepts a client-counted
  `behind`, keeping the caller-supplies-facts rule intact. And declared context
  lived in the process-local session registry, while under ADR-0038 every linked
  worktree shares one `spec.db` and runs its own server: a view built on that
  registry would have reported the sessions of whichever process answered while
  presenting itself as the fleet. Context is now persisted with its
  `declared_at`, so a reader can discount a stale declaration rather than be
  quietly misled by one. Silence is never read as agreement: a session holding a
  claim with no declared base is counted and shown, and `unknown` is modelled
  separately from `current` so the two cannot be collapsed. The view carries its
  own ceiling in the payload — advisory, capped at `review`, never a gate.
- **`retire_design` removes an orphaned design record — by a person, never by a
  missing file (ADR-0042).** `reconcile_designs` keys on the ADR path, so
  renaming an ADR registers a new record and orphans the old one permanently.
  Two such rows existed, left by renumbering one decision twice on its way to
  ADR-0040; their paths exist on no branch, and every Design Board row is
  clickable, so both threw when opened. There was no retirement path at all.
  The tempting fix — retire any design whose ADR file is absent — is refused:
  under ADR-0038 several worktrees on different branches share one `spec.db`, so
  "missing from this checkout" is routine and retiring on it would silently
  delete live decisions on someone else's branch. Retirement is therefore an
  explicit human act carrying an actor and a rationale, guarded so a second
  caller cannot rewrite who did it, and it is **not** a delete (ADR-0019): the
  row keeps its id, path, decision, decider, and materialization history.
  Retirement is also kept orthogonal to `proposed`/`accepted`/`rejected` — a
  fourth status would overwrite the human decision and make "was this accepted?"
  unanswerable. Retired records leave the board and stay in the audit view under
  `list_designs(include_retired: true)`.
- **The enforcement machinery is now reachable.** `complete_clause_contract` and
  `register_control` close a gap that made every other constitutional feature
  decorative: `complete_clause_contract` existed only on the store, called only
  from tests, and there was no generic way to bind a control to a clause at all.
  Audited on this repository's own constitution, the result was **0 of 17 active
  clauses able to drive a hard verdict** — every one with no scope, no evidence
  contract, and no consequence, which is exactly what SPEC-CONSTITUTION §10
  prescribes for migrated clauses and exactly what nothing could then change.
  A constitution that is active, correct, and incapable of reaching a verdict is
  the worst of the three states, because it reads as governed.
  `complete_clause_contract` **refuses a clause on an active version**: moving a
  rule from review-only to `block` changes what governs everyone already working
  under it, and ADR-0039 already fixed the shape of that act — draft an
  amendment, complete the contract there, promote it with a rationale and a
  diff. A direct edit would be precisely the quiet amendment that diff exists to
  expose.
  `register_control` makes the ADR-0034 ceiling usable by something other than a
  ratchet, and asks the caller to declare the power the mechanism honestly has:
  `mechanical` only where the action was genuinely prevented, `observed` where it
  is proved after the fact, `advisory` where the hint may be stale. The last two
  cap at `review` — an advisory that looks like a mutex grants false safety.
- **Cross-cutting work can be declared instead of read as drift (ADR-0041).**
  A task serves one goal, so conformance returned `Drift` for any governed node
  bound to a different one — the same verdict, and the same finding, that an
  unsanctioned edit produces. Three legitimate cross-plane changes hit it
  (ADR-0018, ADR-0024, ADR-0035), and the audit could not tell them apart from
  drift. `create_task` now accepts `also_serves`, declaring at creation the
  additional goals the work serves; a binding to one of those counts as in
  scope. Coverage is fixed at creation and has no later mutator, because
  coverage added once conformance has complained is a rationalisation, not a
  plan. A verdict that leaned on a declaration caps at `needs_human` and names
  the goals it relied on, so declared breadth buys a review rather than a pass
  (ADR-0034 ceiling rule). A task that declares nothing is unchanged, including
  still returning `Drift`.
- **Human review now closes inside the default Work view (ADR-0040).** The
  former Intent Board uses action-oriented labels and puts `Review needed` work
  first, with inline **Accept**, **Retry**, and **Inspect proof** actions backed
  by the existing `resolve_task`, `reopen_task`, and `conformance_history` tools.
  The complete Evidence Board and export flow remain available as advanced
  history but are hidden by default, reducing the common workflow to one surface
  without weakening proof or changing MCP semantics.
- **`scoped-commit` refuses the pre-commit stash race (ADR-0038).** `pre-commit`
  stashes every unstaged change before running hooks and restores it afterwards.
  Alone that is invisible; in a fleet it corrupts. If a second agent writes to
  the same working tree inside that window, the restore collides and the hooks
  report a *fictitious* failure — `files were modified by this hook`, from
  `check-added-large-files` and `check-merge-conflict`, which modify nothing,
  about files the committer never touched. The message points nowhere near the
  cause, so the natural response is to retry, which widens the window.
  `node scripts/scoped-commit.mjs` now exits 3 when the **primary checkout** has
  other worktrees attached and unstaged files outside the declared paths are
  live, names them, and points at `git worktree add`. The trigger is the shared
  checkout rather than the mere existence of a fleet: a linked worktree belongs
  to one agent, so unrelated work in it is that agent's own and the stash is
  harmless. `--allow-foreign-wip` overrides for a single operator. This guards
  the sanctioned path only — a bare `git commit` can still hit it, because the
  stash happens inside `pre-commit` itself and no hook can observe the tree
  before its own framework moved it.
- **A session can declare where it is working (ADR-0035).** `open_session` now
  accepts optional `branch`, `head_sha`, `base`, and `dirty` on both planes, and
  the shared session registry records them against the registered token so a
  later call resolves them. The client supplies the facts; the server performs
  no Git or filesystem inspection of its own, which is the only correct answer
  for a stdio server that may not share the agent's working directory and for a
  linked worktree whose branch differs from the database root. Declared context
  round-trips under the same token, including across a server restart, because
  identity is derived from the token rather than process state. A session that
  declares nothing is unchanged: no `context` in the response, no nulls, and
  nothing for a caller to guess at. A malformed declaration is refused rather
  than silently dropped. This is the substrate the staleness, divergence, and
  overlap-precision heuristics read from; it derives nothing on its own yet.
- **Isolated agent worktrees now share one repository brain and converge only
  through reviewed pull requests (ADR-0038, superseding ADR-0032).** Each clone
  bootstraps a random 128-bit `mindleak.repositoryId` in shared local Git config;
  both planes resolve to one platform-local, non-roaming
  `repositories/<id>/{graph.db,spec.db}` directory from every linked worktree.
  Independent clones remain isolated. Existing repository-root databases migrate
  once by verified SQLite online backup and are left untouched; `MINDLEAK_HOME`
  relocates the shared root, while direct DB overrides still win. VS Code,
  installer, Copilot CLI, and dogfood registrations no longer force
  worktree-local databases, and both planes expose `storage_status` for the id,
  resolved path, origin, legacy source, and migration result. The canonical
  publisher now accepts any clean attached worktree, refuses protected branches,
  dirty/detached/divergent state, and pushes exact `HEAD` to the same branch;
  only protected PR merge advances `main`. Fleet-delivery v2 proposes isolated
  worktrees and branch-owned publication without mutating immutable pack v1.
- **Bounded waivers: the reviewable form of `--no-verify` (ADR-0026 task 5,
  SPEC-CONSTITUTION §9, [ADR-0039](docs/adr/0039-waivers-end-amendments-change.md)).**
  `grant_waiver` records a scoped, expiring, attributed
  exception to one clause, and `revoke_waiver` withdraws it. An exception was
  always possible — `--no-verify` and a commented-out check are exceptions too,
  just unattributed, unbounded, and invisible. A waiver is the same act made
  reviewable.
  **Every waiver ends.** There is no open-ended waiver, because an exception
  that never expires is not an exception — it is the policy, and changing policy
  is an amendment. That one refusal is what stops the waiver table becoming a
  second constitution nobody reviewed. Granting also refuses a clause that
  declares itself unwaivable (otherwise `waivable: false` is decorative) and an
  approver who is not the authority the clause names — so an agent session
  cannot approve an exception to a clause reserved to a human.
  **Expiry is not a status transition.** A lapsed waiver keeps `status: active`
  and simply stops matching, so enforcement returns with nothing having run and
  history reads as it was judged rather than being rewritten by the passage of
  time. Revocation is immediate for future checks, attributed, and never a
  delete — the exception happened, and the record survives.
  A waived breach is **not** silent: the conformance findings name the waiver,
  its approver, and its expiry, so a waived change and a change that never
  touched a governed node are distinguishable in the audit.
- **Waiver state is part of the conformance token (ADR-0025).** A check made
  while an exception was in force is not evidence about a world where it was
  revoked, and one made under enforcement is not evidence about a world where an
  exception was since granted. The token records each waiver's status *and*
  expiry, so it also stops matching once a waiver lapses — which no row rewrite
  would otherwise signal.
- **`clause_waivers` / `active_waivers` make exceptions countable.** How often a
  rule has been excepted is usually the more useful question than what is
  excepted right now: a clause waived repeatedly is a clause that wants
  amending.
- **Amendments: changing adopted policy explicitly (ADR-0026 task 5,
  SPEC-CONSTITUTION §9).** `propose_amendment` drafts the next constitutional
  version **carrying every active clause forward**, so the draft starts as the
  current policy and the eventual diff shows only what the author actually
  changed — an empty draft would make every amendment a re-adoption of the whole
  constitution and report every untouched rule as removed and re-added.
  `amend_constitution` then promotes it with an attributed rationale and a
  stored clause diff, superseding the outgoing version and its clauses rather
  than deleting them, so a prior conformance record keeps naming the policy it
  was judged under.
  It is deliberately a **different call** from `activate_constitution`: adopting
  a first constitution and changing an adopted one are different acts, and only
  the second retires rules people are currently working under. It refuses an
  amendment that changes nothing (a no-op version bump would retire and re-issue
  every clause identically, invalidating live conformance tokens for no reason),
  one that leaves no clauses at all, and one carrying an undecided proposal.
- **`constitution_diff` matches clauses on `slug` and compares the enforcement
  contract, not just the words.** A restated rule reads as `changed` rather than
  a removal plus an addition; and a clause whose consequence moves from `review`
  to `block`, or whose scope widens, is reported even when its statement is
  identical — the quiet amendment a statement-only diff would miss entirely.
- **`plan_pack_upgrade` compares a newer pack against what was actually
  adopted.** A proposal, never an upgrade: upstream can never alter active local
  policy, so planning is a pure read. It compares against the recorded
  provenance rather than the local clause, so a tailored clause does not read as
  an upstream change — and clauses that *were* tailored are flagged, because
  accepting an upstream change to one is the single way a pack upgrade can
  silently discard a deliberate local decision.

### Changed
- **`make design-audit` now reports a superseded ADR as drift rather than as an
  unrepresentable note.** It reported those two files as a modelling gap because
  neither side was stale — they were saying different things, and the ledger
  could not hold one of them. It can now, so a file claiming supersession the
  ledger has not been told about is ordinary drift, and so is the reverse. The
  `unrepresentable` category and the `isDrift` predicate that existed only to
  exempt it are both gone.
- **Scope matching moved to one shared `scope` module.** Clauses and waivers
  both declare scope, and forking the matcher would let the two disagree about
  what a scope reaches. It stays deliberately not a glob engine — exact match,
  or a trailing `**` — because the point of a bounded exception is that a
  reviewer can see how far it goes.
- **A clause's enforcement contract now also declares waivability and
  authority.** A clause that can block should say whether it can be excepted and
  by whom, and the default is `false`, so a clause refuses exceptions by
  omission rather than granting them by omission.
- **Reviewed ratchets: a metric that must not regress, bound to a clause
  (ADR-0026 task 4, ADR-0034).** `register_ratchet` binds a metric and a
  direction to one constitutional clause; `accept_ratchet_baseline` records the
  value it compares against, attributed to the accepting session; and
  `observe_ratchet` reports a measurement and resolves it through the clause.
  Three refusals make the mechanism trustworthy. A ratchet with **no reviewed
  baseline reports `unknown`, never `pass`** — reporting conformance it never
  checked is how an unbaselined ratchet certifies nothing while looking green.
  A ratchet **never moves its own baseline**: a mechanism that adopts whatever it
  last measured launders a regression into the new normal, so one bad run would
  quietly ratchet the standard *down*. And accepting a baseline **bumps the
  control version**, so an observation taken against the old baseline resolves as
  `unknown` rather than being silently re-judged against a number it never saw.
  A failed ratchet resolves at `review` however hard its clause declares, because
  its power is `observed` — it reads a report and proves what already happened,
  it stopped nothing, and whether a particular regression is acceptable is a
  judgement about the change (SPEC-CONSTITUTION §4). The adapter is deliberately
  generic and the engine ships no coverage ratchet: §4 says a ratchet cannot
  determine whether coverage is the right proxy for confidence, so baking one in
  would answer, for every project, the one question the mechanism is not
  entitled to answer.
- **`clause_controls` shows the mechanisms behind a rule.** Each control lists
  the enforcement power it actually has and the ceiling that power implies, so
  the hardest consequence a clause can reach is inspectable rather than assumed.
- **ADR-0037 records why a ratchet never sets its own baseline**, refining
  ADR-0034 with the question SPEC-CONSTITUTION §4 raises and leaves open —
  whether the baseline was trustworthy — and the four refusals that answer it.
- **`constitution_status` reports adoption state instead of leaving it inferred
  (ADR-0026 task 3).** An agent could previously read the active clause set but
  not tell an ungoverned project apart from a governed one that happens to
  permit the change — both looked like "nothing stops me". The new read-only,
  model-free tool reports `absent`, `draft`, or `active` with the version and
  its clause count. An activated version always wins over a later draft, so a
  project mid-amendment still reads as governed, and a draft never reads as
  governing policy (SPEC-CONSTITUTION §7.5).
- **`propose_constitution` drafts policy from cited repository facts, and never
  activates it (ADR-0026 task 3).** Bootstrap classifies supplied repository
  paths — README, AGENTS.md, contributing guidance, ADRs, manifests, CI,
  linters, test configuration, ownership — into durable *project facts*, then
  drafts a constitutional version grounded in them and proposes the Common Core
  for review. Discovery reports evidence, never clauses: an existing CI gate
  proves the project uses a mechanism, not the reason, scope, or desired
  consequence, so every mechanism fact carries the question its configuration
  cannot answer while stated intent carries none. Classification is by path
  alone over paths the caller supplies, so the server performs no filesystem
  scan and discovery stays a deterministic, model-free function. The result is
  always a `draft` with every Common Core clause left undecided; an already
  active constitution is refused as an amendment, and an unresolved draft is
  refused rather than stacked.
- **`activate_constitution` closes the bootstrap loop (ADR-0026 task 3).** A
  reviewed draft becomes governing policy through one atomic transaction that
  validates and promotes together, so a concurrent writer cannot slip an
  undecided clause or a second activation between the checks and the write. It
  refuses a draft with any undecided clause proposal (SPEC-CONSTITUTION §7.5
  forbids silent grandfathering), a draft with no clauses, anything that is not
  a draft, and activation while another version is already active. Adopted
  clauses inherit their version's status, so they are drafts that govern nothing
  until activation promotes them alongside it; a refused activation leaves the
  draft completely untouched. Activation is attributed and needs no model.
  Together with `constitution_status` and `propose_constitution`, an ungoverned
  project can now go from no policy to governing policy deterministically.
- **Immutable policy packs and the five-principle Common Core (ADR-0026 task
  2).** Lodestar validates a canonical SHA-256 digest and engine compatibility
  before registering a pack version; the same id/version/digest is idempotent,
  while changed bytes under an existing version are refused. Pack clauses enter
  a durable adopt/tailor/reject review ledger. Adoption atomically materializes
  a self-contained local constitutional clause plus immutable source pack id,
  version, digest, key, and original content; rejection persists so bootstrap
  cannot repeatedly propose it. Declared pack conflicts route to human review,
  and a newer pack version cannot rewrite an adopted active clause (it requires
  the later amendment workflow). `propose_common_core` uses this same path for
  evidence, intent, safety, proportionality, and evolution principles; MCP
  review is attributed to a registered session.
- **`CHANGELOG.md` is assembled from per-change fragments (ADR-0056).** A pull
  request now adds `changelog.d/<section>-<slug>.md` and does not touch the
  changelog. The shared file was a serialisation point: `.gitattributes` marks
  it `merge=union`, git honours that in a checkout, and GitHub's merge machinery
  does not — so five pull requests in one day reported a conflict that did not
  exist, `gh pr update-branch` could not clear it because that is a server-side
  merge too, and each one had to be reconciled by hand. The real cost was not the
  conflict but that **auto-merge silently stopped working**: armed work went
  stale the moment anything else landed, which is precisely what "armed means
  finished" was supposed to rule out. Two branches never write the same fragment
  path, so there is nothing to merge. `node scripts/changelog.mjs --release
  <version>` folds the fragments, and anything already under `## [Unreleased]`,
  into a dated section once, in the release commit. This is the same shape as ADR
  numbers and the ADR index table, and the same fix both of those already got:
  stop hand-maintaining what can be computed.
- **Recall can now say "I don't know", and conclusions get recorded (ADR-0053).**
  Measured on this repository's own index: asking `recall` the five questions an
  agent actually had during a day's work returned code locations, never
  experience — "canonical-push auto-merge armed refuses" came back with
  `merge_import`, a symbol matched on the word "merge", with nothing to mark it
  as noise. A caller handed a plausible stranger cannot tell it is wrong, so it
  stops asking, and that is the whole adoption problem. `recall` now applies a
  cosine floor (`MINDLEAK_RECALL_FLOOR`, default 0.5) and returns **nothing**
  when nothing clears it. An honest empty answer is usable: fall back to
  `multi_hop_query`, `graph_snapshot`, or the repository.
  The other half is that there was nothing worth recalling. A 500-node sample of
  the graph held 196 executions, 159 symbols, 120 artifacts — and no conclusions,
  because nothing ever asked for one. `complete_task` now takes `learned` and
  records it as durable knowledge at the moment the agent holds it; omitting it
  never blocks completion, because most tasks teach nothing and a gate would only
  produce a column of `n/a`, but the response names the omission so the gap is
  measurable instead of invisible. `record_architectural_decision` embeds the
  node it writes, so a recorded conclusion is recallable immediately rather than
  queued until someone remembers to run `index_nodes`; when no embedding server
  is reachable the node is still written and `embedded: false` says so. The
  zero-token write path is untouched: a conclusion is supplied, never inferred
  from an execution log.

### Fixed
- **`check_overlap` no longer reports an agent colliding with itself
  (ADR-0054).** The Memory Plane carried the same forked identity the Intent
  Plane did: attribution nodes are `agent:{id}` (ADR-0003), so one session
  observed under two process environments produced two agent nodes. This was
  first assessed as cosmetic and it was not — `check_overlap` skips the caller
  by exact id, so an agent's other half was never excluded and the tool reported
  a collision with itself, a false positive indistinguishable from a real one
  that would tell an agent to back off work nobody else was doing.
  `working_set` likewise returned half of an agent's own attention. The
  migration folds the halves rather than picking one: the canonical node takes
  the earliest creation and latest activity, and a shared observation takes the
  strongest weight, the latest touch, the earliest first sighting, and the
  **summed** reinforcement count — a node observed under both names really was
  observed twice. Verified against the live graph, where 17 agent nodes still
  carried a label segment after the Intent Plane migration had already run.
- **One agent is one identity again (ADR-0054).** The agent id was
  `session:v1:{base}:{fingerprint}`, where the fingerprint came from the session
  token but `base` came from `LODESTAR_AGENT` / `MINDLEAK_AGENT` **in the hosting
  server process**. Because every comparison in the system is whole-string
  equality, one session hosted by two differently configured processes resolved
  to two agents. Observed live: `fleet_view` listed
  `session:v1:agent:bff9…` holding one claim and `session:v1:copilot:bff9…`
  holding two — same token, same fingerprint, three claims split in half; two
  further agents in the same fleet were split the same way. Worse, an addressed
  question is matched on `audience = ?1` exactly, so a peer addressed under the
  other half's name never sees it and the task parks until the grace expires —
  which made agent-to-agent dialogue (ADR-0046) undeliverable in practice. The
  id is now `session:v1:{fingerprint}`, derived from the session token alone;
  the label survives as a display name that is written to no key. A migration
  collapses stored ids across all sixteen identity columns, which *heals* the
  existing split because both halves share the fingerprint and merge onto one
  agent. `DEVELOPERS.md`'s "publishing must use the same base that claimed"
  instruction is deleted rather than amended — it was advice for working around
  this bug.
- **An unknown tool argument is now reported instead of silently dropped.**
  Passing `lease_seconds` where `claim_task` declares `lease_secs` did nothing
  visible: the key was ignored, the 300-second default applied, and the claim
  lapsed mid-work. The only symptom was an expired lease, so the typo read as a
  server bug and cost twenty minutes of wrong diagnosis before the real cause
  surfaced. A caller naming an argument a tool does not have is wrong about the
  contract, and the cheapest moment to say so is immediately — the error now
  names the offending key and lists what the tool accepts. Checked at the tool
  boundary against the schema each tool already advertises, so there is no
  second list to drift. Only top-level names are validated, and the keys that
  belong to the call *envelope* rather than to any tool's contract are exempt:
  `agent`, `resolved_agent` and `resolved_context`, which `bind_session` adds
  itself, and `session_id`, which every client adds to every call in one place
  while `apply_session_contract` only declares it on tools that require a
  session. Treating the envelope as an argument rejected every call to a tool
  that needs no session — `board`, `design_board`, `graph_stats` — which took
  the extension's whole readiness path down to `disconnected`. Who supplies an
  envelope key is not what makes it one.
- **A wrapped `Status:` line no longer loses its reference.** ADR-0032 writes
  `- Status: Superseded by` and puts `[ADR-0038](...)` on the next line.
  `scripts/adr-files.mjs` read to the end of the line, so the file parsed as a
  bare `Superseded by` — and that value reached the ADR index table, the
  `make design-audit` report, and a question put to a human as "nobody can tell
  what replaced it". The answer was one line further down, and the superseding
  commit names ADR-0038 in its `DECISION:` line. The parser now reads indented
  continuation lines, and the ADR index row for 0032 names its successor again.
- **The server no longer exits at startup when the database path has no
  directory.** `MINDLEAK_DB=":memory:"` — or a bare `graph.db` — resolves to a
  path whose `parent()` is `Some("")`, not `None`. `create_dir_all("")`
  short-circuits to `Ok`, so the happy path hid it, but the Unix branch then
  called `set_permissions` on that empty path and got `ENOENT`. The process
  died immediately on Linux and macOS reporting only "No such file or directory
  (os error 2)", while Windows started fine because it has no permissions call.
  This is what failed the v0.1.3 release: both platform jobs that actually
  executed a Unix binary failed their smoke test and publication was skipped.
  The macOS x64 job passed only because it is cross-compiled and skips
  execution — a green matrix cell is not always an executed one.
- **A release platform nobody could execute no longer reports itself as
  verified.** The release smoke ran the freshly built servers only when the
  runner's architecture matched the target, and otherwise printed a notice and
  returned — so the job went green having tested nothing. `macos-x64` builds
  `x86_64-apple-darwin` on `macos-14`, which is arm64, so **two of the four
  v0.1.3 platforms were never smoke-tested** and a startup crash reached a
  tagged release with green ticks beside it. The x64 macOS build now runs on
  `macos-15-intel` so every target is native, and a mismatch is a hard failure
  rather than a skip: a binary this workflow cannot execute is one it must not
  ship. A check that reports success on a question it never asked is worth less
  than no check, because it is trusted. The Intel label had to be a currently
  hosted one — `macos-13` was tried first and is retired, and an unknown
  `runs-on` label does not fail the job, it leaves it queued forever.
- **Re-registering a session no longer erases where it said it was working
  (ADR-0044).** `canonical-push` re-opens the session on every publish purely to
  learn its own agent id, declaring no context. That overwrote the stored
  declaration with an empty one, so `fleet_view` reported `branch: null` for
  agents that had declared a branch minutes earlier — the fleet went blind at
  exactly the moment it was busiest, and the tool that blinded it was the one
  added to record where everyone is working. Declaring nothing is not a claim to
  be nowhere: a call that declares no context now leaves the stored context
  alone, in both the in-process registry and the durable row. Within a real
  declaration the replace-wholesale rule is unchanged, because there an omitted
  field is the client saying that field is no longer known.
- **A lapsed lease no longer lets an agent launder unchecked work into an
  `aligned` receipt (ADR-0048).** Re-claiming after a lapse reset
  `claim_started_at` to the moment of the re-claim, so everything done before the
  lapse fell outside the interval the agent was allowed to submit and
  `check_conformance` rejected it with "evidence interval falls outside the live
  claim". That read like under-reporting, but the verdict is computed over
  whatever the evidence covers: the only way forward was to narrow the interval
  until it was admitted, and the narrowed interval passed on the surviving sliver
  and returned `aligned`, which sends the task straight to `done` with every
  governed change made before the lapse never examined by anything. A lapse now
  punches a hole in the window instead of moving it — a same-owner re-claim keeps
  `claim_started_at`, so the earlier work stays provable, while new
  `claim_lapses` and `unleased_seconds` columns record the discontinuity and cap
  the verdict at `needs_human` with a finding naming it. The cap follows the
  task rather than the submitted interval, so shrinking the evidence no longer
  buys a clean pass. A claim by a *different* owner still opens a fresh window,
  so reach-back can never cross a period somebody else owned the task. Both
  columns are additive with defaults; existing databases migrate without
  backfill and windows already open are treated as continuous.
- **A current build could not open an existing database.** Indexes lived in
  `schema.sql` and therefore ran *before* migrations. On an existing database
  `CREATE TABLE IF NOT EXISTS` is a no-op, so the pre-migration table shape was
  still in place when `idx_task_qa_audience` tried to index
  `task_qa(audience, kind)`. The batch failed with `no such column: audience`,
  the migration that would have added the column never ran, and every
  pre-existing database became unopenable — a hard upgrade failure rather than a
  degradation, and silent until someone ran a fresh binary. Indexes now live in
  `indexes.sql` and are applied *after* migrations, so the ordering is
  structural rather than something each new migration has to remember.
- **An ADR whose status carries a parenthetical is no longer dropped from the
  design ledger in silence.** `Accepted (implemented)` is still accepted — a
  parenthetical is commentary on a decision, not a different lifecycle state.
  Requiring an exact match meant ADR-0015 and ADR-0017 were never registered at
  all, while the sync kept reporting success with a quietly lower count. An
  accepted decision the ledger has never heard of is precisely what the ledger
  exists to prevent, so the parser now maps a qualified status to its lifecycle
  state, and any ADR it still cannot read is reported by path and reason to the
  output channel and a warning rather than skipped invisibly.
- **One unreadable materialization no longer blanks the whole Design Board.**
  The refresh fanned `design_promotion` out across every materialized design
  with `Promise.all`, so a single rejection rejected the batch and the view kept
  its stale contents behind one error toast — the board looked out of date
  rather than broken. It now settles each lookup independently, logs the failed
  design id, and renders every row it could read.
- **The extension relaunches its MCP server instead of going quietly dead.**
  The VS Code extension spawns its own `mindleak-mcp` and `lodestar-mcp`
  children, and nothing restarted them when one exited mid-session — a crash,
  or an external `taskkill` while the release binaries were rebuilt. Every
  MindLeak pane then stayed blank until the window was reloaded, with only a
  line in the output channel to say why. The client now relaunches the server
  itself, up to three consecutive attempts before it reports that a reload is
  needed. The exit handler also stays silent when the exit came from disposal,
  which is what raised `Channel has been closed` in the extension host log
  during teardown, and the exit message now names the server that actually
  exited rather than always saying `mindleak-mcp`.
- **The health line follows the server instead of the moment it started.**
  `activate()` recorded `memory connected` / `intent connected` once and never
  revised it, so a server that died mid-session left the one surface meant to
  explain the silence confidently wrong. `McpClient` now publishes
  `connected` / `reconnecting` / `disconnected`, and the extension maps that
  onto the plane's health line. The four independent health strings collapsed
  into the `RuntimeHealth` record they already modelled, behind a single
  change-guarded setter.
- **The Design Board no longer fails silently, and an accepted ADR plans real
  work.** Materializing an accepted design aborted with no message, no log
  entry, and no state change whenever a quick pick or input box was dismissed,
  so a cancelled run looked exactly like a broken one and designs sat at
  `pending` indefinitely. Every abort path now reports and logs, and choosing
  *Create new tasks* with no active objective goal explains why instead of
  closing. Repository ADR synchronization also registered every design with an
  empty summary, so Create-mode planning saw only the ADR title and drafted
  generic filler tasks; design metadata now carries the ADR's `## Decision` and
  `## Context` text — bounded, and truncated at a line boundary — into the
  design item that planning reads.
- **Repository ADR reconciliation refreshes derived facts instead of freezing
  them.** `reconcile_designs` used `INSERT OR IGNORE`, so a design item kept
  whatever title and summary it was first registered with — including an empty
  summary recorded before summaries were extracted at all, which left promotion
  planning with nothing but an ADR title to work from and no way to repair it.
  Reconciliation now refreshes `title` and `summary` from the ADR file, while
  every durable decision (`status`, `decided_by`, `reason`, the original
  proposer, and promotion state) still survives a repository pass that disagrees
  with it. `updated_at` moves only when a fact actually changed, so a no-op pass
  remains genuinely idempotent.
- **Accepting a design no longer guesses which checkout records the decision.**
  `resolveAdrUri` wrote the ADR's `Status:` line into the first workspace folder
  containing that path. Under ADR-0038 a fleet routinely has several worktrees
  of one repository open on different branches, so first-match was close to
  arbitrary: observed writing `Accepted` into a checkout whose branch had no
  relationship to the decision, while that checkout's agent was mid-pull-request
  and had accepted nothing. An ADR's declared status is evidence of a human
  decision, so recording it on the wrong branch is a falsified receipt, and the
  stray edit also lands in someone else's working tree. One matching checkout
  now writes as before; several ask the reviewer which one, and cancelling
  aborts without writing; none keeps the existing clear error. The fix
  deliberately does not bind a design record to a worktree — that would put a
  machine-specific path in a database ADR-0038 shares across checkouts.
- **A lease is now a heartbeat, not a deadline (ADR-0052).** Any authenticated
  call that names a task — `task_scope`, `ask_question`, `answer`,
  `conformance_history`, `advise`, `check_conformance` — renews its lease as a
  side effect. Observed repeatedly in one session: a claim taken with a 3600s
  lease lapsed during `cargo test --all`, and the push that followed was refused
  for having no live claim, three times, while the agent was working throughout.
  Making the heartbeat free is the same shape that made question delivery
  actually adopted in ADR-0046 — a capability that depends on remembering is
  adopted at a rate of zero, so it rides on calls already being made.
  The short default lease is unchanged, because that is what frees a vanished
  agent's work quickly; renewal-on-activity keeps that property instead of
  trading it away by raising the default. A heartbeat can only extend a lease,
  never shorten one an owner deliberately took long, and it leaves
  `claim_started_at` alone so the evidence window still bounds exactly what the
  claim covered (ADR-0048 is unaffected). It is owner-only and silent: a peer
  reading the task renews nothing, an already-lapsed lease is not resurrected by
  a passing call, and neither case errors — the call it rides on has its own job
  to do, and a lapse must still require a deliberate re-claim rather than
  undoing a claim someone else has taken.
- **A test now blocks any merge driver returning to `.gitattributes`, in any
  directory.** Removing the last `merge=union` declaration stopped the phantom
  conflicts, but nothing stopped one being added back. The guard was widened
  from "the root file declares no union driver" to "no tracked `.gitattributes`
  declares any `merge=` driver": git honours a nested `.gitattributes` exactly
  as hard as the root one, and GitHub's merge machinery honours none of them, so
  `ours` and `theirs` diverge from the local result the same way union did. The
  failure names the offending file and line.
- **Nothing declares `merge=union` any more, so phantom conflicts stop.**
  ADR-0056 took the driver off `CHANGELOG.md` but kept it on
  `docs/adr/README.md`, on the grounds that the index is generated and
  hook-guarded so union was "a convenience, not the mechanism". That was right
  about correctness and wrong about cost. **The convenience exists only in a
  checkout; the phantom conflict is what everyone else sees.** Within hours a
  pull request whose *only* both-sides file was the ADR index reported
  `CONFLICTING`, merged clean locally, and could not be repaired with
  `gh pr update-branch` — because that is a server-side merge too. Its
  auto-merge sat armed and silently stopped working, which is exactly the
  failure "armed means finished" (ADR-0045) exists to rule out. Six hand
  reconciliations in one day, and one duplicated `## [0.1.3]` heading that union
  merged happily into a release changelog.
  There is also a defect specific to a *generated* file: union merging a
  generated table can produce a duplicated or misordered one, and the hook then
  regenerates it anyway — so the wrong resolution was being computed and thrown
  away. A generated file is regenerated, never merged. The test asserts the
  declared set is now empty rather than being deleted, because the failure it
  guards is invisible locally: any file added back here merges cleanly in every
  checkout and blocks its pull request on GitHub with no way to clear it.
- **The overlap warning no longer blames a peer for your own claim.**
  `canonical-push` reported "another agent has a live claim over paths this
  branch touches" and named a task that was the publishing agent's own.
  Ownership was decided only by whether the task appeared in a list the caller
  derived from its own live claims — and when identity resolution drifted during
  the ADR-0054 migration, that list came back empty while the claim was plainly
  its own. The notice now compares the claim's owner against this session
  directly, so it is right even when the derived list is wrong. When the session
  has no identity to compare against it says so, rather than asserting the claim
  belongs to someone else. A warning that names the wrong party is worse than no
  warning: it sends someone to ask a question of themselves, and it trains
  readers to discount the next one.

## [0.1.2] - 2026-07-24

### Added
- **Session-scoped cross-plane identity and audited claim recovery (ADR-0030).**
  Clients register one opaque 128-bit `session_id` with both stdio servers and
  reuse it on identity-bearing calls. Both planes derive the same restart-stable
  `session:v1:<base>:<fingerprint>` identity; multiplexed chats no longer alias
  onto one process owner, and arbitrary per-call ids are ignored/rejected.
  `recover_claim` moves only expired compatible legacy owners into the current
  session, starts a fresh evidence window, preserves task scope/Q&A, and records
  the complete prior claim in append-only `task_claim_transfers`; the Intent
  Board exposes that guarded recovery path.
- **First-class GitHub Copilot CLI registration (ADR-0033).** Both stdio planes
  already run under any MCP client, but the installer registered them only into
  `.vscode/mcp.json` — which the `copilot` CLI cannot read (it keys servers under
  `mcpServers`, not `servers`) and whose `${workspaceFolder}` variable the CLI
  does not expand. The canonical installer now also writes a CLI-ready
  `.mindleak/copilot-mcp.json` with absolute, install-time paths and the same
  local stores, merged with the same comment- and server-preserving guarantees as
  the VS Code path (shared `mergeServerRegistrations` — one source, rendered for
  both clients). Point the CLI at it with
  `copilot --additional-mcp-config @.mindleak/copilot-mcp.json`, or merge the
  block into the user-level `~/.copilot/mcp-config.json`. Identity (ADR-0030) and
  the stdio-only, unauthenticated transport are unchanged.
- **Constitutional representation and goal migration (ADR-0026, SPEC-CONSTITUTION
  §10, task 1 of 6).** Goals are now clauses of an explicit, versioned
  constitution. A new `constitution_versions` record freezes the attributed
  policy snapshot (purpose, preamble, project identity, lifecycle state), a
  `principle` goal kind captures broad decision rules that route to review, and
  each clause carries provenance (`origin`), `rationale`, `scope`, an
  `evidence_contract`, a proportional `consequence` (advise / review / block),
  and a waiver policy (`waivable`, `waiver_authority`). A clause is enforceable —
  and can hard-block — only once it declares scope, evidence, and consequence;
  until then it is review-only. Existing active goals migrate into a first
  **local** version (`constitution:v1`) with honest self-attribution and no
  invented purpose, preamble, authority, or enforcement contract; the migration
  is idempotent. `active_constitution_version()` reports the absent / draft /
  active state.
- **`resolve_task` closes the `in_review` gap — the task-level mirror of
  `accept_design`.** A docs-only task inside an objective's chain (not a
  registered design item) still lands `in_review` on a `drift`/`needs_human`
  conformance verdict, and until now had no accept-to-`done` verb — only
  `reopen_task`/`abandon_task` existed. `resolve_task(task_id, human)` (facade +
  MCP) records the attributed human judgement and moves the task to terminal
  `done` with no code-conformance re-run, opening any blocked successor so a
  docs/needs-human predecessor never strands its chain. Human-in-the-loop: a
  reviewer identity is required and may not be the agent whose work is under
  review (read from the task's conformance evidence), mirroring the
  no-self-decision guard on `accept_design`.
- **The VS Code Workspace view guides first value without becoming a source of
  truth (ADR-0027).** A pure readiness projection combines the two MCP
  initialize build identities, `graph_stats`, actionable `board`/`design_board`
  rows, one per-activation identity shared exactly by both child servers, and
  terminal/Git health into five states:
  disconnected, ready-empty, observing, coordinating, and optional degradation.
  Each state exposes one concrete action (settings, first ingest, Context Graph,
  Intent/Design Board, or Telemetry); a fresh empty/disconnected workspace focuses
  the view once and can then be ignored. The clean VS Code 1.93 Extension Host
  smoke now proves empty workspace → active-file ingest → non-empty graph →
  proposed design coordination with no model or manual JSON. README/USAGE show
  the identical ordered MCP path for headless clients.
- **Conformance evidence is exportable, verifiable proof-of-work (ADR-0031).** The
  evidence-backed conformance loop (ADR-0009/0025) already refuses to let an agent
  mark work "done" by asserting it — `complete_task` consumes only a bounded,
  provenance-bearing bundle that a separate `check_conformance` scores against the
  goal's code bindings, bounded by the live claim and attributed to the acting
  agent. Now that proof can leave the local ledger: `export_evidence` renders a
  task's durable `conformance_history` chain (check id, verdict, acting agent,
  claim window, evidence summary) as a committed, verifiable artifact for human
  review, a CI conformance gate (`scripts/conformance-gate.mjs`), and audit. The
  gate is fed by `export_conformance_manifest`, which renders the repo-wide
  governed-node set plus each task's verdict and covered nodes as a machine-checkable
  JSON manifest, so CI fails a merge that changes governed code without an aligned
  receipt. README and ARCHITECTURE now explain why this chain is the fleet's only
  trustworthy proof-of-work — narration is not proof.
- **The Evidence Board surfaces conformance proof in VS Code (ADR-0031).** A new
  tree view lists tasks that carry a conformance chain, each showing its latest
  verdict and expandable to the individual checks (the drift→aligned story a
  reviewer needs), with inline **Inspect** (render the chain as markdown) and
  **Export** (`export_evidence` to a committed artifact under `.lodestar/evidence/`)
  actions. The grouping logic (`evidenceGroups`, `verdictIconId`) is pure and
  vitest-tested; the vscode-coupled provider stays thin.
- **Ask-before-act constitutional advice (ADR-0029).** Agents can now ask what
  governs an intended change *before* doing it, not only discover drift at
  `complete_task`. The new `advise` tool takes the `artifact:`/`symbol:` ids you
  are about to change (and an optional covering task) and returns the governing
  clauses plus a proportional disposition — advise / review / block / needs_human
  — with no evidence, no recorded verdict, no task-state change, and no model
  dependency; it never gates the compare-and-swap claim. `claim_task` and
  `next_task` now surface the clauses governing a task on pickup,
  `governing_for_task` exposes them for any task, and the VS Code Intent Board
  shows them on a claimed task. AGENTS.md makes consulting the advisory a
  claim-time ritual, with retrospective conformance (ADR-0009/0025) as the
  backstop.
- **Pre-flight duplicate-work awareness across both planes (ADR-0024).** A
  Lodestar claim can atomically declare advisory path globs and opaque MindLeak
  symbol ids; `task_scope`, scope-enriched `board`, and Lodestar `check_overlap`
  expose intersections with live claims. MindLeak's same-named read derives
  other agents' direct or mutation-linked artifact/symbol footprint after decay
  filtering, with effective weight still computed at query time. The VS Code
  allocator collects concrete paths/symbols, combines both reads before the
  claim CAS, shows an explicitly overridable warning, and renders scoped work on
  the Intent Board. A locked, source-hashed two-agent benchmark proves the blind
  two-owner control, live claim + footprint detection, 336-hour decay control,
  read-only checks, and a successful `blocked_by` steer; it does not claim agents
  always obey advisory output.
- **`forget_file` reaps a deleted file's structure instead of leaving it to
  decay for a month.** When a file is deleted or renamed in the editor, its
  symbols and artifact node — and every edge touching them — used to linger in
  the graph until decay pruned them (~30 days), and a live graph showed hundreds
  of such stale nodes for moved/split source. The new `forget_file` tool reaps
  that structure outright (a vanished file's structure is definitively invalid),
  and the VS Code extension calls it on `onDidDeleteFiles` / `onDidRenameFiles`
  (editor-mediated events only, so a file briefly absent during a git operation
  is never wrongly reaped). Historical intent and execution nodes remain; only
  their edges to the gone file are cut.
- **`reconcile_workspace` clears accumulated stale structure in one pass.**
  `forget_file` only catches deletions the editor sees going forward; files
  deleted or moved before it existed (or via a terminal `git rm`) leave structure
  that lingers until it decays. The new `reconcile_workspace` tool takes the
  workspace's current file set and forgets every file artifact not in it (plus any
  build/VCS junk), and the extension runs it once on activation from an authoritative
  `findFiles` listing (also available on demand via *MindLeak: Reconcile Graph with
  Workspace Files*). No server-side filesystem scan, so a file briefly absent
  during a git operation is never reaped.
- **Telemetry distinguishes a resolved historical error from a currently failing
  tool (ADR-0010).** The append-only trail means a tool's lifetime `errors` never
  shrinks, so a single past failure used to read as a permanent fault in the VS
  Code Telemetry pane and the Markdown rendering. `telemetry_snapshot` now also
  reports, per tool, `last_success_at`, `last_error_at`, the most recent error's
  `detail` (retained as an audit path even after the raw event ages out of the
  bounded recent window), and a derived `currently_failing`, plus a snapshot-level
  `currently_failing_tools`. The pane gains a "Failing now" health card and a
  per-tool Health column; the Markdown table separates lifetime errors from
  current health. Lifetime totals stay cumulative history; current health is a
  separate append-order signal, including when calls share a timestamp.
- **Two productization decisions make the viability gaps explicit.** ADR-0027
  establishes an extension-led, five-minute first-value workflow over the existing
  portable MCP primitives, without duplicating authoritative state or requiring
  a model. ADR-0028 separates engineering, controlled-efficacy, and external-
  adoption evidence; it defines the privacy-preserving post-v0.1.1 developer
  pilot required before broad product claims or roadmap expansion.
- **Design promotion is reviewed before it creates work (ADR-0023).** Acceptance
  records only the human decision. `plan_design_promotion` previews a concrete
  multi-objective create/link/no-work plan; `promote_design` writes the reviewed
  plan atomically; and `revise_design_promotion` appends an attributed repair
  without deleting prior plans or tasks. This fixes the ADR-0028 duplicate/orphan
  materialization failure while keeping optional model output out of writes.

### Changed
- **Fleet integration now preserves one canonical history (ADR-0032).** Work uses
  one primary checkout, one fleet branch, and one designated publisher.
  `canonical-push.mjs` refuses protected branches, linked worktrees, staged index
  state, and remote divergence; committed-snapshot hooks no longer depend on a
  side worktree.
- **The first-value path now has one product workflow across clients.** Workspace
  readiness, scenario-driven docs, and first-class Copilot CLI registration lead
  users through connection, one useful graph result, and optional coordination
  without requiring an account, model, or manual JSON.

### Fixed
- **Autonomous graph maintenance is reliable under normal editor activity.**
  Pruning runs on its own cadence instead of an idle window starved by UI polls;
  graph writes take `BEGIN IMMEDIATE` so SQLite's busy timeout serializes writers;
  transient execution attribution no longer outlives its evidence; and build/VCS
  output is rejected before it can pollute the graph.
- **The Context Graph remains bounded around high-degree nodes.** Seeded snapshots
  use relevance-first expansion with hard node/fanout limits instead of pulling
  an agent's entire observation history into Cytoscape.

## [0.1.1] - 2026-07-24

### Added
- **Two productization decisions make the viability gaps explicit.** ADR-0027
  proposes an extension-led, five-minute first-value workflow over the existing
  portable MCP primitives, without duplicating authoritative state or requiring
  a model. ADR-0028 separates engineering, controlled-efficacy, and external-
  adoption evidence; it defines the privacy-preserving post-v0.1.1 developer
  pilot required before broad product claims or roadmap expansion.
- **Read tools render as rich Markdown in chat while staying machine-parseable.**
  MCP tool results can now carry a chat-facing Markdown rendering in `content`
  *and* the structured JSON in `structuredContent`, so Copilot Chat shows a
  formatted table instead of raw JSON without breaking the programmatic consumers
  (the VS Code extension's panes, agents). The extension's `parseToolResult` now
  prefers `structuredContent`, falling back to today's JSON parse. Wired so far:
  `graph_stats`, `lodestar_stats` (count tables), `next_task` (a task summary
  card), and `telemetry_snapshot` (a per-tool metrics table); other inline read
  tools follow the same `rendered_result` / `rendered` pattern.
- **Pause and resume a claimed task from the Intent Board (ADR-0020).** The board
  now shows an inline pause action on a `claimed` task and a resume action on a
  `paused` one, calling the owner-guarded `pause_task` / `resume_task` tools for the
  task's owner, so work can be parked and picked up again without releasing the
  claim. A pure `leaseActionFor` helper guards a possibly-stale board row (vitest).
- **The graph now self-cleans: the maintenance worker prunes on idle.** Decay hid
  low-weight edges at query time, but the physical rows only left via a manual
  `prune_graph`, so the graph grew unbounded between calls. The idle maintenance
  worker now runs a deterministic, zero-token prune every pass — reaping decayed
  edges and the execution/symbol/stub nodes they orphan ([ADR-0021](docs/adr/0021-node-lifecycle-and-reaping.md))
  — so no manual pruning is needed. On by default (opt out with
  `MINDLEAK_AUTONOMOUS_PRUNE=false`) and independent of the model-dependent
  consolidation/index tier (`MINDLEAK_AUTONOMOUS_CONSOLIDATION`, still opt-in); the
  worker now starts when either is enabled and emits `autonomous_prune` telemetry
  with reap counts.
- **Design items — an accept→promote→decompose bridge for ADRs (ADR-0023).** An ADR
  can be registered as a first-class *design item* that carries the ADR's review
  lifecycle: while `proposed` it is tainted — it lives on a new **Design Board**
  and never appears in `next_task` or the executive board. `accept_design` is the
  attributed human decision *only* — it does **not** run ADR-0009 code conformance
  (a design decision has no code to conform to) and does **not** create tasks,
  resolving the `in_review` dead-end where design/ADR tasks stranded forever; the
  design becomes `accepted` with promotion state `pending`. The separate,
  **idempotent** `promote_design(id, objective_goal_id)` then materialises the work
  in one step: it decomposes the reviewed design into claimable tasks under the
  chosen objective (model-assisted, deterministic single-task fallback), registers
  any mandated constraints/invariants into the constitution, and records durable
  design→goal / design→task provenance links — so a retry returns the same plan
  instead of duplicating it, and a failed decomposition leaves promotion `pending`
  without undoing the acceptance. Keeping the optional model call out of the
  acceptance write means it never serializes unrelated writers. `reject_design` is
  durable and auditable (archive-not-delete). No agent may decide its own design
  (human-in-the-loop). `reconcile_designs` idempotently imports structured
  Proposed/Accepted/Rejected ADR metadata without a model and without creating
  goals or tasks; existing human decisions and promotion state always win.
  `design_board` now returns proposed decisions plus accepted designs awaiting
  promotion or retry. New tools: `register_design`, `reconcile_designs`,
  `design_board`, `accept_design`, `promote_design`, `reject_design`. The VS Code
  sidebar now ships the separate Design Board and workspace ADR sensor: it syncs
  structured ADR metadata on activation/change or manual command, exposes
  attributed accept/reject and objective selection for promotion, keeps failed
  promotion pending/retryable, and renders persisted objective/task/constraint
  provenance for materialized designs.

### Changed
- **The install and usage on-ramp is action-first and easier to follow.** The
  Quickstart now leads with a three-step download-register-restart happy path,
  adds a "confirm it's connected" tool-list check with the one common failure
  cause, and hands the reader a ready-to-paste first prompt that exercises the
  full query -> act -> write-back memory loop. Verifying the release archive is
  presented as a recommended step rather than a prerequisite that blocks first
  value. The README download section leads with the install command and links to
  the walkthrough, and its `.vscode/mcp.json` example now sets `MINDLEAK_AGENT`
  so agent attribution works out of the box.

### Fixed
- **Extension coverage no longer false-fails the 80% gate on Windows.** The V8
  provider's default `all` baseline walk resolved each included file under the
  OS's uppercase drive letter while recording executed coverage under the
  lowercase drive (`file:///c:/...`), so every file was counted twice — a real
  entry plus a phantom 0% one — halving the reported line total (38.6%) and
  failing `test:coverage` on Windows even though real coverage is ~89%. The
  vitest config now reports only executed in-scope files, which every listed file
  is, restoring an accurate cross-platform number.
- **The VS Code MCP client no longer hangs on an unresponsive or missing server.**
  A spawn failure (for example a misconfigured server path) now rejects
  `start()` instead of leaving activation waiting forever; every request carries a
  timeout (default 30s) so a live-but-silent server surfaces an error rather than a
  stuck command; and `stdin` write failures are guarded and logged instead of
  raising an unhandled stream error.
- **The VS Code Intent Board now allocates work instead of merely displaying
  ownership.** Open and expired-claim rows expose claim-for-me and explicit-agent
  allocation with bounded leases; live claims expose owner-explicit renew and
  release actions. Rows show claim windows and live/reclaimable state, and **Next
  Claimable Task** reveals Lodestar's scheduler choice without auto-claiming it.
  CAS loss, stale owner, expiry, and parked ownership remain visible failures —
  the portal does not invent a parallel assignment store or false lock.
- **Intent Board cleanup now handles stale live work, not only completed rows
  (ADR-0019).** Eligible open, in-review, blocked, and expired-claim rows expose
  a confirmed **Retire Task** action that calls `abandon_task`; the task and its
  conformance history remain durable but leave the operational board. Live
  claims and parked ownership remain protected. ADR-0019 now records the shipped
  hide-never-delete model instead of proposing a second archive lifecycle.
- **The VS Code Intent Board no longer grows forever with completed history.**
  It now requests Lodestar's live/actionable view and defensively filters
  terminal `done` / `abandoned` rows before rendering. The durable task and
  conformance records are unchanged and remain available through
  `board(include_terminal=true)`; stale live work can still be deliberately
  retired with `abandon_task`.
- **Expired leases can no longer be renewed with a stale evidence window.**
  `renew_lease` now succeeds only while the caller's lease is still live. After
  expiry, the owner must win `claim_task` again, which resets `claim_started_at`
  just like any other re-claim and gives conformance one unambiguous work window.
- **`decompose_goal` refuses normative goals, so `next_task` stops handing out
  zombie tasks.** Decomposing a `constraint`/`invariant` goal produced tasks that
  merely restate the rule and can never accrue completion evidence; `next_task`
  (oldest-first) then surfaced one on every call, burying real work. Decompose now
  returns a typed `LodestarError::Invalid` for normative goals (only `objective`
  goals decompose), the four pre-existing restatement tasks were retired with
  `abandon_task`, and a regression test proves `next_task` surfaces actionable
  work instead of a restatement.
- **`index` and `consolidate_session` no longer stall on the model network path.**
  The optional embedding `index` pass embedded one node per HTTP request; it now
  batches up to 64 nodes per `/v1/embeddings` call (OpenAI-compatible array
  `input`), turning a full index from hundreds of sequential round trips into a
  handful. And the optional local-model calls (LLM consolidation + embeddings) now
  use a dedicated network policy — a generous timeout (`MINDLEAK_MODEL_TIMEOUT_MS`,
  default 120s) and **no retry**: a slow-but-working generation was classified as a
  transient failure and re-sent up to three times, tripling the wait before failing
  with nothing produced. Re-running `index` / `consolidate` is the retry. The
  deterministic zero-token write/query path is untouched.

### Changed
- **Lodestar's Intent Plane is shared across git worktrees by default.** With no
  `LODESTAR_DB` override, `lodestar-mcp` now resolves the DB to the git *common*
  dir's parent (`git rev-parse --git-common-dir`) — the main repo root — so every
  worktree of a repo opens the same `<repo-root>/.lodestar/spec.db` and coordinates
  through one plane (ADR-0018), instead of each worktree silently getting its own
  `<cwd>/.lodestar/spec.db`. `LODESTAR_DB` still overrides; outside a git repo it
  falls back to the current directory. The resolver is a pure, unit-tested function.
- **Conformance completion now consumes one authoritative checked verdict
  (ADR-0025).** `check_conformance` persists and returns
  `{ id, token, verdict, findings }`; `complete_task` requires that exact object,
  verifies unchanged evidence and relevant goal-binding/knowledge state, and
  transitions without invoking the optional semantic judge again. Identical
  evidence can no longer preflight `aligned` and complete as `needs_human`, and
  completion no longer writes a duplicate audit row.
- **MCP initialize metadata now identifies the exact source build.** MindLeak and
  Lodestar report `serverInfo.version` as
  `<package-version>+<12-character-git-sha>`, so clients can compare it with
  `git rev-parse --short=12 HEAD` and immediately spot a stale running server.
  A shared, dependency-free Cargo build helper resolves the SHA portably and
  supports `MINDLEAK_BUILD_SHA` for builds outside a Git checkout.
- **Lodestar tests are structurally isolated from any ambient local model.** A
  reusable `LlmClient::unreachable()` seam points the optional planning/judging
  model at an unroutable endpoint, so `decompose` and `judge` take their
  deterministic fallback regardless of whatever server a developer is running.
  The core test helper now uses it, and the previously untested `decompose_goal`
  MCP dispatch gains coverage that asserts the single-task fallback offline
  (closes the "Lodestar core tests are not isolated from a running local model"
  known gap).
- **`board` can hide terminal tasks.** The tool and facade gain
  `include_terminal` (default `true`, unchanged behaviour); `false` returns only
  the live/actionable set (open, claimed, in_review, blocked), so completed and
  abandoned work stays durable but drops out of a lean coordination view. Pairs
  with `abandon_task` to keep the board uncluttered without decaying intent
  (ADR-0004: the Intent Plane never expires tasks).
- **Git hooks are scoped and isolation-aware to stop concurrent-agent poisoning.**
  The cargo fmt/clippy/test pre-commit and pre-push hooks now run only for the
  crate packages a change touches, and — on push, or when a foreign untracked
  file sits in an affected crate — validate against a throwaway worktree snapshot
  rather than the shared dirty tree. An unrelated agent's broken crate or
  uncommitted WIP can no longer fail your commit or push (portable runner
  `scripts/cargo-precommit.mjs`; ADR-0018).
- **Two helper scripts for safe concurrent git in a shared tree (ADR-0018).**
  `scripts/scoped-commit.mjs` stages and commits only the paths you declare
  (pathspec; never `git add -A`), so another agent's staged work is never swept
  into your commit; `scripts/isolated-push.mjs` pushes a commit through the hooks
  from a throwaway worktree so another agent's broken WIP cannot poison your
  pre-push validation. A collision harness (`scripts/collision-harness.mjs`,
  `make collision-harness`) proves the no-clobber, independent-commit, and
  honest-merge-conflict properties in a throwaway sandbox repo.

### Added
- **Constitutional governance now has a holistic adoption design (ADR-0026).**
  The Constitution is the policy authority; tests, scanners, and ratchets are
  evidence-producing controls beneath it. The proposed lifecycle handles repos
  with no constitution through deterministic discovery, an opt-in five-principle
  Common Core, versioned extension packs, clause-by-clause adopt/tailor/reject
  review, atomic activation, and explicit expiring waivers. Philosophy is part of
  the architecture: observed habits may propose policy but never become law
  without attributed project adoption.
- **`unlink_goal_from_code` + `governing_goals` keep the ADR-0009 seam honest.**
  `link_goal_to_code` had no inverse, so a goal↔code binding — including one
  mistakenly attached to a shared doc — was permanent. Over a long multi-agent
  session that accumulated cross-goal bindings, and because `evaluate_conformance`
  flags any changed node governed by a *non-task* goal as drift, honest commits
  serving one goal started drifting against goals they do not realise. The new
  `unlink_goal_from_code(goal_id, node_ids)` prunes a stale binding (idempotent; a
  node not bound to the goal is a no-op; unknown goal is a typed `NotFound`), and
  `governing_goals(node_id)` audits which active goals govern a node and how,
  before pruning. Facade + MCP verbs + integration test that reproduces the
  cross-goal drift and shows the same evidence realign to `aligned` after the
  stale binding is removed.
- **Task lifecycle gains `needs_input` and `paused` states (ADR-0020).** Two live
  states reachable only from `claimed` by the owner, both clearing the live lease
  while keeping the owner and `claim_started_at` evidence window — deliberate
  parking, not release or abandonment. `ask_question` parks a task with a durable,
  append-only question for a human; `answer` records the reply and resumes the task
  under the same owner with a fresh lease. `pause_task` / `resume_task` suspend and
  resume owner-held work. A bounded **parking grace** records `parked_at` so a
  vanished owner cannot strand a parked task — after the grace it returns to the
  pool (`claim_task` / `next_task` reclaim it). Abandoning (terminal, non-`done`) a
  predecessor now **cascades**: its blocked successor transactionally reopens, so a
  dead predecessor never deadlocks a handoff chain, while a merely `in_review`
  predecessor keeps the successor correctly gated. New `task_qa` reads the thread.
  Exhaustive `match` on the extended enum; owner-guarded transitions; regression
  tested.
- **The learned-knowledge loop is wired end to end (ADR-0022).** Two seams that
  were dormant are now connected. A `promote_signals` bridge (facade + MCP verb)
  batch-feeds proven-signal candidates — opaque MindLeak node ids plus their
  provenance span — into the existing count+span consolidation gate; it invents no
  new threshold and builds a deterministic templated statement when no local model
  is available, so promotion never depends on an LLM. Conformance now consults
  `active_knowledge` on every check: when a task's changed nodes intersect a proven
  regularity's referenced nodes it attaches an **advisory** finding and may nudge
  an otherwise-`Aligned` verdict to `NeedsHuman` — but is structurally incapable of
  emitting `Violation` (only the Constitution hard-fails), keeping a stale or wrong
  regularity from blocking valid work. Knowledge stays durable-but-revalidated:
  unreconfirmed statements decay out of `active_knowledge` and are pruned.
- **`abandon_task` retires a task to terminal `abandoned`.** `TaskStatus::Abandoned`
  was defined but unreachable — a mis-filed or superseded task could not be retired
  short of `reset_database`. The new store/facade method and MCP tool move a
  nonterminal task (open, in_review, or blocked) to terminal `abandoned`, clearing
  any owner and dependency, while refusing to disturb an active claim (release
  first) or re-retire terminal work. Distinct from `reopen_task` (recover) and
  `reset_database` (wipe). Regression-tested.
- **Inspect a task's conformance evidence from the Intent Board.** Done and
  in-review tasks gain an "Inspect Task Evidence" action that opens the recorded
  evidence — verdict, findings, summary, and the changed/failed node and
  execution/commit ids — as a readable markdown view, resolved read-only from the
  existing `conformance_history` audit (no recomputation, no state change). The
  MindLeak activity-bar icon is now the brain mascot.
- **`conformance_history` resolves a task's durable evidence link.** Completing a
  task records its evidence bundle, verdict, and findings in the append-only
  conformance audit; the new facade method and MCP tool return that chain (each
  record carries a stable `id`, the recorded evidence, `verdict`, `findings`, and
  `checked_at`) so the proof a task is complete is resolvable after the fact
  without duplicating the evidence blob.
- **Telemetry pane in the VS Code extension.** A new sidebar view surfaces a
  real-time effectiveness readout — graph size, tool-call success/error rates,
  average latency, and per-tool metrics — refreshed on an interval
  (`mindleak.telemetryRefreshSecs`, default 3s) while visible. Full live event
  logging is opt-in via a **Live log** toggle (off by default). Numbers are
  derived from the existing `graph_stats` and `telemetry_snapshot` tools; no new
  server surface or hot-path cost.
- **`reopen_task` recovers stranded Lodestar tasks.** A task that landed in
  `in_review` (a drift or needs-human completion outcome) or was manually blocked
  with no predecessor previously had no path back to a claimable state. The new
  facade method and MCP tool return such a task to `open`, while refusing to
  bypass a handoff dependency, disturb an active claim, or revive terminal work.

### Changed
- **Consolidation classifies edge relations instead of always `refactored`.** The
  sleep-phase consolidation prompt now constrains the local model to a closed
  relation vocabulary — `fixed`, `relates_to`, `refactored` — and a new
  `RelationType::Fixed` variant is added. The deterministic layer is authoritative:
  any omitted, unknown, or structural relation the model returns is coerced to
  `refactored`, so fix/bug work and `DECISION:`/`WHY:` rationale links are no
  longer mislabelled as `refactored`.

### Fixed
- **`lodestar-mcp` no longer advertises a duplicated `consolidate` tool.** The
  ADR-0022 knowledge-loop change copy-pasted the `consolidate` definition, so
  `tools/list` returned two identical entries and MCP clients saw an ambiguous
  duplicated verb. The duplicate is removed and a `tools_list` regression test
  now asserts every advertised tool name is unique.
- **Injected embedding backends remain safe for maintenance worker threads.**
  `TextEmbedder` now requires `Send + Sync`, restoring the workspace build after
  the injectable semantic-recall seam made `MindLeak` non-`Send`. Compile-time
  and unit regression assertions preserve the worker-thread contract.
- **`record_knowledge` now honours a revised half-life.** Re-recording an
  existing statement previously updated weight, evidence, and the revalidation
  clock but silently kept the original `half_life_hours`, so a caller's changed
  revalidation cadence was lost. The `ON CONFLICT` clause now updates it, with a
  regression test.
- **Lodestar goal slugs no longer emit a trailing dash.** `slugify` trimmed
  separators before applying the 48-character cap, so a title whose boundary
  landed on a dash produced a goal id ending in `-`. Truncation now runs before
  trimming, with a regression test.
- **Duplicate `define_goal` returns a typed error instead of a raw SQLite fault.**
  Defining the same title and statement a third time collides on the derived
  `goal:{slug}-{hash(statement)}` id; it previously surfaced an opaque
  `UNIQUE constraint failed` error. `store::define_goal` now pre-checks the
  derived id and returns `LodestarError::Invalid`, pointing the author at
  `supersede_goal`, with a fail-pre/pass-post regression test.

## [0.1.0-preview.1] - 2026-07-23

### Added
- **Progressive task handoffs** (ADR-0015): `create_task(blocked_by=...)`
  creates an unclaimable successor that opens transactionally only after aligned
  predecessor completion. A deterministic two-connection benchmark demonstrates
  maximum same-file ownership of one versus two concurrent owners for
  independent tasks; advisory symbol leases remain intentionally unshipped.
- **Bounded working-memory tier** (ADR-0017 phase 1): `working_set` returns the
  configured agent's highest active observations, hard-capped at a startup
  `MINDLEAK_WORKING_SET_SIZE` (default 7, bounded 1-32). Sustained observations
  contribute deterministic rehearsal evidence without storing a separate buffer
  or invoking a model.
- **Opt-in autonomous consolidation** (ADR-0017 phase 2): an idle/rate-limited
  worker uses its own file-backed SQLite connection and the existing
  `consolidate_signal` path. A persisted workspace lease prevents duplicate
  manual/idle model spend across processes. Bounded post-model gist/provenance
  writes and unchanged raw candidate acknowledgement commit atomically without
  retaining raw inputs; attempts emit categorized maintenance telemetry and
  shutdown is bounded.
- **Per-project decay policy** (ADR-0014): strict committable
  `.mindleak.toml`, optional `MINDLEAK_CONFIG`, per-relation environment
  overrides, and bounded prune-threshold tuning. `GraphStore` applies the
  startup-resolved policy retroactively at read/prune time without rewriting
  stored edges or effective weights.
- **Productized distribution** (ADR-0016): one-command, JSONC-preserving
  two-plane workspace installer; self-contained platform-targeted VSIX packages;
  versioned native bundles for Windows x64, Linux x64, macOS Intel, and macOS
  Apple Silicon; SHA-256 checksums and signed GitHub provenance attestations;
  and a pinned VS Code 1.93.1 live Extension Host CI smoke.
- **VS Code lifecycle controls and complete health**: complete active-graph
  export, two-plane online backup, modal memory-only reset, and visible memory,
  intent, terminal, and Git health/degraded status.
- **Local data lifecycle** (ADR-0013): shared integrity-checked SQLite online
  backup for both planes; complete active graph JSON export; separately
  confirmed memory (`RESET MINDLEAK`) and durable intent (`RESET LODESTAR`)
  resets; and documented upgrade, rollback, retention, and privacy procedures.
- **Core engine** (`mindleak-core`): SQLite graph + FTS5, exponential half-life
  decay engine, and a registered `effective_weight()` scalar SQL function.
- **Zero-token deterministic ingestion**: `execution` (stack-trace `failed_on`
  parsing), `git` commits (with `DECISION:`/`HACK:` rationale extraction), and
  heuristic `ast` extraction of symbols **and in-file `calls` edges** for 8
  languages.
- **ADR-0006 structural imports, phase 1**: static JavaScript/TypeScript
  `import`/`require` declarations create artifact/package `imports` edges;
  direct calls to named import bindings create cross-file `calls` edges. Both
  participate in artifact-owned reconciliation and relation-directed impact.
  Token-aware extraction filters comments/member calls/basic shadowing, while
  candidate-backed artifact stubs promote across mixed extensions and index
  modules or disappear when their final import is removed.
- **ADR-0006 type hierarchy, phase 2**: simple named JavaScript/TypeScript class
  and interface heritage creates durable `extends`/`implements` edges for local
  and named imported types. Hierarchy participates in incoming impact traversal,
  consumer-first stub promotion, and artifact-owned retraction; the strict truth
  set measures 100% relation and impacted-type precision/recall.
- **ADR-0006 manifest dependencies, phase 3**: direct dependencies in
  `Cargo.toml`, `package.json`, `go.mod`, and `requirements*.txt` create durable
  artifact-owned `depends_on` edges to package nodes. Structured TOML, JSON, and
  PEP 508 parsers preserve renamed/canonical identities; malformed manifests
  fail before reconciliation, preserving the last valid snapshot.
- **MCP server** (`mindleak-mcp`): newline-delimited JSON-RPC 2.0 over stdio
  exposing 21 tools (`graph_multi_hop_query`, `get_impact_radius`,
  `record_architectural_decision`, plus ingestion/snapshot/prune/stats, an
  optional `consolidate_session` helper, `list_agents`, and the optional
  semantic-recall pair `recall`/`index`).
- **Observability, telemetry & network resilience** (ADR-0010): structured
  `tracing` to **stderr** (never stdout, which carries the JSON-RPC channel),
  gated by `MINDLEAK_LOG` / `MINDLEAK_LOG_FORMAT`; a durable, queryable
  `telemetry_events` audit trail recording every tool call (name, outcome,
  latency) surfaced through the `telemetry_snapshot` MCP tool; and a `net` layer
  giving all optional HTTP (embeddings, consolidation, LLM) explicit timeouts,
  bounded retry with backoff, and a per-endpoint **circuit breaker**. Tunable via
  `MINDLEAK_HTTP_TIMEOUT_MS` / `MINDLEAK_HTTP_RETRIES` /
  `MINDLEAK_BREAKER_THRESHOLD` / `MINDLEAK_BREAKER_COOLDOWN_MS`. The deterministic
  path never touches the network; telemetry never touches stdout or graph state.
- **Multi-agent attribution**: set `MINDLEAK_AGENT=<id>` and each ingest/focus
  also records a decay-weighted `agent:<id> --observed--> <node>` edge — shared
  graph, per-agent attention that fades. Roster via `list_agents`.
- **VS Code passive evidence sensors** (ADR-0011): focus boosts a node, save
  ingests structure, shell-integrated terminal start/end events ingest command
  outcomes and workspace mutation evidence, and built-in Git commit events
  ingest commit metadata and changed paths. Output retention is opt-in,
  redacted, and bounded; capture health reports concrete degraded modes.
- **Offline Cytoscape graph visualizer** (vendored, no CDN) with prune/export
  controls.
- **VS Code Intent Board**: a tree view of the Lodestar task board (who owns
  what) plus save-triggered conformance diagnostics (drift/violation surfaced
  inline) via a second `lodestar-mcp` client. Config: `mindleak.lodestarServerPath`,
  `mindleak.lodestarDatabasePath`, `mindleak.conformanceOnSave`.
- **Optional local-LLM consolidation** over the **OpenAI-compatible**
  `/v1/chat/completions` API (Ollama `/v1`, LM Studio, llama.cpp, …), configured
  via `MINDLEAK_LLM_URL` / `MINDLEAK_MODEL` / `MINDLEAK_LLM_API_KEY`; async and
  off the hot path. Both LLM clients (MindLeak + Lodestar) extract the JSON object
  from model output robustly (fence/prose-tolerant), verified end to end against
  `glm4:9b` by `#[ignore]`d live round-trip tests.
- **Optional semantic-recall embedding index** (ADR-0008): an off-hot-path
  vector *lens onto the graph*, complementing decay traversal rather than
  replacing it (ADR-0002). `index` embeds nodes lacking a current vector through
  a local **OpenAI-compatible** `/v1/embeddings` server (Ollama, LM Studio,
  llama.cpp, …), and `recall` returns the nearest node ids by cosine similarity —
  entry points to *seed* `graph_multi_hop_query`, not a substitute for it.
  Embeddings live in a derived, recall-only `embeddings` table and never touch
  the zero-token write path. Configured via `MINDLEAK_EMBED_URL` /
  `MINDLEAK_EMBED_MODEL` / `MINDLEAK_EMBED_API_KEY`; errors cleanly when no
  embedding server is reachable.
- Engineering baseline: pre-commit hooks, rustfmt/clippy/eslint/prettier,
  GitHub Actions CI (Linux + Windows), `.gitattributes`, and the `docs/`
  documentation set.
- **Test coverage pipeline**: CI runs workspace-wide Rust tests under
  `cargo-llvm-cov`, enforces 80% Rust line coverage plus 80% line and branch
  coverage on the extension's unit-testable `util.ts` surface, and uploads both
  LCOV reports for every push and pull request.
- **Tag-driven binary releases**: GitHub Actions gates tags through the full
  repository CI, builds and smoke-checks both MCP servers for Windows x64,
  Linux x64, macOS Intel, and macOS Apple Silicon, then publishes attested
  platform archives with `SHA256SUMS`.
- **Repeatable graph evaluation harness**: a cross-platform MCP/stdio scenario
  records stale-structure and cross-file-impact behavior against a fresh
  temporary database, with machine-readable baseline results, source revision,
  and executable hash. It clears ambient agent attribution and requires a typed
  structural edge before impact can pass.
- **Pinned real-agent outcome gate**: GitHub Copilot CLI 1.0.63 with
  `claude-haiku-4.5` runs no-memory, flat-history, MindLeak, and
  MindLeak+Lodestar arms in randomized fresh workspaces/databases and isolated
  Copilot homes. Across three runs per arm, MindLeak reduced median exploration
  18.2% and reached 66.7% success; MindLeak+Lodestar reached 100% success with
  zero regressions versus 0% for both controls.
- **Lodestar Intent Plane** (`lodestar-core` + `lodestar-mcp`): the durable "spec
  brain" (ADR-0004) — a versioned constitution (goals/constraints/invariants), an
  executive task ledger with an **atomic claim/lease compare-and-swap** for
  collision-free coordination of parallel local agents across worktrees, a
  conformance check (aligned/drift/violation), and **consolidated learned
  knowledge** that is durable-but-revalidated (ADR-0005). A second stdio MCP
  server with 23 tools; optional local SLM for decomposition and semantic
  conformance with deterministic fallbacks; shared `.lodestar/spec.db` (WAL) with
  the constitution exportable to committed markdown.

- **Derived signal-weighted decay** (ADR-0005/0012): every graph read derives a
  bounded half-life multiplier from span-qualified reinforcement, independent
  source diversity, failure/change/success consequence, surprise, structural
  in-degree, and explicit decisions. Effective weight remains derived and the
  multiplier is capped at 8x. `prune_graph` returns near-expiry proven signal
  with provenance and retains expired candidates until optional
  `consolidate_signal` succeeds, then acknowledges the raw evidence.

### Fixed
- Execution ingestion now batches one execution and all artifact edges in a
  single SQLite transaction. The 200-file/8 KiB passive-sensor benchmark moved
  from 296 ms to 28.651 ms p95, below the 50 ms gate.
- The committed dependency graph and source now compile with the declared Rust
  1.75 minimum: `Cargo.lock` uses format 3, parser/TLS transitives are pinned to
  compatible releases, and post-1.75 `Option` helpers use equivalent stable
  expressions.
- The exported `.lodestar/CONSTITUTION.md` is now committable while local
  Lodestar database and lease state remain ignored.
- Extension compiler and VS Code API typings are pinned to supported versions,
  preventing installs from silently advancing beyond the declared toolchain.
- Re-ingesting a source file now atomically replaces its artifact-owned
  structural snapshot, retracting removed symbols and call edges immediately.
- Focusing an entity now updates node attention without reviving the weight or
  decay clock of unrelated failures, modifications, and structural evidence.
- Impact analysis excludes agent observation edges, orphaned removed symbols are
  pruned after historical evidence expires, structural ownership conflicts fail
  atomically, and legacy migrations serialize concurrent openers.

[Unreleased]: https://github.com/monk-eee/MindLeak/compare/v0.1.3...HEAD
[0.1.3]: https://github.com/monk-eee/MindLeak/compare/v0.1.2...v0.1.3
[0.1.2]: https://github.com/monk-eee/MindLeak/compare/v0.1.1...v0.1.2
[0.1.1]: https://github.com/monk-eee/MindLeak/compare/v0.1.0...v0.1.1
[0.1.0]: https://github.com/monk-eee/MindLeak/compare/v0.1.0-preview.1...v0.1.0
[0.1.0-preview.1]: https://github.com/monk-eee/MindLeak/releases/tag/v0.1.0-preview.1
