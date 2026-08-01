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
# --install-hooks installs every type in default_install_hook_types
# (pre-commit, pre-push, post-commit) in one step. Installing only some — the
# old two-line form omitted post-commit — leaves commits with no provenance,
# which the pre-push hook-health check now refuses.
pre-commit install --install-hooks

# Extension dependencies
npm --prefix editors/vscode install

# MCP servers, installed once per machine outside every worktree (ADR-0073)
cargo build --release -p mindleak-mcp -p lodestar-mcp
node scripts/install-servers.mjs
```

On systems with `make`, `make setup` does the hook + extension steps, and
`make install-servers` does the last one.

### Open a window on the worktree you are editing (ADR-0073)

Node ids are repository-relative, and a file is made relative against the
server's workspace root. A window rooted somewhere other than the worktree it
edits cannot place those files, so they are refused and never reach the graph —
measured at 4% of all `ingest_file` calls while every window was rooted at the
primary checkout.

So open the worktree itself as the workspace folder. The extension contributes
both servers to that window and roots each one at the folder the window opened,
so the servers follow the window. All worktrees still share one graph and one
board — the repository id comes from the git common dir, not from the folder you
opened.

The servers are deliberately **not** built per worktree. They are installed once
at `~/.mindleak/bin`, which the extension prefers over any worktree build. If a
server fails to start with a missing executable there, run `make install-servers`.
After installing a new build, restart the servers (or reload the window) so
clients pick it up.

There is no committed `.vscode/mcp.json`. The extension provides both planes
through `mcpServerDefinitionProviders` (ADR-0073), so where a binary lives is
decided by one rule — `resolveBinaryPath` in editors/vscode/src/util.ts — rather
than by a config file carrying a second copy of it. This needs the extension
installed and VS Code 1.101 or newer; `npm --prefix editors/vscode run
package:vsix` then `code --install-extension` after pulling a build that changes
the provider.

Never copy the binaries into a worktree's own `target/release`: cargo's
fingerprints would still read fresh, so that worktree would never rebuild and
would silently serve a binary that does not match its source.

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
| Merge audit | `make merge-audit` | `node scripts/merge-audit.mjs` — fails if a merged branch has commits that never reached `main` |
| Delivery queue | `make queue` | `node scripts/delivery-queue.mjs` — show the queue and update the branch whose turn it is (ADR-0062). `make queue-watch` runs it as an agent |
| Artefact hygiene | `make sweep` | `node scripts/artefact-sweep.mjs` — report reclaimable build output; `ARGS=--apply` acts. Diagnosis only: the sweep already runs from `make queue-watch` on a cadence, under a lock in the common Git directory |
| Board health | `make board-health` | `node scripts/board-health.mjs` — separates parked work a human must decide from work nobody can resolve, and lists stranded claims (ADR-0058). Needs `LODESTAR_SESSION_ID` and a release `lodestar-mcp` |
| Stranded report | `make stranded-report` | `node scripts/stranded-report.mjs` — for each lapsed claim, names the commit that most likely shipped it, with a confidence. An agent cannot close these (ADR-0048); this makes confirming them a judgement rather than an investigation |
| Design audit | `make design-audit` | `node scripts/design-audit.mjs` — reports drift between the ADR files and the design ledger. Local only: it reads the ledger through a release `lodestar-mcp`, which CI has no database for |
| Changelog | `make changelog` | `node scripts/changelog.mjs` — show what the next release contains. A change adds `changelog.d/<section>-<slug>.md`; **do not edit `CHANGELOG.md` in a pull request** (ADR-0056) |
| Everything CI runs | `make ci` | see [`.github/workflows/ci.yml`](.github/workflows/ci.yml) |

> **`make` is optional.** Every target maps to the direct command in the
> right-hand column — `cargo`, `npm`, and `git` are identical on Linux, macOS,
> and Windows, so run those directly if `make` is unavailable.

## When a generated file conflicts, regenerate it

`docs/adr/README.md` is derived entirely from the ADR files. Every branch that
adds an ADR appends a row at the same place, so merging `main` into a branch
that added one conflicts every time. This is expected and it is not a merge to
reason about:

```bash
git checkout --ours docs/adr/README.md
make adr-index          # or: node scripts/adr-index.mjs
git add docs/adr/README.md
git commit --no-edit
```

**Do not hand-resolve it.** Keeping "both sides" of a generated table produces a
duplicated or misordered index that the pre-commit check then rejects, so the
hand-resolution is discarded work. `.gitattributes` explains at length why a
`merge=union` driver is not the answer either — GitHub does not honour merge
drivers, so the union resolution exists only in your checkout while every
reviewer sees a phantom conflict. A generated file is regenerated, never merged.

The same rule covers anything else under a generator: regenerate, then stage.
`CHANGELOG.md` avoids the problem entirely by not being edited in a pull request
at all — changes land as `changelog.d/` fragments and are assembled at release
(ADR-0056).

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

## The delivery queue

`main` requires branches to be up to date before merging. With several armed
pull requests that becomes a traffic jam: every merge makes all the others
stale, and each one that updates itself burns a full check run against a `main`
that the next merge invalidates again.

The queue takes those turns in order (ADR-0062):

```bash
make queue          # show the queue, update whichever branch's turn it is
make queue-watch    # run it as an agent until you stop it
node scripts/delivery-queue.mjs --dry-run   # decide, change nothing
```

It reads the queue from GitHub — **a pull request with auto-merge armed is a
queued one** (ADR-0045), ordered by when it was armed. Exactly one branch is
updated at a time; that is the entire mechanism.

**It never merges.** Merging stays with GitHub's auto-merge and the same five
required checks, so the queue cannot become a second way into `main` that branch
protection does not govern. Nothing depends on it running: an unattended queue
just means branches go stale the way they did before.

Branches it will not touch, and reports instead:

- **a real conflict** — reconcile it in its own worktree (ADR-0038); it must not
  hold up everything behind it
- **failing checks** — updating would only burn CI to fail again
- **checks still running** — waiting is the point; a second update now would
  invalidate the first before it lands

### Build-artefact hygiene rides on the watcher

A fleet of worktrees builds the same crates over and over, and nothing ever
removes the result: a measured 149 GiB across 124 cache directories, on clean
branches already merged into `main`. Cleanup that depends on someone
remembering does not happen, because the agent that filled a worktree has
finished and moved on before it is safe to empty it.

So the sweep has no schedule of its own. It rides on `make queue-watch`, which
is already persistent and already single-owner: once at startup, then on a
bounded cadence with the last run recorded in the common Git directory. It
takes a lock there too, so two worktrees can never sweep at once.

```bash
node scripts/artefact-sweep.mjs           # what would be reclaimed, and why not
node scripts/artefact-sweep.mjs --apply   # reclaim it now
make sweep                                # the same, via make
node scripts/delivery-queue.mjs --watch --no-sweep   # the queue, without hygiene
```

`make sweep` is for diagnosis; the watcher is the mechanism. Both report by
default, because no report can be un-deleted, and both take the same lock.

It removes only reproducible build output — `target/debug`, `target/release`,
`target/llvm-cov-target`, and the extension's `node_modules`, `out`, `coverage`
and `.vscode-test`. It never removes a worktree, source, Git state,
`target/tmp`, telemetry, completion offers, release assets, or **the bare
host's `target/release`, which serves the running MCP binaries**. It skips any
worktree that is detached, dirty, unmerged, backing an open pull request, or
active within the grace period, and re-checks every one of those immediately
before deleting, so a plan that went stale while the disk was being walked is
abandoned rather than acted on. Anything ambiguous is skipped and counted in
the report.

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
2. Assemble the changelog: `node scripts/changelog.mjs --release <version>` folds
  the `changelog.d/` fragments, and anything under `## [Unreleased]`, into one
  dated section (ADR-0056). Commit that; do not hand-edit `CHANGELOG.md`.
3. Merge the release commit to `main` and confirm CI is green.
4. Create and push a matching tag:

```bash
git tag v0.1.0
git push origin v0.1.0
```

The pre-push hook checks a tag differently from a branch: it confirms the tag
names a commit already on `origin/main`, and nothing else. Tagging is how a
release is chosen, so choosing an unmerged commit would ship code that never
passed review. The branch rules — a live claim, an attached non-protected
branch, a clean worktree — do not apply and are not enforced; tagging from a
detached HEAD is fine.

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
| `LODESTAR_SESSION_ID` | *(empty)* | 32-hex session id resolved to this agent's identity; **required to publish** (ADR-0049) |
| `LODESTAR_AGENT` | `agent` | display name for this process's sessions in reports; not part of the agent id (ADR-0054) |
| `LODESTAR_MCP_BIN` | `target/release`, then `target/debug` | Lodestar (Intent Plane) server the claim gate and publisher drive; set it when publishing from a worktree with no local build, or the ledger is unreachable and the push is refused as unattributable to a claim |
| `MINDLEAK_MCP_BIN` | `target/release`, then `target/debug` | MindLeak (Memory Plane) server the publisher drives to record the published commit; set it when publishing from a worktree with no local build, or the commit is not recorded — the work does not certify and no completion offer is produced |
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
2. Add a definition to `definitions()` and a branch to `call()` in the matching
   module under [`tools/`](crates/mindleak-mcp/src/tools/).
3. Add a test in [`tests/integration.rs`](crates/mindleak-core/tests/integration.rs).
4. Add a row to the tool table in [`docs/TOOLS.md`](docs/TOOLS.md).

## Known gaps

Be honest — an empty Known Gaps section is almost always a lie. The rough edges
and footguns live in [`gaps.d/`](gaps.d/), one file per gap.

They are fragments rather than a list in this file for the reason ADR-0056 gave
for [`changelog.d/`](changelog.d/): a shared append-only section is a
serialisation point, and every branch that recorded a gap edited the same lines.
That produced a merge conflict on almost every pull request which expressed no
disagreement at all — two agents adding two unrelated observations to the same
paragraph. Two branches never write the same path, so recording a gap cannot
conflict with recording a different one.

Unlike a changelog fragment, a gap fragment is never folded back into this file.
A gap has no release event — it is open until it is fixed — so folding would put
the shared list, and the conflict, straight back.

```bash
node scripts/gaps.mjs --list     # read every open gap
node scripts/gaps.mjs --check    # validate fragments (runs in the hook)
```

**Record a gap:** add `gaps.d/<slug>.md` opening with a `- **` bullet that names
the gap, where it is (file plus symbol or test name), its impact, and whether it
was fixed this run or left for later.

**Close a gap:** delete its fragment in the commit that fixes it, so the fix and
the retraction are one reviewable change rather than two that can drift apart.
