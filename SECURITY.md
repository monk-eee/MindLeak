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

- **Nothing leaves the machine.** Deterministic ingestion is fully local. The
  optional Ollama consolidation layer talks to a local model endpoint; no code
  or telemetry is sent to a third party.
- **Ingest tools are unauthenticated.** Any process with stdio access to
  `mindleak-mcp` can write nodes and edges. This is acceptable for local use;
  **do not expose the server over a network without adding an auth layer.**
- **The graph may contain source excerpts, commit messages, and command output.**
  Treat `.mindleak/graph.db` with the same sensitivity as your workspace. It is
  gitignored and regenerable — do not commit it.

## Supported versions

MindLeak is pre-1.0; only the latest `main` is supported. Fixes land on `main`.

## Handling secrets

MindLeak never asks for or stores credentials. If you find that ingestion has
captured a secret from terminal output into the graph, run `prune_graph` or
delete `.mindleak/graph.db` (it is regenerable), and report the capture pattern
so we can improve redaction.
