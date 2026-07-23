# MindLeak 0.1.0 Preview Release Notes

MindLeak 0.1.0 is the first productized preview of the local Temporal Context
Graph Engine and its separate Lodestar Intent Plane. The release includes both
native MCP servers, targeted VS Code packages, passive evidence capture, local
data lifecycle controls, and reproducible evaluation artifacts.

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
Use `--agent <stable-id>` to set the attribution/task identity.

For the editor experience, install the matching platform-targeted `.vsix` from
VS Code's **Extensions: Install from VSIX** command. The VSIX contains both
native servers; no Rust toolchain or global `PATH` change is required.

## Measured outcomes

The product gate used GitHub Copilot CLI 1.0.63 with pinned
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
productization decision for the measured composite scenario, not a universal
efficacy claim. Broader repositories, models, and two-agent duplicate-work
scenarios remain to be replicated.

Other validated results:

- JavaScript/TypeScript structural, hierarchy, and direct manifest truth sets:
  100% precision and recall on their declared deterministic fixtures.
- Passive terminal/Git capture: 28.651 ms p95 for the full 200-file/8 KiB local
  processing, MCP, and SQLite path, below the 50 ms gate.
- Signal benchmark: consequence/corroboration retains resolved failure evidence
  while same-session repetition earns no multiplier; 200-edge snapshot p95 was
  16.757 ms.
- Pinned VS Code 1.93.1 Extension Host smoke: both packaged servers connect,
  graph ingestion and both view refresh paths execute, and both databases open.

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
| Cross-file imports/calls | JavaScript/TypeScript | Static named imports and `require`; named imported calls |
| Type hierarchy | JavaScript/TypeScript | Simple named `extends`/`implements`, same-file or named import |
| Failure locations | Generic `path:line`; Python `File "path", line N` | Failed execution to artifact evidence |
| Direct dependencies | Cargo.toml, package.json, go.mod, requirements.txt / PEP 508 | Direct declarations only; fail closed when malformed |

Not supported in 0.1.0: transitive/lockfile dependency resolution; TypeScript
path aliases, re-exports, namespace/default cross-file calls, or expression-based
mixins; precise cross-file structure for languages other than JavaScript and
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

- Passive terminal capture requires VS Code shell integration; unsupported shells
  report a visible degraded status instead of inferring commands from text.
- The optional consolidation and embedding features require an external
  OpenAI-compatible endpoint and fail cleanly when it is unavailable.
- Autonomous consolidation is off by default. When explicitly enabled it may
  call the configured model during idle, uses a file-backed database, and emits
  maintenance telemetry for completed attempts. Manual and idle calls share a
  persisted rate limit; bounded shutdown may terminate an in-flight HTTP attempt
  before its final telemetry event.
- Unit Test MCP currently reports successful Rust/custom runs with zero test
  counts; compile failures still surface, while CI remains the authoritative
  count/coverage gate.
- The measured agent result has one model, one runner, one engineered composite
  task, and three repetitions per arm. Do not generalize the percentages beyond
  that scope.
