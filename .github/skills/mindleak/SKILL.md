---
name: mindleak
description: "Set up, verify, use, and troubleshoot the MindLeak and Lodestar MCP servers. Use when installing or configuring MindLeak, connecting an MCP client, opening a shared session, checking code impact or agent overlap, recalling repository context, coordinating or claiming tasks, renewing leases, recording durable knowledge, completing work with evidence, or diagnosing storage and connection problems."
argument-hint: "setup, verify, work <task>, status, or troubleshoot"
---

# MindLeak

Use MindLeak's decaying Memory Plane for repository evidence and Lodestar's
durable Intent Plane for goals, coordination, governance, and completion proof.
Do not query their SQLite files directly.

## Route the Request

1. If either MCP server is unavailable, the user asks to install/configure it,
   or registration is suspect, follow [setup](./references/setup.md).
2. If both planes are available, open one shared session and verify storage as
   described below.
3. For implementation, investigation, planning, coordination, or handoff, follow
   [the working loop](./references/workflow.md).
4. For a simple status question, use the smallest read-only tools that answer it;
   do not claim work or write knowledge merely because the tools are available.

## Session Invariant

- Mint one random 128-bit lowercase hexadecimal `session_id` per chat/session.
- Call `open_session` on both planes with that exact token. Reuse it for every
  identity-bearing call; the human-readable agent label is not an identity.
- Declare `branch`, `head_sha`, `base`, and `dirty` when they are known. The
  servers record declarations and never inspect Git themselves.
- Call `storage_status` on both planes after setup. Their `repository_id` values
  must match and their database paths must share one repository directory.
- Never print or persist credentials in a session declaration or tool evidence.

## Default Work Loop

1. Identify the concrete workspace-relative paths and symbol ids in scope.
2. Run both preflights before editing: Lodestar's live task-overlap query and
   MindLeak's decay-active `check_overlap`. Treat unknown ids as unknown, not
   clear, and a quiet impact result as "nothing recorded", not "no impact".
3. Ask Lodestar `advise` what governs the intended artifacts. Respect `review`,
   `block`, and `needs_human`; advice is evidence, not a lock bypass.
4. If work has a Lodestar task, claim it with the same paths/symbols. Renew its
   lease after each substantial step and before long validation.
5. Ground the change with deterministic evidence (`evidence_for`, impact, graph
   traversal, task scope). Semantic `recall` is optional supporting evidence,
   never the sole basis for a risky edit.
6. Make and validate the smallest principled change.
7. Write back only useful facts: changed files, meaningful executions, commits,
   and genuine architectural decisions. Never ingest secrets or noisy logs.
8. Complete a claimed task through Lodestar with validation and conformance
   evidence. Report failures even when they remain unfixed.

## Operating Rules

- MindLeak and Lodestar are local and fully useful without an LLM, embedding
  model, account, or network service.
- Keep ingestion deterministic. Optional model calls belong only to explicit
  consolidation or semantic-recall paths.
- Coordinate on overlap; do not interpret advisory checks as filesystem locks.
- Prefer the tool schema exposed by the running server over copied argument
  shapes. Product versions can add fields while preserving the workflow.
- If a tool is missing or a server identity differs from the expected build,
  stop the workflow and use the setup/troubleshooting reference.
