# Developing MindLeak

From a clean machine to the engine building, tested, and the extension running.
If you get stuck, that is a defect — fix it or add it to [Known gaps](#known-gaps).

## Prerequisites

- **Rust** 1.75+ (via [rustup](https://rustup.rs)); MSVC toolchain on Windows.
- **cargo-llvm-cov** for local Rust coverage (`cargo install cargo-llvm-cov --locked`).
- **Node** 18+ and npm (for the VS Code extension).
- **Python** 3.8+ with `pip` (for the `pre-commit` framework).

## One-time setup

```bash
git clone https://github.com/monk-eee/MindLeak
cd MindLeak

# Rust components
rustup component add rustfmt clippy
cargo install cargo-llvm-cov --locked

# Pre-commit hooks (client-side enforcement)
pip install pre-commit
pre-commit install
pre-commit install --hook-type pre-push

# Extension dependencies
npm --prefix editors/vscode install
```

On systems with `make`, `make setup` does the hook + extension steps.

The cargo hooks are **scoped and committed-snapshot aware**
(`scripts/cargo-precommit.mjs`): they run `cargo fmt/clippy/test` only for the
crate packages your change touches and, when the live tree could leak another
agent's WIP, materialize the staged or committed tree through a temporary Git
index. No worktree, branch, commit, or shared ref is created by validation.

Each concurrent workstream uses its own linked worktree and task branch
(ADR-0038). Claim work with that concrete path scope, then run
`node scripts/scoped-commit.mjs -m "<msg>" -- <path>...` inside the worktree to
commit only declared paths (never `git add -A`). A clean worktree may publish
its own branch's exact `HEAD` with `node scripts/canonical-push.mjs`; the
publisher refuses protected branches, detached `HEAD`, any uncommitted state,
and remote divergence. Do not cherry-pick, rebase, or squash routine work.

`main` is a protected branch: it advances only through a pull request whose five
CI checks pass, and it refuses force pushes and deletion. Land a fleet branch by
opening a PR; `gh pr merge --auto` queues the merge until CI is green rather than
landing on an unverified tree. Admin bypass is currently still possible — see the
`enforce_admins` note in [ADR-0034](docs/adr/0034-typed-controls-and-enforcement-ceilings.md).

**Success looks like:** `cargo test --all` reports `test result: ok` for every
crate, and `target/debug/mindleak-mcp` starts and prints
`[mindleak-mcp] ready — graph at …` on stderr.

## Everyday commands

| Task | `make` | Direct command |
|---|---|---|
| Build | `make build` | `cargo build` |
| Test | `make test` | `cargo test --all` |
| Coverage | `make coverage` | Rust LCOV + scoped Vitest coverage; both enforce an 80% floor |
| Format | `make fmt` | `cargo fmt --all` |
| Format check | `make fmt-check` | `cargo fmt --all -- --check` |
| Lint (Rust) | `make clippy` | `cargo clippy --all-targets --all-features -- -D warnings` |
| Lint (extension) | `make ext-lint` | `npm --prefix editors/vscode run lint` |
| Test (extension) | `make ext-test` | `npm --prefix editors/vscode test` |
| Compile extension | `make ext-compile` | `npm --prefix editors/vscode run compile` |
| ADR safety | `make adr-guard` | `node scripts/adr-guard.mjs` — fails if any ADR is uncommitted or on no remote ref |
| Everything CI runs | `make ci` | see [`.github/workflows/ci.yml`](.github/workflows/ci.yml) |

> **`make` is optional.** Every target maps to the direct command in the
> right-hand column — `cargo`, `npm`, and `git` are identical on Linux, macOS,
> and Windows, so run those directly if `make` is unavailable.

## Local gate before a PR

Do your laundry locally — CI is the safety net, not the first line of defence:

```bash
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all
npm --prefix editors/vscode run lint
npm --prefix editors/vscode test
npm --prefix editors/vscode run compile
make coverage
```

## Publishing a binary release

The tag-driven [release workflow](.github/workflows/release.yml) publishes both
MCP servers for Windows x64, Linux x64, macOS Intel, and macOS Apple Silicon.
Each target gets a one-command installer archive and a VSIX containing both
native servers. The workflow reruns `make ci`, performs native MCP
initialization/tool-list smoke checks, packages runtime-only VSIX files,
attests the ZIP/VSIX assets, and publishes `SHA256SUMS`. CI separately runs a
live pinned VS Code 1.93.1 Extension Host smoke on Windows.

1. Update `[workspace.package].version` in [`Cargo.toml`](Cargo.toml), the VS Code
  package version, and the corresponding changelog/release notes.
2. Merge the release commit to `main` and confirm CI is green.
3. Create and push a matching tag:

```bash
git tag v0.1.0
git push origin v0.1.0
```

Prerelease tags such as `v0.1.0-preview.1` may share the base workspace version
`0.1.0`. A mismatched or malformed tag fails before any binaries are built.

## Pre-commit

Hooks run automatically on `git commit` (formatting, lint, whitespace, JSON/TOML
validity) and on `git push` (the test suite). Never bypass with `--no-verify`;
fix the code instead. Configuration: [`.pre-commit-config.yaml`](.pre-commit-config.yaml).

## Running the MCP server by hand

```bash
cargo run -p mindleak-mcp
```

Inside Git, both servers bootstrap one clone-local repository id in shared local
Git config and resolve to the same user-local repository directory from every
linked worktree. Call `storage_status` on either plane to inspect the id, path,
origin, and legacy migration result. Set `MINDLEAK_HOME` to relocate the shared
root, or `MINDLEAK_DB` / `LODESTAR_DB` only for an explicit direct override.

Then paste newline-delimited JSON-RPC requests on stdin, e.g.:

```json
{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}
{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}
```

> Pipe a request file to the server's stdin from any shell/harness — it reads
> one JSON object per line: `mindleak-mcp < in.jsonl > out.jsonl`.

## Debugging the extension

```bash
cargo build              # produce target/debug/mindleak-mcp(.exe)
npm --prefix editors/vscode run watch
```

Press **F5** in VS Code to launch an Extension Development Host. The extension
auto-detects the workspace `target/debug` or `target/release` binary.

## Environment variables

| Variable | Default | Used by |
|---|---|---|
| `MINDLEAK_WORKSPACE` | process working directory | worktree used for Git repository identity and project config |
| `MINDLEAK_HOME` | platform-local non-roaming state directory | shared per-repository storage root |
| `MINDLEAK_DB` | repository-id store, or workspace-local outside Git | explicit server graph override |
| `MINDLEAK_AGENT` | *(empty)* | agent id for attribution (`observed` edges); empty = off |
| `LODESTAR_AGENT` | *(empty)* | agent id for Lodestar claims; **required to publish** (ADR-0048) |
| `LODESTAR_MCP_BIN` | `target/release`, then `target/debug` | Lodestar server the claim gate drives; set it when publishing from a worktree with no local build |
| `MINDLEAK_CONFIG` | `<workspace>/.mindleak.toml` | per-project decay policy |
| `MINDLEAK_WORKING_SET_SIZE` | `7` | hard cap for the current agent's derived working set (1-32) |
| `MINDLEAK_AUTONOMOUS_CONSOLIDATION` | `false` | explicit opt-in to idle model-backed consolidation |
| `MINDLEAK_CONSOLIDATE_IDLE_SECS` | `300` | idle trigger (30-86400) |
| `MINDLEAK_CONSOLIDATE_MIN_INTERVAL_SECS` | `3600` | minimum attempt interval (60-86400) |
| `MINDLEAK_CONSOLIDATE_MAX_NODES` | `20` | candidates per pass (1-200) |
| `MINDLEAK_LLM_URL` | `http://localhost:11434/v1` | consolidation server (OpenAI-compatible) |
| `MINDLEAK_MODEL` | `glm4:9b` | consolidation model |
| `MINDLEAK_LLM_API_KEY` | *(empty)* | bearer token for hosted LLM servers (optional) |

## Adding an MCP tool

1. Add a method to the `MindLeak` facade in [`lib.rs`](crates/mindleak-core/src/lib.rs).
2. Add a definition to `list()` and a branch to `call()` in
   [`tools.rs`](crates/mindleak-mcp/src/tools.rs).
3. Add a test in [`tests/integration.rs`](crates/mindleak-core/tests/integration.rs).
4. Add a row to the tool table in [`README.md`](README.md).

## Known gaps

Be honest — an empty Known Gaps section is almost always a lie. The rough edges
and footguns, with impact and status:

- **A stalled wait is only bounded by the seven-day parking grace — SURFACED,
  not prevented.** — ADR-0046 lets `ask_question` address a peer, so an agent
  can park on one that never answers. The mutual case (a wait cycle) is now
  detected and reported by `fleet_view`, and answering any named task breaks it.
  What is *not* solved: a one-way wait on an agent that has vanished is not a
  cycle and is not flagged — correctly, since the addressee could still answer,
  but it means a task can sit parked for a week on someone who is never coming
  back. Nothing alerts either way: `fleet_view` is a pull, so the finding is only
  seen if a human or agent looks. Impact: bounded wasted wall-clock, never
  permanent. Fix would be a staleness threshold on an unanswered wait — an
  addressee with no live claim and no recent session is a different, weaker
  signal than a cycle and should read as such.
- **Accepting a design wrote `Accepted` into whichever worktree resolved the ADR
  path first — FIXED.** — Observed Jul 2026 while ADR-0044 was still an unmerged
  proposal on its own branch. Two ADR files in this checkout changed from
  `Status: Proposed` to `Status: Accepted` in the working tree, uncommitted,
  while the agent that owned the checkout was mid-pull-request and had accepted
  nothing. One of the two, ADR-0043, belonged to a different branch entirely —
  which is the tell: the write landed in a checkout that had no relationship to
  the decision being accepted. The write itself is human-triggered (`accept()` /
  `reject()` prompt for a name before calling `alignAdrStatus`), so this was
  never a status flipping itself; the defect was *where the write went*.
  `resolveAdrUri` ([`editors/vscode/src/designBoardController.ts`](editors/vscode/src/designBoardController.ts))
  walked `workspace.workspaceFolders` and wrote to the first folder containing
  the path. Under ADR-0038 several worktrees on different branches share one
  `spec.db` and are commonly open together, so "first folder containing this
  path" was close to arbitrary. — Medium impact: no data loss, but an ADR's
  declared status is evidence of a human decision, and this could plant that
  evidence on a branch nobody decided anything about — or, as here, drop an
  uncommitted edit into another agent's tree mid-commit, which is also the
  pre-commit stash race described below. Caught only because the owning agent
  read `git status` before pushing rather than trusting it. — Fixed Jul 2026:
  `chooseAdrTarget` never picks. One matching checkout writes as before; several
  ask the reviewer which one, and cancelling aborts without writing; none keeps
  the existing clear error. Deliberately *not* fixed by binding a design record
  to a worktree — that would put a machine-specific path in a database ADR-0038
  shares across checkouts, and it answers a question the reviewer is better
  placed to answer while already standing in the prompt.
- **Stalled ledger work is invisible: nothing notices a lapsed lease or a
  shipped change with no receipt — OPEN.** — Found Jul 2026 auditing why three
  tasks sat unfinished. They stalled for three *different* reasons and the board
  reported none of them:
  1. **A lapsed lease produces no signal.** `task:c3ef672e0ae3` (fleet view) was
     built and opened as a pull request, but `check_conformance` was never
     called and the lease simply expired. Its only conformance record is the one
     written during a later audit. The work exists in Git and does not exist in
     the ledger, and nothing anywhere says so.
  2. **Work that ships outside a claim window can never be certified.**
     `task:92778f8ad0f5` was delivered under an earlier pull request, so every
     honest evidence window for it is empty and the verdict is necessarily
     `needs_human`. This is the evidence contract behaving correctly — it
     refuses to certify what it cannot bound — but the task then waits on a
     human with nothing prompting one.
  3. **Cross-cutting work reads as `drift`, and by design cannot be repaired
     afterwards.** `task:05dade200195` ran the full loop with real evidence (2
     commits, 11 artifacts, complete provenance) and still resolved `drift`:
     *"governed code changed without a covering task"*, naming two goals other
     than its own. ADR-0041's `also_serves` is the answer, but it is fixed at
     creation with no later mutator — deliberately, because coverage added once
     conformance has complained is a rationalisation. So the only exit is human
     judgement.
  Blocked work then queues behind these silently: `task:0bcbb4220bcc` waited 78
  hours on (2), with zero conformance records of its own, and the fleet-overlap
  chain waited on (1). — Medium-to-high impact: no state is wrong and nothing is
  lost, but the board looks idle while three finished pieces of work sit
  uncertified, and the only way to find out is to go looking. It is the same
  shape as the ADR-loss problem — silent, and caught by accident. — Left for
  later; the fix is a read-only stall report (lapsed leases, `in_review` older
  than a threshold, tasks blocked by something already terminal or `in_review`)
  rather than any change to the evidence contract, which is behaving correctly
  in all three cases. Note that (1) is the only one that is purely mechanical;
  (2) and (3) are rules working as intended and want a human, not a fix.

- **The pre-commit stash race reports a failure that names the wrong thing —
  GUARDED, not fixed.** — `pre-commit` stashes every unstaged change before
  running hooks and restores it afterwards. Alone that is invisible; in a fleet
  it corrupts. If a second agent writes to the same working tree inside that
  window, the restore collides and hooks report `files were modified by this
  hook` — from `check-added-large-files` and `check-merge-conflict`, which modify
  nothing, about files the committer never touched. Observed Jul 2026: three
  consecutive commit attempts failed this way, each blamed a different innocent
  hook, and the real cause (two agents in one checkout) appeared nowhere in the
  output. The natural response is to retry, which widens the window. — Medium
  impact, high cost to diagnose: no data is lost, but the diagnosis is
  actively misleading and can consume an entire session. — `scoped-commit.mjs`
  now refuses (exit 3) when more than one worktree is attached and unstaged
  files outside the declared paths are live, naming them and pointing at
  `git worktree add`. That closes the sanctioned path only: a bare `git commit`
  can still walk into it, because the stash happens inside `pre-commit` itself
  and no hook can observe the tree as it was before its own framework moved it.
  The real fix is ADR-0038 isolation — one worktree per workstream.
- **Each worktree needs its own `node_modules` — FIXED.** — `npm ci` in
  `editors/vscode` costs ~13s and ~449 packages per worktree. Worse than the
  cost was the symptom: a fresh worktree failed at *push* time with
  `Cannot find module .../prettier/bin/prettier.cjs`, which says nothing about
  the real cause, and failed extension tests with an `npx` prompt offering to
  install `vitest` rather than a clear "dependencies not installed". Hit four
  times in one session. — Low impact, real friction: it made spinning up a
  worktree for a small docs change feel disproportionate, which is exactly the
  pressure that pushes agents back into the shared checkout ADR-0038 moved them
  out of. — Fixed Jul 2026: `make worktree-setup` installs just the extension
  deps. Hooks and cargo tools are shared through the common `.git` dir and the
  user's cargo bin, so a linked worktree needs nothing else, and running the
  full `make setup` per worktree would re-run `pip install` and
  `cargo install` for no reason.

- **A lapsed lease silently shrinks the evidence window a task can prove — OPEN.** —
  Observed Jul 2026 closing ADR-0026 task 4. Building three commits took longer
  than the lease, and the only route back to a live claim is `claim_task`, which
  opens a **fresh** `claim_started_at`. `evidence_for` is bounded by that window,
  so the three implementation commits sat outside it and the receipt covers only
  the final ADR commit plus its validation run. Nothing was lost and nothing was
  falsified — this is the evidence contract correctly refusing to certify work it
  cannot bound — but the durable proof under-reports the work, which is a
  different kind of wrong than over-reporting. `recover_claim` does not help: it
  is deliberately restricted to *legacy* pre-ADR-0030 owners and refuses a
  same-session expired claim with "requires a compatible legacy owner". — Medium
  impact: no incorrect completion, but proof-of-work is thinner than reality and
  the operator has no honest way to reattach. — Left for later: the fix is either
  a same-owner reattach that preserves the original window, or renewal semantics
  that survive a lapse when nobody else claimed the task. Both are policy
  decisions about how much an expired lease should forfeit, so they want an ADR
  rather than a quiet patch. Mitigation today: renew the lease before long
  builds.
- **A renamed ADR leaves an unreachable Design Board row forever — OPEN.** —
  Observed Jul 2026 while investigating "the Design Board seems to have errors".
  `list_designs` returned two rows, `design:0036-one-work-surface` and
  `design:0037-one-work-surface`, whose `adr_path` matches no file on any branch:
  both are residue from renumbering the same ADR (0036 → 0037 → finally 0040).
  Every tree row is wired to `mindleak.design.openAdr`, so clicking either throws
  and surfaces an error toast — the reported symptom. The cause is that
  `reconcile_designs`
  ([`facade/design.rs`](crates/lodestar-core/src/facade/design.rs)) is
  upsert-only and keys on ADR path, so a rename registers a new id and orphans
  the old one; there is no `retire_design`. — Medium impact: no decision is lost
  and no state is wrong, but the board accumulates unclickable rows that erode
  trust in it. — Left for later, and deliberately **not** fixed by auto-retiring
  designs whose file is absent: under ADR-0038 several worktrees on different
  branches share one `spec.db`, so "file missing from this checkout" is a normal
  branch-local condition, and retiring on it would delete live decisions. The fix
  is an explicit, attributed `retire_design` plus a rule about whether
  branch-local ADRs should register at all — that wants an ADR.
- **ADRs with a qualified status were dropped from the ledger in silence —
  FIXED.** — Observed Jul 2026. `parseAdrMetadata`
  ([`editors/vscode/src/designBoard.ts`](editors/vscode/src/designBoard.ts))
  required the status line to equal `proposed`/`accepted`/`rejected` exactly, so
  `Accepted (implemented)` and `Accepted (no symbol-lease primitive)` failed the
  check and returned `null`. ADR-0015 and ADR-0017 were therefore never
  registered at all, while `sync()` kept logging success with a lower count, so
  nothing reported the loss. — High impact: an accepted decision invisible to the
  design ledger is exactly the failure this ledger exists to prevent. — Fixed
  this run: `normalizeAdrStatus` strips a parenthetical qualifier, and
  `readWorkspaceAdrMetadata` now returns the skipped paths with a reason which
  `sync()` logs and warns on. Regression test: "accepts a status carrying a
  parenthetical qualifier".
- **One unreadable materialization blanked the whole Design Board — FIXED.** —
  `DesignBoardController.refresh()` fanned `design_promotion` out over every
  materialized design with `Promise.all`, so a single rejection rejected the
  batch, `provider.update` never ran, and the view silently kept stale contents
  behind one error toast. — Medium impact: the board looked merely out of date
  rather than broken. — Fixed this run: `Promise.allSettled`, with each failed
  lookup logged against its design id and the remaining rows still rendered.
- **New MCP tools are invisible until VS Code reloads, and the binaries cannot be
  rebuilt while it runs — OPEN.** — `cargo build --release` fails with `Access is
  denied (os error 5)` on `lodestar-mcp.exe` / `mindleak-mcp.exe` because the
  running servers hold the files open. So a session that adds a tool cannot
  exercise it, and there is no in-band signal that the advertised tool list is
  stale — the tool simply does not exist. — Low impact, high friction: purely an
  inner-loop cost, but it silently blocks end-to-end verification of anything
  added to the MCP surface within the same session. — Left for later; workaround
  is to reload the window (or restart the servers) before verifying new tools.
- **A dead extension-side server left every pane blank and the health line
  lying — FIXED.** — Observed Jul 2026: the MindLeak views were all
  empty while the agent-facing `mcp_*` tools worked normally. The extension
  spawns its **own** `mindleak-mcp` / `lodestar-mcp` children (`McpClient` in
  [`editors/vscode/src/mcpClient.ts`](editors/vscode/src/mcpClient.ts), resolved
  by `resolveBinaryPath` to the *bundled* `bin/`, not `target/release`), and the
  previous session's `taskkill` — the documented step before rebuilding the
  release binaries — killed them. Nothing restarted them, so the panes stayed
  dead for hours until the extension host happened to restart. The health line
  compounded it: `activate()` recorded `memory connected` once and never revised
  it, so the one surface that should have said something was confidently wrong.
  — Medium impact: no data loss, but the product looks broken and the cause is
  invisible unless you think to open the output channel. — Fixed Jul 2026: the
  client relaunches the server itself (three consecutive attempts, then it says
  a reload is needed), no longer logs from the exit handler during disposal —
  which was raising `Channel has been closed` in the extension host log — and
  publishes `connected` / `reconnecting` / `disconnected` to a state listener
  that the extension maps onto the plane's health line. The four independent
  health strings collapsed into the `RuntimeHealth` record they already modelled,
  behind one change-guarded `setHealth`. Note the fix is TypeScript, so an
  **installed** extension keeps the old behaviour until it is rebuilt and
  reloaded.
- **The Design Board silently swallowed a cancelled materialization, and planned
  from an empty summary — FIXED.** — `promote` / `revisePromotion` returned with
  no message, no log line, and no state change whenever any quick pick or input
  box was dismissed, so an accepted design simply stayed `pending` and a
  cancelled run was indistinguishable from a broken one; ADR-0033 sat that way
  for three days and read as an unusable tool. Separately, `parseAdrMetadata`
  hardcoded `summary: ""`, so Create-mode `plan_design_promotion` saw only the
  ADR title and drafted generic filler ("Review documentation", "Design a
  workflow model") — the same shape as the earlier hallucinated-task incident,
  and a direct route back into the ADR-0028 duplicate/orphan failure. — Medium
  impact: no data loss, but the Design Board was effectively unusable and its
  planning output untrustworthy. — Fixed Jul 2026: every abort path reports and
  logs, an empty objective list explains itself instead of closing, and
  `extractAdrSummary` carries bounded `## Decision` + `## Context` text into the
  design item planning reads. The store half mattered just as much:
  `reconcile_design_item` used `INSERT OR IGNORE`, so no repository pass could
  ever repair an already-registered empty summary; it now refreshes `title` and
  `summary` while leaving status, decision, proposer, and promotion state
  durable. Note the extension half is TypeScript, so an **installed**
  extension keeps the old behaviour until it is rebuilt and reloaded.
- **One shared stdio MCP server gave concurrent chat sessions the same agent id — FIXED.** —
  The first ADR-0030 implementation qualified identity with a *per-process*
  nonce in both MCP entry points, but VS Code multiplexes multiple concurrent
  chat sessions through a **single**
  long-lived MCP server process. All of those sessions therefore share one nonce
  and one identity (observed: `copilot-4e151e90` held three simultaneous live
  claims across independent sessions), so per-agent claim ownership, leases, and
  evidence attribution cannot distinguish the sessions. — Medium impact: owner
  guards and evidence loops treat distinct concurrent agents as one; there is no
  data loss, but coordination invariants degrade under real fleet use. — Fixed by
  ADR-0030 session registration: clients mint one token, both planes derive one
  stable identity, and every identity-bearing call is bound to that registered
  token rather than process state. The pinned Extension Host release smoke now
  asserts the session-qualified identity and current session-only task actions,
  rather than the removed process nonce/arbitrary allocation contract.
- **A server restart could strand a legacy base-id claim until lease expiry — FIXED.** —
  This run claimed work while the configured identity was the legacy `copilot`;
  after the ADR-0030 server restart the process identity became nonce-qualified,
  so owner-guarded lifecycle operations correctly refused the old owner's live
  claim. — Medium migration impact: work is preserved, but the new process must
  wait for lease expiry. — `recover_claim` now requires expiry/grace, exact owner,
  compatible base, and a reason; it starts a fresh window and appends the prior
  owner/window/status to `task_claim_transfers`. Live claims, wrong bases, and
  qualified sibling sessions are refused.
- **`recall`'s one-off "100% failure" was a missing embedding model, not a bug.** —
  Telemetry showed `recall` as the only tool with an error (1 call / 1 error, 3ms
  fast-fail); the recorded detail was `/v1/embeddings status 404`. Root cause: the
  embedding model (`MINDLEAK_EMBED_MODEL`, default `nomic-embed-text`) was not yet
  pulled into Ollama, so the query-embedding POST 404'd — an environment/config
  issue (ADR-0008: recall is optional and off the deterministic hot path).
  Verified it degrades cleanly (typed `MindLeakError::Http`, no panic/block, no
  hot-path poisoning — 2362 events, only `recall` ever errored) and, once
  `ollama pull nomic-embed-text` was run, recall returns scored results (94ms,
  confirmed live). — Low impact (optional feature, self-announcing at startup and
  documented in QUICKSTART/USAGE). — Resolved: operator remediation already
  documented; contract covered by
  `recall_and_index_degrade_cleanly_when_the_embedder_is_unreachable` (unreachable
  model → error) and `recall_returns_empty_not_error_when_the_index_is_unpopulated`
  (reachable model, empty index → empty, not error); observed on
  `task:2c86cc1f51ea`.
- **Cross-goal bindings on shared *source* files caused false drift — RESOLVED.** —
  Repeated per-task `link_goal_to_code` calls left 10 lodestar /
  mindleak source files each bound to two active goals (e.g. `model.rs`, `lib.rs`,
  `store/coordination.rs`, `facade/conformance.rs`,
  `crates/mindleak-core/src/graph/evidence.rs`), so a commit serving goal A reports
  drift against goal B. — RESOLVED for documentation: goals govern code, not the
  shared prose every task touches, so `evaluate_conformance` now ignores `governed`
  bindings on documentation nodes **at read time** — deleting nothing (commit
  `8ce8516`, which superseded and removed the rejected auto-delete-on-restart
  *clobber* `b55f2a0`; an explicit `forbid_change` lock on a doc is still honoured).
  The one-time clobber had already dropped the 10 documentation bindings (89 → 79)
  before removal; those were benign pollution and are re-linkable. `unlink_goal_from_code`
  + `governing_goals` (commit `6b22bca`) provide an explicit, audited prune path. —
  **RESOLVED Jul 2026 (task:c4bae4cc6ec2)** via human-in-the-loop
  `unlink_goal_from_code` triage: each file's true owner is its plane's objective,
  so the mistaken bindings were the *MindLeak-graph* objective
  (`local-temporal-context-graph`) on the 8 Lodestar source files, and the
  `principled-verified-delivery` **constraint** (a cross-cutting rule, not a
  per-file owner) on `model.rs` and `graph/evidence.rs`. Those 10 bindings were
  dropped (explicit/audited, no auto-delete); each of the 10 files now has exactly
  one governing goal, so honest commits no longer drift. Data-plane only — no code
  change. — **Follow-up Jul 2026:** the triage did not, and could not, cover files
  whose two bindings are *both* accurate — `crates/mindleak-mcp/src/tools/mod.rs`
  is legitimately the graph engine's MCP surface *and* the ADR-0030 session
  registrar. That residue is addressed by ADR-0041 (declared coverage), not by
  more unlinking: there is no wrong binding left to remove.

- **Blind design promotion could omit governing goals or duplicate existing work
  — FIXED.** — ADR-0024
  was correctly implemented across Lodestar, MindLeak, the extension, evaluation,
  and docs under promoted `task:46dd49254e4c`, but that task belongs only to
  `goal:local-temporal-context-graph`; exact commit evidence produced conformance
  audit `65` with `drift` for the independently governed Intent Plane and
  principled-delivery surfaces. The ADR-0018 audit confirmed the same shape:
  promoted `task:d2900fdfa41b` belongs to the graph goal while its required git
  safety scripts are governed by `goal:principled-verified-delivery`, so exact
  evidence for green commit `321cf17` produced audit `68` with `drift`. ADR-0028
  exposed the second failure mode: deterministic fallback created unblocked
  `task:735e36892ffa` even though release-gated pilot `task:7f5ae1198134` already
  represented the exact work under the Intent Plane objective. — High
  coordination impact: a design could look materialized while bypassing its real
  delivery chain. — Fixed Jul 2026 (`task:53a02c15fa67`): planning is read-only;
  humans review explicit create/link/no-work plans; create may span objectives;
  link reuses authoritative tasks; materialization is atomic/idempotent; repairs
  append attributed revisions and replace only the current projection. The bad
  ADR-0028 task was durably abandoned rather than deleted or relinked by hand.

- **The `evidence_for` → Lodestar conformance seam is sound, but convention-
  sensitive.** — The producer and consumer agree on schema version 1, normalized
  `agent:<id>` observation provenance, successful-execution subset rules, and
  inclusive claim bounds. Executions source `modified` / `failed_on`; commit
  intent nodes source `refactored`, so every changed or failed node names a
  source accepted by `validate_evidence_shape`. This is not a product bug. The
  otherwise-unenforced ingestion convention is pinned by
  `evidence_for_emits_self_consistent_provenance`, which exercises execution,
  failure, and commit evidence and fails if a future ingester emits an unusable
  bundle; `evidence_for_normalizes_prefixed_agent_and_includes_window_boundaries`
  pins agent normalization and inclusive endpoints. — Verified Jul 2026 on
  `task:40c4e757e601`.

- **The real-agent product gate is narrow.** — Three runs per arm on one
  composite typed-session fixture with Copilot CLI 1.0.63 / Haiku 4.5 cross the
  exploration and success thresholds, but do not establish general performance
  across repositories, models, or long-running teams. The two-agent duplicate-
  work mechanism is now covered by ADR-0024's deterministic two-plane overlap
  benchmark, but independent agents' scope accuracy and willingness to heed an
  advisory are not. — Medium impact on claim breadth. — Productization may
  proceed; broader external replications remain required for universal efficacy
  claims.

- **Policy packs are immutable and reviewable, but bootstrap activation and
  upgrade amendment diffs are not yet implemented.** — ADR-0026 task 2 now
  validates/registers immutable pack versions, proposes Common Core, persists
  dispositions, materializes adopted clauses with source provenance, and blocks
  conflicts/upstream rewrites. An ungoverned project still needs task 3's
  deterministic fact discovery plus atomic draft activation; a newer pack
  version is deliberately refused until task 5 supplies an attributed amendment
  diff. — Medium onboarding impact, no silent policy risk: current behavior
  fails closed (`draft` / `needs_human` / amendment-required) rather than
  inheriting or auto-activating policy.

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
- **Episodic edges previously used ingestion wall-clock time.** — Delayed passive
  execution/commit ingestion could invert failure/change/success chronology and
  fabricate or hide consequence. — High impact on signal correctness. — Fixed
  this run: execution and commit edges now use authoritative record timestamps,
  with regression tests.

- **Symbol and import extraction remains heuristic and partially scoped.** —
  Static JS/TS named imports now produce cross-file `calls`, but default and
  namespace calls, re-exports, path aliases, dynamic imports, and other language
  import syntaxes are not resolved. Type hierarchy supports simple named local
  and imported JS/TS heritage, not default/namespace targets or expression-based
  mixins. Non-JS brace/indent extractors also remain regex-based. — Medium impact
  on graph completeness. — Tracked: expand fixture-backed deterministic parsers;
  Tree-sitter remains the precision upgrade (ADR-0002).
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
- **Ingest tools are unauthenticated (by design).** — Any client with stdio
  access to `mindleak-mcp` can write nodes/edges. — Acceptable for local
  single-user use; the server has no network listener. Do not expose it over a
  network without an auth layer (see [docs/SPEC.md § 8](docs/SPEC.md)).
- **Passive execution evidence depends on VS Code shell integration.** — VS Code
  1.93 shell start/end events provide command/exit evidence; unsupported or
  conflicting shells report degraded capture and are not guessed from terminal
  text. Concurrent terminal executions can both observe one workspace mutation,
  so changed paths prove temporal overlap rather than process-level causality. —
  Medium impact on provenance precision in overlapping command sessions.
- **Lodestar worktree sharing was path-based, then checkout-root based.** — The
  original server used `LODESTAR_DB` or the process CWD; the first fix resolved
  through Git's common directory but still privileged the checkout owning
  `.git`. — Low impact on correctness, high coupling to physical topology. —
  **Superseded Jul 2026 by ADR-0038:** both planes now resolve one random
  per-clone repository id from shared local Git config and use the same
  platform-local user store from every linked worktree. Explicit DB overrides
  still win; scratch use outside Git remains workspace-local.
- **Unit Test MCP 1.3.6 cannot validate this workspace reliably.** — Its Vitest
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
- **Disposable Git fixtures inherited the parent hook's alternate index —
  FIXED.** — Committed-snapshot Cargo hooks set `GIT_INDEX_FILE`; child `git`
  commands in repository-state and publisher tests inherited it even when they
  changed CWD to a temporary repository. A fixture `git add README.md` therefore
  staged its one-line file into the parent index, and fixture-local `user.name` /
  `user.email` leaked into shared clone config. — High impact: the next scoped
  commit could carry a destructive parent-repository edit under the wrong
  identity. — **Fixed Jul 2026:** production repository discovery and every Rust
  / Node disposable Git harness clear `GIT_DIR`, `GIT_WORK_TREE`,
  `GIT_COMMON_DIR`, `GIT_INDEX_FILE`, and object-directory overrides before
  invoking Git. The contaminated README was restored exactly and local identity
  overrides were removed; focused and full pre-push suites pass.
- **The extension toolchain has one low-severity development advisory.** —
  Vitest resolves `esbuild` 0.27.7, affected by GHSA-g7r4-m6w7-qqqr when its
  development server runs on Windows. `npm audit --omit=dev` is clean and the
  package is not shipped with the extension; a normal `npm audit fix` finds no
  compatible update. — Low impact. — Left open until Vitest accepts a fixed
  `esbuild`; do not use `--force` to hide the compatibility decision.
- **Lodestar task recovery and retirement verbs.** — `reopen_task` returns a task
  stranded in `in_review` or a manual `blocked` hold to claimable `open`, and
  `abandon_task` retires a nonterminal task to terminal `abandoned` (facade + MCP
  tool, regression-tested), making `TaskStatus::Abandoned` reachable and closing
  the retire-a-mis-filed-task gap. — Resolved Jul 2026. Note: the verbs are wired
  in source, but a stale running MCP binary may not expose them until
  rebuilt/restarted (see the stale-binary gap above).
- **`renew_lease` and re-claim now share one evidence-window rule.** — Renewal is
  a heartbeat for a still-live lease and preserves `claim_started_at`; it refuses
  after expiry. A lapsed owner must win `claim_task` again, which resets
  `claim_started_at` and opens a fresh conformance evidence window. The guarded
  single-statement CAS and both paths are regression-tested. — Resolved Jul 2026.
- **Duplicate `define_goal` title+statement surfaces a raw SQLite error.** — A
  third goal sharing a title and statement collides on the derived
  `goal:{slug}-{hash(statement)}` id and fails with an opaque `UNIQUE
  constraint` error instead of a typed `LodestarError::Invalid`. — Low impact
  (edge case; goals are rarely exact duplicates). — **Fixed Jul 2026:**
  `store::define_goal` pre-checks the derived id and returns a typed
  `LodestarError::Invalid` pointing the author at `supersede_goal`; regression
  test `redefining_an_identical_goal_is_a_typed_error_not_a_raw_sqlite_fault`.
- **A dead defensive guard remains in `record_conformance_and_transition`.** —
  It errors when a predecessor has more than one successor, but
  `task_handoffs.predecessor_id` is the PRIMARY KEY, so the count is always at
  most one and the branch can never fire. — No functional impact; kept as
  documented defense-in-depth rather than removed, since the PK is the real
  guard. — Noted during the Jul 2026 audit.
- **Conformance preflight and completion could disagree on identical evidence.**
  — `check_conformance` returned `aligned` for task `task:aae950aecd78`, then
  `complete_task` immediately reran the optional semantic judge, returned
  `needs_human`, and stranded the task in review despite no evidence or intent
  change. — High impact on verified delivery. — Resolved Jul 2026 by ADR-0025:
  checks now return a durable id + state token, and completion consumes that
  exact audit result without a second model call (task `task:1b5bdafd5e99`).
- **MCP build identity exposes stale running binaries.** Both servers now report
  `serverInfo.version` as `<package-version>+<12-character-git-sha>` during MCP
  initialize. Compare the suffix with `git rev-parse --short=12 HEAD`; a mismatch
  means the server must be rebuilt and restarted before debugging source
  behaviour or relying on newly added tools. The shared Cargo build helper watches
  Git HEAD/ref changes and supports `MINDLEAK_BUILD_SHA` outside a checkout. —
  Resolved Jul 2026.
- **Docs-only design tasks could not complete via conformance, stranding
  successors — FIXED.** — A design task produces a docs commit; `complete_task` runs
  ADR-0009 code conformance, which returns `needs_human` ("evidence does not touch
  code bound to the task goal") and parks the task in `in_review` forever. Any
  implementation task chained `blocked_by` a docs-ADR predecessor then never opens
  (`blocked_by` clears only on predecessor `done`), and with no live `reopen_task`
  it cannot be un-gated — clearing the gate via `block_task(id, None)` leaves it
  `blocked` with no predecessor and no path back to `open`. — High impact on the
  design-first workflow. — Fixed for registered *design items* by the accepted
  ADR-0023 Design Board path: a human `accept_design` completes design review
  without code conformance, then a separately reviewed create/link/no-work plan
  maps it to executive work. Blind fallback creation was removed after ADR-0028
  exposed a duplicate-task failure. A docs-only task inside an *objective's*
  task chain (not a registered design item) —
  e.g. the AGENTS.md/README/USAGE/SPEC-INTENT task closing the ADR-0029 advise
  chain — still lands `in_review` via the same honest `needs_human` verdict. — **Fixed
  Jul 2026:** `resolve_task(task_id, human)` (facade + MCP) is the task-level
  mirror of `accept_design` — it human-accepts an `in_review` task to `done` with
  no code-conformance re-run while preserving the original audit, opens any
  blocked successor, and refuses self-resolution by the reviewed agent (the
  worker read from the task's conformance evidence). `reopen_task` and
  `abandon_task` retain their distinct recovery and retirement meanings. Tests:
  `resolve_task_accepts_an_in_review_task_to_done`,
  `resolve_task_refuses_self_resolution_by_the_reviewed_agent`,
  `resolve_in_review_opens_a_blocked_successor`.
- **Human task-review attribution is checked but not persisted.** —
  `facade::executive::resolve_task` requires a non-empty human identity and
  refuses the worker recorded in the latest conformance evidence, but
  `store::coordination::resolve_in_review` records only the transition to
  `done`; the supplied reviewer and decision timestamp are not written to an
  append-only review record. — Medium audit impact: the ledger proves that a
  task required review and later became done, but cannot answer who accepted
  it. The Work view asks for identity only to enforce independent review and
  does not claim durable attribution. — **Left for later:** add a task-resolution
  audit record and expose it in task proof without rewriting conformance history.
- **`next_task` surfaces non-actionable policy tasks.** — A `constraint` goal was
  decomposed into four tasks that merely restate the constraint and can never
  accrue completion evidence; `next_task` (oldest-first) hands one out on every
  call. — Low-medium impact: agents are handed a zombie. — **Fixed Jul 2026:**
  `decompose_goal` now returns `LodestarError::Invalid` for `constraint`/
  `invariant` goals (only `objective` goals decompose); the four restatement
  tasks were retired with `abandon_task`; regression test
  `constraint_goals_cannot_seed_junk_and_next_task_surfaces_actionable_work`.
- **Injected embedders made `MindLeak` non-`Send`.** — Commit `5d52877` added
  `Box<dyn TextEmbedder>` without the thread-safety contract required when the
  maintenance runtime moves `MindLeak` into `std::thread::spawn`. — High impact:
  the workspace build and strict clippy were red. — Resolved Jul 2026 by making
  `TextEmbedder: Send + Sync` and adding compile-time and unit regression
  assertions that `MindLeak: Send` (Lodestar task `task:e0548f57556a`).
