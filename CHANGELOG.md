# Changelog

All notable changes to MindLeak are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and this project adheres
to [Semantic Versioning](https://semver.org/).

## [Unreleased]

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
### Fixed
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

## [0.1.3] - 2026-07-27

### Added

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

### Fixed

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

### Documentation

- **ADR-0053: the graph records events, not conclusions (Proposed).** After an
  eight-hour session, `recall` was put to the four lessons that session had
  actually cost time to learn. All four returned noise from a graph of 4,463
  nodes and 9,572 active edges. Three causes, all confirmed in the code: the
  zero-token write path can only capture executions and symbols, never a
  sentence; `recall` is cosine similarity with **no floor**, so it always returns
  `limit` rows however unrelated — the nonsense query `zzzzqqq wibble flarp`
  scores 0.54, higher than any of the four real questions; and a recorded node is
  invisible until the offline `index_nodes` pass embeds it, demonstrated by
  `record_architectural_decision` writing a node whose own title then recalled
  `[]`. The ADR proposes a similarity floor that lets `recall` honestly return
  nothing, indexing on record, recording what was learned as part of finishing
  work, and a long half-life for conclusions — without touching the zero-token
  invariant. Proposed only, not accepted, nothing implemented in this build.

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
