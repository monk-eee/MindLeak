# Security Policy

## Reporting a vulnerability

Please **do not** open a public issue for security problems. Report them
privately via [GitHub Security Advisories](https://github.com/monk-eee/MindLeak/security/advisories/new)
(or email the maintainer if you have that contact). Include what you observed,
how to reproduce it, and the potential impact. You will get an acknowledgement,
and a fix or mitigation plan.

## Threat model (what MindLeak is)

MindLeak is a **local, single-user developer tool**. The graph database lives on
your machine (`.mindleak/graph.db`) and the MCP server speaks JSON-RPC over
**stdio only** — there is no network listener by default.

- **The deterministic path is local.** Ingestion, graph queries, and SQLite
  storage never call a model or network endpoint. Optional consolidation and
  embeddings send selected text to the user-configured OpenAI-compatible URL.
  The default is local Ollama; a hosted URL can send source/log/intent content
  to that provider and must be governed accordingly.
- **Ingest tools are unauthenticated.** Any process with stdio access to
  `mindleak-mcp` can write nodes and edges. This is acceptable for local use;
  **do not expose the server over a network without adding an auth layer.**
- **The graph may contain source excerpts, commit messages, and command output.**
  Treat `.mindleak/graph.db` with the same sensitivity as your workspace. It is
  gitignored and regenerable — do not commit it.
- **Intent and backups are sensitive too.** `.lodestar/spec.db` and either
  plane's backups may contain goals, task ownership, evidence, source excerpts,
  and audit records. Store them with workspace-equivalent access controls.
- **Terminal output retention is opt-in.** Passive command metadata does not
  retain output by default. When enabled, output is stripped of terminal control
  sequences, redacted for common credential forms, and capped before MCP
  submission. Secret-bearing command shapes are suppressed entirely; this is
  defense in depth, not a guarantee that arbitrary output contains no secrets.

## Supported versions

MindLeak is pre-1.0; only the latest `main` is supported. Fixes land on `main`.

## Handling secrets

MindLeak never asks for credentials. If ingestion captures a secret, first
rotate/revoke it, then call `reset_database(confirm="RESET MINDLEAK")` to erase
graph, embeddings, and telemetry, or stop all clients and delete the database.
Remove any backups containing the value and report the capture pattern so
redaction can improve. `prune_graph` alone is not a secure-erasure mechanism.
