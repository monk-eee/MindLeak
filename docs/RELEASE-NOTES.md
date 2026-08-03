# MindLeak 0.1.5 Release Notes

MindLeak 0.1.5 makes repository learning easier to find, preserve, and hand
between agents. It adds semantic search and source-aware agent-memory promotion
to Lodestar, ships the canonical MindLeak workflow skill in native installer
archives, exposes completed ADRs in VS Code, and improves deterministic source
ingestion without adding model calls to the write path.

## Highlights

- **Knowledge is searched where agents already read it.** `active_knowledge`
  accepts an optional `query` and ranks learned lessons by meaning through the
  configured OpenAI-compatible embeddings endpoint. No new MCP verb is needed.
  If the endpoint is unavailable, the call falls back to literal substring
  matching and reports that in `match_mode` (ADR-0080).
- **The semantic index warms itself and reports partial coverage.** A search
  backfills missing active-knowledge vectors in one bounded batch. Ranking only
  claims the lessons that were actually embedded; when the index is still
  partial, `ranked_by_meaning` and `match_mode_note` say how much of the result
  is semantic and which tail remains in weight order. Malformed vectors are
  refused rather than silently reshaped.
- **Reusable agent memory can leave one client's private notes.**
  `record_knowledge` accepts an attributed `/memories/repo/...` or
  `/memories/session/...` `source_ref`. Exact repeats reconfirm one lesson;
  edited source text creates a successor; removing the final source can retire
  the old lesson without deleting its history. Global user preferences remain
  private by default (ADR-0081).
- **Native release installs include the MindLeak agent skill.** The installer
  places one canonical project skill under `.github/skills/mindleak`, covering
  setup, shared session identity, overlap and impact checks, claim/lease
  discipline, evidence-backed completion, and troubleshooting. Managed files
  upgrade automatically while locally edited skill files are preserved and
  reported.
- **Completed ADRs are visible without becoming work again.** The VS Code
  Design Board remains an actionable queue by default. Its empty state and
  archive control open the durable live ADR ledger in one bounded read without
  hydrating every completed promotion.
- **Deterministic ingestion is more faithful and cheaper.** Rust extraction now
  preserves restricted and `const` functions, masks comments and literals
  before finding calls, and keeps same-named methods in separate `impl` blocks
  distinct. Constant definition, call-site, and error-location regexes compile
  once instead of once per file or command.

## Compatibility and migration

- Existing unsourced `record_knowledge` calls remain valid. A sourced write
  requires a registered session and evidence that reaches artifact/symbol
  nodes, a goal, or a known task.
- Existing `active_knowledge` `node` and `contains` filters keep their contracts.
  `query` is additive; callers using it should inspect `match_mode` and, when
  present, `ranked_by_meaning` rather than assuming every row was embedded.
- Rust structural snapshots from the previous extractor are marked stale and
  refresh deterministically under extractor version 2.
- The native archive contains three additional skill files consumed by
  `install.mjs`. Existing installations can rerun the installer; locally edited
  managed skill files are preserved.
- VS Code 1.101 or newer, Node.js 20 or newer for the installer, and Rust 1.75
  for source builds remain unchanged. No third-party dependency was upgraded as
  part of the version bump.

## Reliability and correctness

- `effective_weight` uses saturating timestamp subtraction, so an extreme or
  corrupt timestamp decays safely instead of overflowing a graph query.
- `lodestar_stats` applies the same retirement and decay predicate as
  `active_knowledge`; retired lessons no longer remain in the active count.
- The Windows embeddings test server consumes complete HTTP headers and bodies
  before replying, removing a fixture reset race from Windows CI.
- Public documentation now describes MindLeak as local context infrastructure:
  a decaying Memory Plane plus Lodestar's durable intent, coordination, and
  proof. Optional model defaults are local, while deliberately configured
  hosted OpenAI-compatible endpoints remain supported with explicit data
  boundaries.

## Install

Download the archive matching the target platform, verify it against
`SHA256SUMS` and its GitHub artifact attestation, extract it, then run this once
from the workspace:

```text
node /path/to/extracted/install.mjs
```

The installer smoke-tests and registers both local stdio MCP servers, installs
the project-scoped MindLeak skill, preserves unrelated MCP registrations, and
adds local-state privacy rules to `.gitignore`.

For VS Code, install the matching platform-targeted `.vsix` through
**Extensions: Install from VSIX**. The VSIX contains both native servers and
contributes them through VS Code's MCP provider; no Rust toolchain or workspace
MCP configuration is required.

## Supported platforms

| Asset | Supported target |
|---|---|
| `windows-x64` | Windows x64 |
| `linux-x64` | Linux x64 with glibc |
| `macos-x64` | macOS Intel |
| `macos-arm64` | macOS Apple Silicon |

Every target publishes a native installer archive and a matching VSIX. Release
assets have SHA-256 checksums and GitHub build-provenance attestations. Native
binaries are not publisher-signed, so operating-system trust prompts may appear.

## Language and dependency matrix

The supported language surface is unchanged in 0.1.5. Rust extraction is more
accurate within that existing surface.

| Capability | Supported inputs | Scope |
|---|---|---|
| Symbol extraction | Rust; JavaScript/TypeScript (`js`, `jsx`, `mjs`, `cjs`, `ts`, `tsx`); Python; C#; Go; Java; Kotlin | Deterministic heuristic definitions |
| In-file calls | Rust, JavaScript/TypeScript, Python, Go | Calls between symbols defined in one file |
| Cross-file imports | Rust; JavaScript/TypeScript | Rust `mod`/`use`; static JS/TS imports and `require` |
| Cross-file calls | JavaScript/TypeScript | Named imported calls |
| Type hierarchy | JavaScript/TypeScript | Simple named `extends`/`implements`, same-file or named import |
| Failure locations | Generic `path:line`; Python `File "path", line N` | Failed execution to artifact evidence |
| Direct dependencies | Cargo.toml, package.json, go.mod, requirements.txt / PEP 508 | Direct declarations only; fail closed when malformed |

## Data and privacy

- Both MCP servers remain local, stdio-only, and open no network listener.
- Deterministic ingestion and ordinary graph queries use no model tokens.
- Embedding and consolidation are optional. Defaults target local endpoints;
  data leaves the machine only when an operator configures a hosted endpoint.
- Sourced knowledge stores the portable logical `source_ref` supplied by the
  client. Lodestar never reads VS Code's private workspace-storage directory.
- Global `/memories/*.md` preferences and scratch notes are not promoted into a
  repository ledger by default.
- Databases and backups can contain source excerpts, commands, commit messages,
  goals, lessons, and audit events. Protect them as workspace-sensitive.

## Measured outcomes

No new agent-productivity benchmark was run solely for this release number. The
controlled 0.1.2 productization experiment remains the latest causal benchmark:
MindLeak reduced median exploration calls by 18.2% in its pinned composite task,
and MindLeak plus Lodestar passed all three runs. That result is evidence for
that model, runner, scenario, and sample only; it is not a universal efficacy
claim. Full provenance remains in [EVALUATION.md](EVALUATION.md).

## Known limitations

- Coordination remains advisory. Claims, overlap grades, and rescue notices do
  not lock files or transfer ownership silently.
- Semantic knowledge search depends on the configured embeddings endpoint. Its
  explicit substring fallback cannot find differently worded lessons, and the
  first search may report a partially warmed index.
- Source-aware memory promotion is deliberate rather than a watcher over client
  storage. Agents decide which atomic, non-private lessons are reusable.
- Structural extraction remains deterministic and heuristic. JavaScript and
  TypeScript have the richest cross-file model; other languages remain more
  file-local, and TypeScript path aliases/re-exports are not resolved.
- Transitive lockfile dependency resolution and shared graph databases across
  unrelated repositories remain unsupported.
- Passive terminal capture still requires VS Code shell integration.
- The measured agent result remains one model, one runner, one engineered
  composite task, and three repetitions per arm. Independent-developer
  adoption and retention evidence remains open.

The complete itemized change record is in [CHANGELOG.md](../CHANGELOG.md).
