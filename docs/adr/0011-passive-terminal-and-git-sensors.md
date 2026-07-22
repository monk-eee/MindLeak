# ADR-0011: Passive terminal and Git evidence sensors

- Status: Accepted
- Date: 2026-07-22
- Deciders: MindLeak maintainers
- Related: [ADR-0003](0003-agent-attribution-as-observed-edges.md),
  [ADR-0009](0009-evidence-backed-conformance.md),
  [ADR-0010](0010-observability-and-resilience.md)

## Context

MindLeak can ingest executions and commits, but clients must call those tools
explicitly. That makes evidence optional and lets a coding agent omit failures or
changes. Editor focus and file saves are passive, but they prove observation and
current structure, not command outcomes or mutation provenance.

VS Code exposes stable terminal shell-execution start/end events only from 1.93.
Those events provide command confidence/trust, working directory, bounded output
streaming, and exit code when shell integration is active. Older versions cannot
capture this reliably without scraping terminal text or intercepting shells.
VS Code's built-in Git extension separately exposes repository commit events,
HEAD state, commit metadata, and changed paths.

## Decision

Raise the extension engine and API type floor to VS Code 1.93 and add two local,
deterministic sensors.

### Terminal sensor

- Subscribe to `onDidStartTerminalShellExecution` and
  `onDidEndTerminalShellExecution`.
- Start reading output immediately, but retain it only when output capture is
  explicitly enabled. Strip ANSI control sequences, redact common secret shapes,
  and cap retained text before MCP submission.
- Capture only medium/high-confidence command lines. Suppress the full record for
  commands likely to carry secrets (`read`, credential/token/password commands,
  masked input flows); never capture environment variables or shell input.
- Subscribe once to VS Code's cross-platform workspace file watcher and collect
  normalized create/change/delete paths while each execution is active. Apply
  configured workspace-relative exclusions before submission. Submit that
  bounded mutation set to `ingest_execution` with command, exit code, cwd,
  output, and timestamp.
- If shell integration is absent or an exit code is unknown, expose degraded
  capture health. Never invent a successful execution.

### Git sensor

- Use the built-in Git extension API, not `.git` filesystem watching or polling.
- On `Repository.onDidCommit`, read the new HEAD commit and the parent-to-HEAD
  changed paths, normalize them relative to the workspace, and submit
  `ingest_commit` with SHA, message, changed files, and commit timestamp.
- De-duplicate the last ingested SHA per repository for the extension session.

### Configuration and health

- Execution metadata capture defaults on; terminal output capture defaults off.
- Output size and workspace exclusions are bounded settings.
- The graph view and output channel report `connected`, `capture active`, or a
  concrete degraded reason. Capture failures never stop editing or MCP queries.

## Consequences

- Execution, failure, changed-file, and commit evidence no longer depends on an
  agent volunteering ingestion calls when supported integration is active.
- The extension no longer supports VS Code 1.85-1.92. This is an intentional
  compatibility cost for a stable, testable API instead of terminal scraping.
- Shells without VS Code shell integration remain visible as degraded; terminal
  metadata cannot be reconstructed after the fact.
- Concurrent terminal executions can both observe the same workspace mutation;
  the sensor records evidence rather than claiming process-level causality.
- All capture remains local and deterministic. No LLM or network service is
  introduced on the write path.

## Rejected alternative

Before/after `git status --porcelain=v1 -z` snapshots were implemented first.
On the Windows proof workspace, a single snapshot measured 71.7 ms p95 over 40
runs, so two snapshots cannot meet the 50 ms capture-overhead gate. Narrower
status/diff modes remained above 65 ms p95. The file watcher keeps command-event
handling in-process and leaves Git process work to the existing Git extension.
