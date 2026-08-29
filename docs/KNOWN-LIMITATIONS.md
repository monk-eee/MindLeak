# Known limitations

Things this project does not do, will not do, or cannot do — each with the
evidence that established it.

**This is not a defect list.** Open defects live in [`gaps.d/`](../gaps.d/), one
fragment per gap, and each one names something in this repository's own code
that could be fixed. Everything here is different: either a deliberate design
position that is working as intended, or a defect in a tool this repository does
not own. Neither has a fix waiting to be written, so neither belongs in a
catalogue of unfinished work.

The distinction matters because it decides what a reader should do. A `gaps.d/`
fragment is an invitation to fix something. An entry here is an invitation to
*stop* — to recognise a behaviour you were about to file as a bug, or to avoid
building on a guarantee that is not there. Mixing them made the gap catalogue
read as 36 outstanding jobs when roughly half of them had no job in them at all,
which devalues the half that do.

## How to read both catalogues

Neither file is authoritative about the code. The code is.

- **A gap fragment can say OPEN long after its defect is fixed.** A fragment is
  a durable note, not a live query — nothing re-evaluates it, so it goes stale
  silently and the staleness is invisible from the text. Verify against the tree
  before you act on one, and close it by deleting it in the commit that fixes it.
  This was measured, repeatedly, and no mechanical check can catch it; the full
  evidence is the first entry under *Design positions* below, including the two
  greps that were tried and failed, so they are not rebuilt.
- **An entry here can be overtaken by a decision.** A design position holds until
  an ADR changes it. If the ADR that established one is superseded, this entry is
  history, not policy.
- **Silence is not an all-clear.** Neither catalogue is complete. A behaviour
  described in neither file has not been ruled out; it has not been looked at.

## Where a new observation goes

Ask one question: **is there something in this repository I could change to make
this stop being true?**

- **Yes** → [`gaps.d/<slug>.md`](../gaps.d/). Even if you are not fixing it now.
- **No, because this is how it is designed to work** → here, under *Design
  positions*. Say what the alternative would cost, so the position can be
  revisited rather than merely rediscovered.
- **No, because the defect is in someone else's tool** → here, under *External
  tools*. Record the version you measured, because that is the only thing that
  makes the entry falsifiable later.

If a limitation later becomes fixable — a dependency ships the fix, or a design
position is reversed by an ADR — move it to `gaps.d/` and let it be a defect.
Movement in both directions is expected.

---

## Design positions

Behaviours that are working as intended. Each one is a trade that was made
deliberately; the entry records what the alternative would have cost.

- **A gap fragment can declare itself OPEN long after its defect is fixed, and
  no check can catch it — MEASURED 2026-07-30, OPEN by construction.**
  `scripts/gaps.mjs --check` refuses a fragment whose heading carries a terminal
  marker with no OPEN residual, which closes the direction where a fixed gap
  advertises itself as fixed and lingers. The opposite direction is the one that
  misleads, and it is invisible to the validator: a fragment that still says
  OPEN, VERIFIED or MEASURED while the thing it describes has been repaired. The
  validator reads a heading's self-declared status; whether that status is
  *true* lives in the code the fragment describes, so no rule over headings can
  decide it.

  Measured against `origin/main` on 2026-07-30, auditing the fragments that make
  a falsifiable claim about a named symbol, file or setting. Four of them were
  contradicted by the tree:
  `a-task-does-not-record-the-branch-it.md` ("`Task` has no branch field" —
  `model/executive.rs` declares `pub branch: Option<String>`, and a live claim
  reads it back);
  `a-renamed-adr-leaves-an-unreachable-design-board.md` ("there is no
  `retire_design`" — `facade/design.rs` has one under ADR-0042, and the ledger
  now reports zero rows whose `adr_path` is missing);
  `the-post-commit-ingest-hook-is-not-installed-so-commits-land.md` (the shared
  hooks directory now holds `post-commit`);
  and `six-adrs-are-absent-from-the-design-ledger-and-the-design-bo.md` (all six
  are registered; the ledger holds 72 rows). The first two were removed, the
  last two narrowed onto the residual that survived.

  The cost is not tidiness. An agent with an empty board reads this catalog to
  find work. On 2026-07-30 that agent picked a fragment, created a task, claimed
  it, and only then discovered the work had already shipped — one claim and one
  task creation spent on a fixed defect. Sampling was not unlucky: the first two
  fragments opened were both false.

  Not every fragment is stale, and the audit proved that too rather than
  assuming it: `squash-merging-is-still-enabled-at-the-button.md` was checked the
  same way and holds — the repository still reports `allow_squash_merge: true`.

  **Second pass, 2026-07-31, and the rate has not changed.** Re-audited against
  the ledger. `the-design-ledger-could-not-say-superseded-fixed.md` claimed
  ADR-0032's supersession "cannot be" recorded for want of an attributed
  decider; `design_items` shows `decided_by=monk-eee` and
  `superseded_by=design:0038-…`, and ADR-0018 → ADR-0032 likewise — so it was
  removed. It had also carried a cross-reference to "the parser gap below",
  which resolved to nothing: fragments became one-file-each under ADR-0056, and
  "below" had no referent left. A stale fragment rots in its links as well as
  its claims. `accepted-design-rows-carry-no-decider.md` was corrected rather
  than removed, because there the *count* moved while the *finding* held: 6 of
  72 became 3 of 76, all four ADRs it named by number are now decided, and
  ADR-0074 arrived undecided on the very day the gap was written. That one is
  worth stating as a rule — read the intake, not the count, because a backlog
  being worked and refilled is indistinguishable from one nobody has touched.

  **Scheduling the re-verification by date is closed too, and it was measured
  rather than argued.** The obvious next move after three manual passes is to
  stop remembering and start scheduling: report which fragments are overdue for
  a re-check. Three measurements on 2026-07-31 close that avenue.
  *An age report would read all-fresh:* only **18 of 49** fragments carry a
  parseable date, and every one of those is 0–3 days old, so the report would
  say nothing is due while the **31** with no date at all — the ones whose
  freshness is genuinely unknowable — are invisible to it by construction. That
  is a false green of the same family this catalogue keeps recording.
  *The door-rule cannot be adopted today:* requiring a date on any fragment that
  claims to be measured is the right shape, but **13** current fragments claim
  measurement without one, and a check that fails on day one is a check people
  switch off.
  *And the dates cannot be backfilled honestly:* **9 of those 13** were added by
  one commit, the ADR-0056 migration that moved gaps out of `DEVELOPERS.md`, so
  git knows when the file appeared and not when the claim was measured. Writing
  that date in would look like provenance while being an artefact of a file move.
  What remains is a maintainer's call, and a cheap one: date new measured
  fragments from now on, and let the undated 31 be re-dated only by whoever
  re-verifies them. Nothing else recovers the information, because the only
  person who knows when a measurement was taken is the one who took it.

  **A near-duplicate pair survived three audits, including one of mine.**
  `the-conformance-chain-governs-8-code-nodes-none.md` and
  `the-engine-was-ungoverned-and-the-gate-that-would-enforce-it.md` recorded the
  same gap, and on 2026-07-31 I re-measured one and corrected it without noticing
  the other already carried the correction, taken hours earlier. Duplicate
  entries do not just waste the reader's time: they make the catalogue look
  staler than it is, because a fix recorded in one copy leaves the other
  standing. The pre-flight that would have caught it is the near-miss check
  AGENTS.md already asks for — search the catalogue for a fragment covering the
  same gap before correcting one, not just the open pull requests for a file
  collision.

  **Two mechanical checks were tried before concluding none can work, and the
  measurement is recorded so it is not rebuilt.** Both were greps run over
  every fragment: the first flagged any fragment naming a live symbol near a
  negation word (39 of them), the second required the negation to sit adjacent
  to the symbol (11). Reading the 11 showed essentially all were false
  positives, because the negation is almost always about *behaviour* rather than
  existence — "`renew_lease` refuses outright", "`recall` cannot" — and those
  fragments are correct. The four failures that motivated this entry were of the
  form "there is no `retire_design`", which needs the checker to understand
  negation over a claim, not to grep for a name. This is the same conclusion the
  entry reached from the other direction, now with a measurement behind it: the
  truth of a fragment lives in the code it describes, and nothing over its text
  can decide it. Recorded so the next agent does not spend the afternoon
  rebuilding the same two greps.

  (The opening clause of that paragraph was missing — it began mid-sentence at
  "every fragment:" — and was restored on 2026-08-29 from what the rest of the
  paragraph states. No claim in it was changed.)

  No fix is proposed, because the obvious one is wrong. A status vocabulary rule
  would only ever check that a fragment agrees with itself. What the class needs
  is periodic re-verification against the tree — cheap for fragments naming a
  symbol or a setting, and not mechanisable for the rest — so it is recorded
  here as a standing maintenance cost rather than a defect awaiting a patch.

- **`done` does not mean `aligned`, but shipped work no longer remains silently
  claimable — NARROWED, MEASURED 2026-08-01.** The earlier fragment combined
  two different failures: work already merged still appearing open/free, and a
  completed task carrying a non-aligned receipt. The first is closed. Before
  this audit the live board held exactly two claimed tasks, both attached to
  current workstreams, with zero open, blocked, paused, needs-input, or
  in-review residue. Completion offers at publication, merge-derived evidence,
  `existing_work`, reviewed rescue, and explicit human resolution removed the
  ten known shipped-but-claimable examples recorded in the old measurement.

  The receipt distinction remains and should stay visible in reporting. Taking
  each of 330 `done` tasks' `resolved_conformance_id` when present and otherwise
  its latest conformance check yields 157 `aligned` (47.6%), 133 `needs_human`
  (40.3%), and 40 `drift` (12.1%). This improves on the prior 119 of 247
  `needs_human` result (48.2%), but it is not automated affirmation. A task can
  be `done` because a named person reviewed and resolved a non-aligned receipt;
  that is an auditable decision, not a claim that conformance aligned.

  Impact: delivery dashboards must report automated alignment separately from
  human resolution. Do not loosen evidence windows, erase lease lapses, or
  reinterpret a human resolution as `aligned` to improve the ratio. The
  remaining work is measurement and workflow quality: advise before task
  creation, claim before the first commit, keep the lease live, submit the
  publication offer promptly, and show both completion route and verdict in
  aggregate product metrics.

  NARROWED FURTHER, 2026-08-19: the aligned/needs_human/drift split above
  required a one-time manual audit query. `lodestar_stats` now reports it
  directly as `done_verdicts` (aligned/needs_human/drift/violation/unresolved),
  computed the same way -- human `resolved_conformance_id` when present,
  otherwise the task's latest conformance check, otherwise `unresolved`. Any
  caller can watch the ratio move without re-deriving the query each time. This
  closes the measurement gap, not the underlying fact: `done` still means
  shipped, not that conformance affirmed it, and a human can still resolve a
  non-aligned receipt for a good reason. Dashboards and agents should read
  `done_verdicts` rather than treating `done_tasks` as a proxy for correctness.

  RE-MEASURED 2026-08-26 via `lodestar_stats` directly (no manual audit
  needed, confirming the 2026-08-19 tooling fix still holds): of 803 `done`
  tasks, 485 `aligned` (60.4%), 240 `needs_human` (29.9%), 78 `drift` (9.7%) —
  up from 47.6% `aligned` on 2026-08-01 across roughly 2.4x as many done
  tasks. The ratio has moved in the direction this fragment asks dashboards to
  watch for, not away from it. This is not automated affirmation and does not
  change the underlying fact the fragment records: a human resolving a
  non-aligned receipt is still an auditable decision, never a claim that
  conformance aligned, and no code change accompanies this entry — it is a
  data point, recorded so the next reader does not have to re-run the query
  to know whether the trend is improving or eroding.

- **A superseded server binary survives its own replacement while the process is
  running, and keeps serving the defect.** `install-servers.mjs` renames the
  running binary to `<name>.exe.<timestamp>.old` and writes the new one in its
  place, which succeeds on Windows even with the file open — but the live
  process keeps executing the old image, so a fixed binary on disk changes
  nothing until every server process is restarted. Measured here: four servers
  started ~13 minutes before the fix went on disk continued to advertise the
  broken schema afterwards, and their `.old` files could not be deleted
  (`Access to the path ... is denied`) until those processes were stopped. The
  installer's closing advice to "restart the MCP servers" is therefore load
  bearing rather than housekeeping, and the leftover `.old` files are a reliable
  signal that a pre-fix process is still live. Fixed this run only in the sense
  that the processes were stopped and the residue collected.
- **NARROWED 2026-08-29: that signal is now reported instead of discarded, but
  the underlying survival is unchanged and OPEN.** `pruneSupersededInstalls`
  already learned exactly what this fragment describes every time it ran — a
  set-aside binary that will not delete is a process still holding it — and threw
  it away in a bare `catch`. It now returns `{ pruned, held }`, and both the
  install and `--prune` paths say that a held binary means those servers are
  still running the code the install replaced, so the change is not live until
  they restart. The unconditional "restart the MCP servers" advice is therefore
  no longer the only thing standing between a shipped fix and a process quietly
  serving the defect. Two limits worth stating plainly: this is evidence only on
  Windows, because Unix unlinks a running binary happily and leaves no residue,
  so a quiet result there means "no evidence", never "nothing is running"; and
  nothing here stops or restarts anything, so the operator still has to act. The
  root — a live process keeps executing the old image — is inherent to replacing
  a running binary and is not fixable in the installer.

- **`active_knowledge` decays purely on `half_life_hours`, never on whether the
  code it describes has actually changed.** — Observed 2026-08-17: knowledge
  entries lose reach on a timer regardless of whether the ADR or file they
  reference has since been amended, superseded, or deleted. Where:
  `crates/lodestar-core/src/facade/knowledge.rs` (`record_knowledge`/
  `revise_knowledge` apply `decay::KNOWLEDGE_DEFAULT_HALF_LIFE_HOURS` with no
  check against current repo state). Impact: a lesson about a file can keep
  reaching agents for its full half-life even after the described behaviour
  was fixed (a false positive advisory), or can expire while the file it
  describes is untouched and the lesson still fully applies (a false
  negative). Left for later: no code change made this run; the fix shape is
  to let a knowledge entry's `node` references be checked against current
  repo state (git blame / file hash since recorded), rather than trusting
  elapsed time alone.

  **NARROWED 2026-08-26 — corrected a false citation, substantive claim
  re-verified and unchanged.** This entry originally pointed at
  `scripts/check-evidence.mjs` as prior art already doing this kind of
  evidence-backed staleness check. That file has never existed anywhere in
  this repository's git history (`git log --all --diff-filter=A -- "*check-
  evidence*"` returns nothing, on any branch, ever) — the comparison was
  wrong from the start, not a tool that was later renamed or removed. Left
  uncorrected, the next reader would go looking for a pattern to reuse and
  find nothing where the fragment said something existed. Re-verified the
  decay path itself directly against `facade/knowledge.rs` rather than trust
  the fragment's own account: the substantive claim holds exactly as
  written — decay is still purely timer-based, with nothing checking a
  knowledge entry's referenced nodes against current repo state. The fix
  shape described above is unbuilt and remains open; only the citation was
  wrong.

- **The recall floor cannot rank, and raising it makes recall worse — MEASURED,
  do not "fix" it.** The obvious response to `recall` returning a plausible
  stranger is to raise `MINDLEAK_RECALL_FLOOR`. Measured against this
  repository's own index over six real questions, that is backwards. Recorded
  conclusions scored **0.553–0.790**; structural nodes matched on shared
  vocabulary scored **0.527–0.667**. The ranges *overlap*, so any threshold high
  enough to exclude the worst stranger (0.667) also excludes real conclusions
  (0.553). One query returned the right conclusion at 0.651 and an unrelated
  `merge_import` symbol at 0.626 — a 0.025 gap that no global constant can
  separate.
  What does work is rank: a conclusion was the **top hit in six of six**
  queries. So the floor's job is to answer "is there anything here at all",
  which it does (ADR-0053), and it is not a relevance knob. If recall's
  precision needs improving, the lever is the ranking or the embedding model,
  not the threshold. Reproduce with a `recall` sweep and compare the score
  ranges before changing the default.

  **The ranking lever has since been taken; the floor advice above stands
  unchanged.** ADR-0075 stopped recall ordering by raw cosine: similarity is now
  weighted by node kind, so a recorded conclusion outranks a symbol that merely
  shares a word, which is exactly the 0.651-versus-0.626 case above that no
  constant could separate. The same change added a per-query distinctiveness cut
  for the reason this fragment gives — an absolute number cannot judge a score
  whose baseline moves with the query. Neither touched
  `MINDLEAK_RECALL_FLOOR`, and its default is deliberately unchanged. This is
  recorded so the next reader does not re-derive the measurement or re-implement
  the fix; what remains open is the *other* lever, the embedding model, which
  nothing here has tested.

  **Measured afterwards, and the overlap repeated itself one level up.** On the
  live 19,317-node index the ranking change did what it claimed — hits naming a
  node the graph no longer holds fell from 24 of 50 to 0 of 49, and recorded
  conclusions rose from 14% of what the caller is handed to 96%. But the
  distinctiveness cut does **not** let recall reject a question it has no answer
  for: top-hit distance above the field is 3.11–3.90 standard deviations for
  nonsense controls against 3.71–6.21 for real questions, so those bands overlap
  by 0.19σ exactly as the score ranges above do. A threshold in σ is still a
  global constant; moving from cosine to σ changed the units, not the shape of
  the problem. So the warning in this fragment generalises further than it was
  written: **the lever for "is there an answer here at all" is not a constant in
  any unit**, and nothing has yet found one that works. Numbers and harness in
  [EVALUATION.md](../docs/EVALUATION.md).

- **Signal consequence remains a bounded temporal proxy.** — A failure earns
  consequence only when the same command later succeeds after a related change,
  but this still cannot prove causality. The 8x cap, provenance-bearing handoff,
  and eventual decay limit coincidence laundering. — Medium impact on salience
  precision. — Left explicit; stronger causal tracing needs process/test
  attribution rather than another heuristic.

- **Derived signal queries are benchmarked, not asymptotically free.** — Evidence
  is computed per edge from graph state; a 200-edge snapshot measured 16.757 ms
  p95, but much larger dense graphs may need batched SQL/materialized raw
  provenance. — Low current impact. — Left as a measured scaling boundary.

- **Symbol and import extraction remains heuristic and partially scoped.** —
  Static JS/TS named imports now produce cross-file `calls`, and literal dynamic
  imports produce structural dependency edges, but default and namespace calls,
  path aliases, computed or template-literal dynamic imports, and other
  language import syntaxes are not resolved. Re-exports (`export { a } from
  '...'`, `export * from '...'`, `export * as ns from '...'`) now resolve to a
  structural import edge exactly like an ordinary import. Type hierarchy
  supports simple named local and imported JS/TS heritage, not default/namespace
  targets or expression-based mixins. Non-JS brace/indent extractors also
  remain regex-based. — Medium impact on graph completeness. — Tracked: expand
  fixture-backed deterministic parsers; Tree-sitter remains the precision
  upgrade (ADR-0002).

- **Manifest dependency support is direct-only.** — `Cargo.toml`, `package.json`,
  `go.mod`, and named PEP 508 lines in `requirements*.txt` emit `depends_on`.
  Lockfiles, transitive dependencies, npm overrides, Cargo workspace catalogs,
  Go replacements, requirement includes/options, and unnamed VCS/local Python
  requirements do not. — Low impact on direct impact analysis; intentional to
  avoid turning catalogs and resolver output into false direct edges.

- **The live LLM round-trip runs only on demand, not in CI.** — Ignored tests
  (`cargo test -- --ignored`) exercise the real `/v1/chat/completions` call for
  both planes (MindLeak `consolidate`, Lodestar `decompose`/`judge`) against a
  running model; CI can't run them without one. — Low impact. — Running them
  surfaced (and fixed) that `glm4:9b` wraps its JSON in prose even with
  `response_format: json_object`; both clients now extract the JSON object
  robustly.

- **The real-agent product gate is narrow.** — Three runs per arm on one
  composite typed-session fixture with Copilot CLI 1.0.63 / Haiku 4.5 cross the
  exploration and success thresholds, but do not establish general performance
  across repositories, models, or long-running teams. The two-agent duplicate-
  work mechanism is now covered by ADR-0024's deterministic two-plane overlap
  benchmark, but independent agents' scope accuracy and willingness to heed an
  advisory are not. — Medium impact on claim breadth. — Productization may
  proceed; broader external replications remain required for universal efficacy
  claims.

---

## External tools

Defects and limits in tools this repository depends on but does not own. Nothing
here is fixable from this codebase. Each entry records the version measured,
because that is what lets a future reader check whether it still holds.

- **Passive execution evidence depends on VS Code shell integration.** — VS Code
  1.93 shell start/end events provide command/exit evidence; unsupported or
  conflicting shells report degraded capture and are not guessed from terminal
  text. Concurrent terminal executions can both observe one workspace mutation,
  so changed paths prove temporal overlap rather than process-level causality. —
  Medium impact on provenance precision in overlapping command sessions.

- **A new MCP tool is invisible until VS Code reloads, and refreshing the
  installed binary does not change what is running — NARROWED 2026-08-14, still
  OPEN.** Two of this fragment's claims are contradicted by the tree and are
  corrected here rather than left to mislead a reader into rebuilding work that
  exists. The residual is smaller than the heading above it used to claim, and
  it has moved.

  **No longer true: the running servers lock the binaries.** The original text
  said `cargo build --release` fails with `Access is denied (os error 5)`
  because the servers hold the files open. Measured 2026-08-14 with eight server
  processes live: `target/release/lodestar-mcp.exe`,
  `target/release/mindleak-mcp.exe` and `~/.mindleak/bin/lodestar-mcp.exe` each
  opened `r+` without complaint. Two things removed the lock — the fleet
  executes sha-suffixed copies (`lodestar-mcp-1551270.exe`) rather than the
  build output, and `scripts/install-servers.mjs` renames the destination aside
  before copying, for the reason its own comment gives: "Windows refuses to
  overwrite a live executable but does allow renaming one".

  **No longer true: there is no in-band signal.** `stale_build` is threaded from
  `main.rs` through `server.rs` into `tools/mod.rs` on both planes and returned
  from `open_session`, the one call every agent makes first, with
  `build_identity.rs` producing "running a stale build of this checkout: binary
  was built from `<sha>`, HEAD is `<sha>`". A second notice, `replaced_binary`,
  covers the swap that `stale_build` structurally cannot see.
  Both are tested. It fired during the session that wrote this: a binary built
  from `ecac179523e5` answering for a checkout at `ee94dc73`.

  **Still true, and why this fragment stays:** a tool added during a session
  cannot be exercised in that session. VS Code caches the advertised tool list,
  so a new verb is absent from `tools/list` until the window reloads, and
  reaching it by name still lands in a server process that predates it.

  **New, and the sharper half: installing a fresh build silently changes nothing
  that is running.** `install-servers.mjs` writes the *unsuffixed*
  `executableName(name)` into the shared install directory, while the live
  processes execute sha-suffixed copies sitting beside it. On 2026-08-14
  `~/.mindleak/bin/lodestar-mcp.exe` was almost four hours newer than the
  `lodestar-mcp-1551270.exe` every server was actually running. The install
  succeeds, reports success, and the fleet goes on serving the older build —
  which is precisely what `stale_build` then reports, one layer too late to have
  prevented it.

  **No longer true: nothing collects the copies left beside them.** This
  fragment reported roughly 70 MB of `.old` and `.superseded` binaries that
  nothing reclaimed; measured on the same day at eight copies and 68.2 MiB. The
  cause was narrower than "nothing collects it": `pruneSupersededInstalls`
  already existed, but it matched only `.old` — the name `installOne` writes —
  while a hand deploy renames the live file to `.superseded` for the same lock
  reason, and it ran only as a side effect of a full install, which a deploy
  that copies a build in by hand never performs. It now takes both suffixes and
  is reachable on its own as `node scripts/install-servers.mjs --prune`.

  Left open because the remaining fix is a decision, not a patch: the
  sha-suffixed copy is an operator workaround for a lock the installer already
  solves by renaming, so what needs settling is which name the registration
  should spell, and who is allowed to change it.

- **One editor window accumulates MCP server processes without bound.** Measured 2026-08-27 on a single machine: 27 live `mindleak-mcp`/`lodestar-mcp` processes across six parent clients, with one VS Code window alone holding **16 children**; the `mindleak-mcp` count grew from 5 to 14 within about two minutes of ordinary tool use. Each one opens the repository `graph.db`/`spec.db`, so this is not only memory — it multiplies WAL readers on a shared SQLite file. Impact: any operation needing an exclusive lock (notably the `VACUUM` that returns pages after a table rebuild) has to be given a deliberately cleared window, and a routine upgrade cannot rely on getting one; the reclaim is best-effort precisely because of this. Also makes the "restart the MCP servers" advice in `scripts/install-servers.mjs` harder than it sounds, since a fixed binary on disk changes nothing until every one of them is stopped (see `a-superseded-server-binary-survives-its-own-replacement.md`). Not diagnosed further: the spawning is client-side, so the cause may be VS Code/Copilot restarting servers rather than anything in this repository. Not fixed this run; recorded because it silently raises the cost of every future migration.
- **UPDATE (2026-08-27): a session's own connection can break outright and not recover, even across a window reload.** Distinct from the accumulation above (this is one client losing its one connection, not many clients holding many). Measured across roughly 24 hours: every `mindleak`/`lodestar` tool call returned `Transport closed`; the two server processes that connection was originally bound to had exited and were never replaced, while dozens of other sessions' server processes kept spinning up and down normally around them; a full VS Code window reload spawned fresh `mindleak-mcp`/`lodestar-mcp` processes (confirmed by new PIDs/start times) but did not restore the broken session's binding. Still not diagnosed further — client-side, as above. **Mitigated, not fixed:** `scripts/mcp-direct.mjs` drives one batch of tool calls directly against the built release binaries over the same newline-delimited JSON-RPC stdio `scripts/canonical-push.mjs` already speaks on every publish, independent of any editor's persistent connection. Used for real during the outage to run `open_session`/`check_overlap`/`advise`/`task_create`/`task_claim`/`task_transition` for two shipped fixes (PR #798 and its own follow-up) — the claim and evidence trail were genuine, not skipped. A batch must be one invocation (session state lives only in that one process's memory), which is why the script takes a list of calls rather than one at a time.
- **FIXED 2026-08-29 (recorded 2026-08-28) — a reconnect after the session's own worktree was reclaimed silently fell back to a fresh, empty, workspace-local database instead of erroring.** Fixed by `ensure_workspace_exists` in `crates/mindleak-storage/src/repository/resolve.rs`: both `resolve_database` and `resolve_database_in` now refuse a workspace path that is not an existing directory, returning the new `RepositoryStorageError::MissingWorkspace`, and they do it *before* Git discovery runs. An explicit database path still wins, and a directory that merely sits outside Git still falls back to workspace-local exactly as before. Worth recording that the silent-empty-database symptom was Unix-only: spawning `git rev-parse` into a deleted working directory reports `NotFound` there, which is the same `io::ErrorKind` as a missing `git` binary and so hit the designed "not inside Git" arm, while Windows reports `NotADirectory` and surfaced an opaque `Io(Os { code: 267 })` instead — one bug wearing two faces, neither of which said "your workspace is gone". Regression test: `a_workspace_that_no_longer_exists_is_refused_rather_than_silently_emptied`. The reclaim side of the story is also better than it was: `scripts/worktree-reclaim.mjs` now keeps any worktree that is dirty, unlanded, owned by another session, or held by a live Lodestar claim. Original diagnosis follows. A long-lived session (spanning over a day, multiple sub-tasks) had its own worktree (`MindLeak.worktrees/<slug>`, the directory that session's chat was rooted in) reclaimed by this repository's own `make reclaim`/queue-watch automation while the session was still alive inside it — its branch carried no unmerged commits of its own, so the reclaim was individually correct, but nothing warned the still-running session. The next MCP client reconnect (visible as a fresh `mindleak-mcp`/`lodestar-mcp` process pair) resolved storage by calling `git rev-parse --git-common-dir` against that now-`.git`-less directory (`mindleak-storage::repository::resolve::resolve_database`); the call fails, and the resolver's designed fallback for "not inside Git" (`DatabaseOrigin::Workspace`, a bare per-directory SQLite file with `repository_id: None`) fires exactly as coded — it does not distinguish "never was a Git checkout" from "was a valid worktree seconds ago and just lost its link." Symptom: `storage_status` returns `repository_id: null` and `origin: "workspace"` on both planes; `lodestar_stats` reports 0 goals/0 tasks/0 knowledge where it reported hundreds moments before; no error, no warning, just quietly different (and functionally empty) storage. Not fixed this run: the fix belongs in `resolve_database`'s Git-detection path (a git-common-dir lookup that fails on a directory that visibly still contains a repository's own tracked files, e.g. `AGENTS.md`/`Cargo.toml` at its root, is a strong signal the workspace argument itself is stale rather than genuinely non-Git, and could at minimum surface a distinct `DatabaseOrigin` or a loud warning instead of a silent, indistinguishable-from-legitimate fallback) or in the reclaim tooling (never reclaim a worktree a live session is still rooted in, if that is detectable). Recovered by starting a fresh session rooted at a newly created worktree; the dead one was left alone rather than repaired in place.

- **Unit Test MCP 1.3.6 cannot validate this workspace reliably — still true in
  the tool; no longer this repo's default path.** — Its Vitest
  discovery finds `src/util.test.ts`, but `run_tests` reports a passing total of
  zero even for that explicit path. On Windows, a backslash Cargo root is
  rejected as `INVALID_ROOT_DIR`; normalizing it to forward slashes runs the
  custom command and surfaces failures, but successful runs still report zero
  tests. Vitest coverage also depends on drive-letter casing: a lowercase `c:`
  root duplicates every covered source as an uppercase `C:` zero-hit shadow,
  falsely reporting 38.64% lines; the canonical uppercase root produces the
  correct unique-file aggregate (89.19% lines / 84.85% branches). — High impact
  on local proof. — Left open in the external adapter; use a canonical uppercase
  Windows drive root for coverage, while CI's test counts remain authoritative.

  **NARROWED 2026-08-26.** `AGENTS.md` no longer tells agents to prefer this
  tool for test runs — it names this fragment (and its three siblings)
  directly and routes to `cargo test`/`npm test` instead. This closes this
  repository's exposure to the zero-count and coverage-shadow defects above;
  the defects themselves remain unfixed, because 1.3.6 is a third-party
  extension version, not code in this repository.

- **The Unit Test MCP cargo adapter hides the assertion, so a red test cannot be
  diagnosed — OBSERVED, still true in the tool; this repo no longer routes
  agents to it by default.** `run_tests` with `framework=custom` returns
  `status: FAILED` with `passed/failed/total` all zero and a message containing
  only cargo's stderr (`error: test failed, to rerun pass -p <crate> --test
  <target>`). The failing test's name and its assertion output go to the
  harness's stdout, which the adapter drops, and `compact_output=false` does not
  bring them back. Impact: a genuine red is indistinguishable from a compile
  error, and there is no way to tell *which* test failed or why. This is what
  turned the mtime bug above into a long hunt instead of a one-line read.
  Workaround: have the test write its result to a file under `target/tmp/` and
  read that file, then delete the write before committing. Left for later — the
  adapter needs to surface harness stdout on failure.

  **`test_pattern` is also ignored, and its apparent effect is a trap —
  MEASURED 2026-07-29.** Passing `test_pattern` does not narrow a cargo run: the
  whole lib suite executes and aborts at the first failure. Proven by a control
  experiment while red/green-proving the amendment control tests — with a
  deliberate break in `amend_constitution`, a run naming
  `an_amendment_that_changes_nothing_is_refused`, a test that cannot touch that
  code, still returned `FAILED`.
  The trap is the timing. A filtered-looking run returns in 6–7 s against ~60 s
  for a green suite, which reads exactly like a filter working. It is not: that
  duration is *time to first failure*, so it shrinks as the suite gets redder.
  Anyone using run duration to infer that a filter took effect will conclude the
  named test failed when the failure was somewhere else entirely — this note
  exists because that inference was made and acted on earlier the same day.
  To attribute a failure to one test, mark the others `#[ignore]` and run the
  full suite; that does work, and it is how the three control tests were each
  proven red for their own reason.

  **NARROWED 2026-08-26.** The line above originally read "while the repo
  instructions correctly forbid running `cargo test` in a terminal" — that
  instruction is gone. `AGENTS.md`'s "Commands" section used to prefer the
  Unit Test MCP tool for test runs generally; it now names this fragment's
  hidden-assertion and ignored-`test_pattern` defects directly (alongside its
  three siblings) and tells agents to run `cargo test`/`cargo test --all` in a
  terminal instead, which surfaces the real assertion and honours a real
  filter. This closes this repository's exposure to both defects; neither is
  fixed in the adapter itself, which remains a third-party extension.

- **Unit Test MCP reports `PASSED` for `scripts/*.test.mjs`, which it never
  runs — OPEN in the tool itself; this repo's exposure to it is now removed.**
  The repository's guard tests are `node:test` files and no adapter covers
  them. Asked to run one with `framework=custom`, `run_tests` returned
  `status: PASSED` with `passed`/`failed`/`total` all zero. A red/green
  probe on 2026-07-29 proved the false green: an assertion that `1 === 2` inside
  `scripts/measure-tool-surface.test.mjs` still came back `PASSED`. — High
  impact: a suite that never executed is indistinguishable from a real green
  result. — Until an adapter exists, validate script tests with
  `make script-test` (`node scripts/script-tests.mjs`), which is what CI runs.

  **NARROWED 2026-08-26.** `AGENTS.md`'s own "Commands" section used to read
  "prefer the Unit Test MCP tools for test runs where available", actively
  directing every agent working here toward the tool this fragment (and its
  three siblings) measured giving false greens. That line is gone: the
  section now names the specific failure modes across all four fragments and
  tells agents to run `make`/`cargo`/`npm` directly instead, which is what CI
  trusts. This closes this repository's *dependency* on the broken tool, not
  the tool's own defect — a third-party MCP extension is not this repo's to
  fix, so the underlying bug stays OPEN and this fragment stays open with it.

- **Unit Test MCP with `framework=custom` run from `editors/vscode` silently
  runs Cargo, not Vitest, and reports PASSED — CONFIRMED, config footgun in the
  tool itself; this repo no longer routes agents to it.**
  Cargo walks up from `editors/vscode` and finds the workspace `Cargo.toml`, so
  the Rust suite runs and goes green while the extension tests never execute.
  Verified by breaking a `util.test.ts` assertion on purpose: `framework=custom`
  reported PASSED; `framework=vitest` with
  `root_dir=<repo>/editors/vscode` reported the real failure and the assertion
  diff. Any extension change validated through the custom adapter has a
  meaningless green behind it. Use `framework=vitest` for
  `editors/vscode`, and treat a suspiciously fast/slow duration as the tell.

  **NARROWED 2026-08-26.** `AGENTS.md` used to tell every agent to prefer the
  Unit Test MCP tool for test runs; it now names this footgun (alongside its
  three siblings) directly and points at `npm --prefix editors/vscode test`
  instead. That closes the exposure this repository controls; the adapter's
  own silent-substitution bug is unchanged and still OPEN, since it lives in
  a third-party extension.
