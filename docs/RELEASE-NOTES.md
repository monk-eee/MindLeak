# MindLeak 0.1.4 Release Notes

MindLeak 0.1.4 turns the local memory and intent planes into a more dependable
multi-agent work surface. It makes worktree identity and server rooting explicit,
bounds concurrent ownership, carries memory retrieval onto the claim path agents
already use, and makes completion, conformance, review, and rescue state visible
instead of relying on somebody remembering a second call.

## Highlights

- **One repository, every linked worktree** — all worktrees still share one
  repository-id store, while each editor window roots file ingestion at the
  checkout it actually edits. Paths from sibling worktrees resolve to the same
  repository-relative artifact, and unplaceable foreign paths fail visibly
  instead of creating a second identity (ADR-0038, ADR-0073).
- **The extension provides both MCP servers** — VS Code 1.101+ discovers the
  packaged MindLeak and Lodestar servers through the extension's MCP provider.
  The supported editor path no longer depends on a committed `.vscode/mcp.json`;
  headless clients continue to use the release installer and generated config.
- **Fleet coordination has a bounded shape** — one logical agent can hold at
  most three concurrent claims, lapsed ownership is visible, overlap is graded
  by same-branch collision versus cross-branch merge risk, and branch context is
  carried at claim and board decision points. Claims remain advisory rather than
  filesystem locks.
- **Memory retrieval rides on the claim** — a successful scoped claim returns a
  bounded MindLeak `check_overlap` preflight with the exact paths and symbols.
  Telemetry counts that bundled retrieval as a memory read, so adoption is
  measured against the workflow rather than against an optional standalone call
  (ADR-0066).
- **Completion and review are explicit** — publication can offer the exact
  evidence/check payload; verified merge evidence can close already-landed work
  without manufacturing an execution receipt; non-aligned work enters a visible
  human-review queue with attributed reviewer labels (ADR-0058, ADR-0065,
  ADR-0071).
- **The task lifecycle is auditable** — task events form an append-only history,
  paused/lapsed work reaches its owner or a reviewed successor, and `open_session`
  can surface rescue work, addressed questions, paused ownership, and work waiting
  on a person without transferring anything implicitly.
- **The graph is harder to corrupt silently** — Rust `mod`/`use` structure can be
  re-ingested across the existing graph, real artifacts safely replace importer-
  created stubs, optional HTTP/embedding responses fail closed, and build identity
  notices distinguish a stale binary from a checkout that is merely behind.

## Compatibility and migration

- **VS Code 1.101 or newer is required.** This is the first editor release floor
  with the MCP server-provider API used by the extension. Reinstall or upgrade
  the targeted VSIX so the provider and native servers move together.
- **At most three concurrent Lodestar claims per logical agent.** Lapsed claims
  count until they are reclaimed, released, completed, or retired.
- **The canonical task/design/constitution tool vocabulary is smaller.** Legacy
  aliases remain a transition aid, but clients and scripts should use the
  currently advertised grouped verbs and discriminators.
- **Repository paths are fail-closed.** Files under any worktree of the same
  repository are accepted and canonicalized; paths outside every known root are
  refused.

## Install

Download the archive matching the target platform, verify it against
`SHA256SUMS` and its GitHub artifact attestation, extract it, then run this once
from the workspace to register both MCP servers:

```text
node /path/to/extracted/install.mjs
```

The installer requires Node.js 20 or newer. It smoke-tests both servers before
copying them to `.mindleak/bin/<version>/`, preserves unrelated registrations and
comments in `.vscode/mcp.json`, and adds local-state privacy rules to `.gitignore`.
Use `--agent <label>` to set the human-readable identity base. Each client mints
and registers its own session token; the label is not an owner credential.

For the editor experience, install the matching platform-targeted `.vsix` from
VS Code's **Extensions: Install from VSIX** command. The VSIX contains both
native servers and contributes them through the VS Code MCP provider; no
workspace MCP config, Rust toolchain, or global `PATH` change is required.

## Measured outcomes

The controlled productization baseline remains the 0.1.2 GitHub Copilot CLI
experiment with pinned
`claude-haiku-4.5`, three randomized fresh runs per arm, isolated Copilot homes,
hidden correctness checks, and one composite typed-session repair scenario.

| Arm | Success | Regression rate | Median exploration calls | Median output tokens | Median duration |
|---|---:|---:|---:|---:|---:|
| No memory | 0.0% | 100.0% | 11 | 3,502 | 72.060 s |
| Flat history | 0.0% | 100.0% | 11 | 3,034 | 61.273 s |
| MindLeak | 66.7% | 33.3% | 9 | 2,284 | 53.370 s |
| MindLeak + Lodestar | 100.0% | 0.0% | 10 | 2,275 | 50.877 s |

MindLeak reduced median exploration by 18.2%, crossing the declared 15% gate.
MindLeak + Lodestar passed all three runs with no measured regression. This is a
productization decision for that measured composite scenario, not a new 0.1.4
benchmark or a universal efficacy claim. The scenario was not rerun merely to
produce a larger release number.

Other validated results:

- JavaScript/TypeScript structural, hierarchy, and direct manifest truth sets:
  100% precision and recall on their declared deterministic fixtures.
- Passive terminal/Git capture: 28.651 ms p95 for the full 200-file/8 KiB local
  processing, MCP, and SQLite path, below the 50 ms gate.
- Signal benchmark: consequence/corroboration retains resolved failure evidence
  while same-session repetition earns no multiplier; 200-edge snapshot p95 was
  16.757 ms.
- Pinned VS Code 1.101 Extension Host smoke: both packaged servers connect,
  graph ingestion and both view refresh paths execute, and both databases open.
- Production PR telemetry now has a reproducible read-only harness that keeps
  GitHub delivery outcomes, Lodestar task/conformance history, MindLeak runtime
  health, and the controlled benchmark in separate evidence tiers. It reports
  incomplete attribution and missing checks rather than inferring success.

Full provenance and reproduction details are in [EVALUATION.md](EVALUATION.md).
The premium agent benchmark is not part of routine CI.

## Supported platforms

| Asset | Supported target |
|---|---|
| `windows-x64` | Windows x64 |
| `linux-x64` | Linux x64 with glibc |
| `macos-x64` | macOS Intel |
| `macos-arm64` | macOS Apple Silicon |

Every target publishes a native installer archive and a matching VSIX. Release
assets have SHA-256 checksums and signed GitHub build-provenance attestations.
The native binaries are not yet signed with Windows/macOS publisher identities,
so operating-system trust prompts may appear.

## Language and dependency matrix

| Capability | Supported inputs | Scope |
|---|---|---|
| Symbol extraction | Rust; JavaScript/TypeScript (`js`, `jsx`, `mjs`, `cjs`, `ts`, `tsx`); Python; C#; Go; Java; Kotlin | Deterministic heuristic definitions |
| In-file calls | Rust, JavaScript/TypeScript, Python, Go | Calls between symbols defined in one file |
| Cross-file imports | Rust; JavaScript/TypeScript | Rust `mod`/`use`; static JS/TS imports and `require` |
| Cross-file calls | JavaScript/TypeScript | Named imported calls |
| Type hierarchy | JavaScript/TypeScript | Simple named `extends`/`implements`, same-file or named import |
| Failure locations | Generic `path:line`; Python `File "path", line N` | Failed execution to artifact evidence |
| Direct dependencies | Cargo.toml, package.json, go.mod, requirements.txt / PEP 508 | Direct declarations only; fail closed when malformed |

Not supported in 0.1.4: transitive/lockfile dependency resolution; TypeScript
path aliases, re-exports, namespace/default cross-file calls, or expression-based
mixins; precise cross-file calls for languages other than JavaScript and
TypeScript; shared graph databases across unrelated repositories.

## Data and privacy

- Both servers are local, stdio-only, and open no network listener.
- The deterministic ingest/query path uses no model tokens or network calls.
- Terminal output retention is off by default; opt-in output is redacted and
  bounded before MCP submission.
- `backup_database` creates integrity-checked online SQLite backups for either
  plane. JSON graph and Markdown constitution exports are not backups.
- `RESET MINDLEAK` clears regenerable memory only. `RESET LODESTAR` is a separate
  explicit action for durable intent. The VS Code reset command never clears
  Lodestar.
- Databases and backups may contain source excerpts, commands, commit messages,
  terminal output, goals, and audit events. Protect them as workspace-sensitive.

See [DATA-LIFECYCLE.md](DATA-LIFECYCLE.md) for upgrade, rollback, retention,
backup, export, and reset procedures.

## Known limitations

- Coordination remains advisory. Claim scopes, overlap grades, rescue notices,
  and deadlock reports inform agents and reviewers; they do not lock files or
  silently transfer ownership.
- The memory preflight now rides on scoped claims, but post-change adoption has
  not yet been measured across an independent cohort. Earlier telemetry showed
  agents routinely wrote before reading memory; 0.1.4 changes the mechanism, not
  the evidence retroactively.
- Structural and symbol extraction is deterministic and deliberately heuristic.
  JavaScript/TypeScript has the richest cross-file model; Rust imports are
  supported, while other languages remain primarily file-local.
- Passive terminal capture requires VS Code shell integration; unsupported shells
  report a visible degraded status instead of inferring commands from text.
- The optional consolidation and embedding features require an external
  OpenAI-compatible endpoint and fail cleanly when it is unavailable.
- Autonomous consolidation is off by default. When explicitly enabled it may
  call the configured model during idle, uses a file-backed database, and emits
  maintenance telemetry for completed attempts. Manual and idle calls share a
  persisted rate limit; bounded shutdown may terminate an in-flight HTTP attempt
  before its final telemetry event.
- Unit Test MCP does not route this Rust workspace (`INVALID_ROOT_DIR`) and may
  report zero Vitest counts despite executing the suite; repository hooks and CI
  remain the authoritative Rust test/count and coverage gates.
- The measured agent result has one model, one runner, one engineered composite
  task, and three repetitions per arm. Do not generalize the percentages beyond
  that scope.
- Independent-developer recruitment remains open; no external-adoption,
  retention, or causal productivity claim is made by these release notes.
