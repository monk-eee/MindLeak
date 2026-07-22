# Developing MindLeak

From a clean machine to the engine building, tested, and the extension running.
If you get stuck, that is a defect — fix it or add it to [Known gaps](#known-gaps).

## Prerequisites

- **Rust** 1.75+ (via [rustup](https://rustup.rs)); MSVC toolchain on Windows.
- **Node** 18+ and npm (for the VS Code extension).
- **Python** 3.8+ with `pip` (for the `pre-commit` framework).

## One-time setup

```bash
git clone https://github.com/monk-eee/MindLeak
cd MindLeak

# Rust components
rustup component add rustfmt clippy

# Pre-commit hooks (client-side enforcement)
pip install pre-commit
pre-commit install
pre-commit install --hook-type pre-push

# Extension dependencies
npm --prefix editors/vscode install
```

On systems with `make`, `make setup` does the hook + extension steps.

**Success looks like:** `cargo test --all` reports `test result: ok` for every
crate, and `target/debug/mindleak-mcp` starts and prints
`[mindleak-mcp] ready — graph at …` on stderr.

## Everyday commands

| Task | `make` | Direct command |
|---|---|---|
| Build | `make build` | `cargo build` |
| Test | `make test` | `cargo test --all` |
| Format | `make fmt` | `cargo fmt --all` |
| Format check | `make fmt-check` | `cargo fmt --all -- --check` |
| Lint (Rust) | `make clippy` | `cargo clippy --all-targets --all-features -- -D warnings` |
| Lint (extension) | `make ext-lint` | `npm --prefix editors/vscode run lint` |
| Test (extension) | `make ext-test` | `npm --prefix editors/vscode test` |
| Compile extension | `make ext-compile` | `npm --prefix editors/vscode run compile` |
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
```

## Pre-commit

Hooks run automatically on `git commit` (formatting, lint, whitespace, JSON/TOML
validity) and on `git push` (the test suite). Never bypass with `--no-verify`;
fix the code instead. Configuration: [`.pre-commit-config.yaml`](.pre-commit-config.yaml).

## Running the MCP server by hand

```bash
MINDLEAK_DB="$PWD/.mindleak/graph.db" cargo run -p mindleak-mcp
```

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
| `MINDLEAK_DB` | `<cwd>/.mindleak/graph.db` | server graph location |
| `MINDLEAK_AGENT` | *(empty)* | agent id for attribution (`observed` edges); empty = off |
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

- **Symbol/`calls` extraction is heuristic and in-file only.** — `calls` edges
  are resolved within a single file by name; cross-file calls are not linked, and
  brace/indent scoping does not account for braces inside strings or comments. —
  Medium impact on graph completeness. — Tracked: a Tree-sitter backend is the
  precision upgrade (see [ADR-0002](docs/adr/0002-sqlite-decay-over-vector-llm.md)).
- **The consolidation network round-trip is untested.** — Everything around it is
  covered (client construction, the JSON contract, error propagation), but the
  live `/v1/chat/completions` call is not exercised in CI (needs a running
  model). — Low impact. — Unowned; an HTTP-mock test would close it.
- **Ingest tools are unauthenticated (by design).** — Any client with stdio
  access to `mindleak-mcp` can write nodes/edges. — Acceptable for local
  single-user use; the server has no network listener. Do not expose it over a
  network without an auth layer (see [docs/SPEC.md § 8](docs/SPEC.md)).
- **Lodestar conformance reads caller-supplied node ids, not live MindLeak
  telemetry.** — `check_conformance` computes drift/violation from the goal↔code
  links plus the change set the caller passes; it does not yet query MindLeak's
  actual `modified`/`failed_on` edges to confirm what really changed. — Medium
  impact on conformance accuracy. — Deferred; the loose node-id seam (ADR-0004)
  is intentional, the deeper read is a follow-up.
- **Lodestar worktree sharing is path-based, not git-aware.** — The Intent Plane
  DB resolves from `LODESTAR_DB` else `<cwd>/.lodestar/spec.db`; sibling git
  worktrees share one plane only if pointed at the same path. — Low impact. —
  Git-common-dir auto-resolution is a documented follow-up (SPEC-INTENT §3).
- **Unit Test MCP 1.3.6 cannot validate this workspace reliably.** — Its Vitest
  discovery finds `src/util.test.ts`, but `run_tests` reports a passing total of
  zero even for that explicit path. On Windows, a backslash Cargo root is
  rejected as `INVALID_ROOT_DIR`; normalizing it to forward slashes runs the
  custom command but also reports a passing total of zero. — High impact on local
  proof: MCP results cannot currently establish that tests executed. — Left open
  in the external adapter; CI remains the executable test authority until repaired.
