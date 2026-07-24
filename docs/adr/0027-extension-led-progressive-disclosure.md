# ADR-0027: Extension-led progressive disclosure over MCP primitives

- Status: Accepted
- Date: 2026-07-24
- Deciders: MindLeak maintainers
- Related: [ADR-0004](0004-intent-plane-spec-brain.md) (two-plane boundary),
  [ADR-0010](0010-observability-and-resilience.md) (visible degradation),
  [ADR-0016](0016-platform-packaging-and-registration.md) (self-contained
  distribution), [ADR-0023](0023-design-board-accept-bridge.md) (human design
  workflow)

## Context

MindLeak is technically installable: targeted VSIX packages contain both native
servers, the archive installer registers both planes, and the Extension Host
smoke proves activation, connectivity, ingestion, and view refresh. Installation
does not yet guarantee comprehension or first value.

The product exposes many correct, granular MCP tools across memory, graph,
coordination, design, knowledge, and conformance. That surface is valuable to
headless agents but asks a new user to understand node ids, agent identity,
claims, leases, evidence windows, goal bindings, and optional models before the
workflow feels coherent. The extension currently exposes capabilities as
separate panes and commands; it does not yet guide an unfamiliar developer from
"installed" to "my agent resumed useful work".

Combining both servers or replacing the granular tools with one stateful
"do everything" tool would simplify the demo by weakening the architecture:
portable MCP clients would become second-class, failures would be harder to
localize, and the extension could become a second source of truth.

## Decision

Keep the two MCP servers as the authoritative, portable, granular engines. Make
the VS Code extension the optional **product shell** that progressively reveals
those primitives through user-intent workflows.

### One derived readiness model

The extension derives workspace readiness from existing server responses and
configuration; it does not persist a parallel product state machine:

| State | Meaning | Primary action |
|---|---|---|
| `disconnected` | one or both packaged servers cannot initialize | show exact server/path remediation |
| `ready_empty` | both planes are healthy but no useful workspace evidence exists | ingest the active file and explain passive capture |
| `observing` | memory is collecting evidence; no coordinated work is active | open graph/recall and optionally define an objective |
| `coordinating` | actionable Lodestar tasks or designs exist | open the relevant board and next action |
| `degraded_optional` | deterministic core works but embeddings/model/shell integration is unavailable | name the optional capability and remediation without blocking core use |

Readiness is a projection over initialize metadata, health, graph statistics,
board/design-board state, and sensor capability. Dismissed teaching UI may use
VS Code workspace state, but authority remains in the two databases.

### A five-minute first-value path

On first activation in a workspace, the extension guides rather than markets:

1. verify both packaged servers and show exact build identity;
2. register one client session and confirm its stable identity on both planes;
3. ingest the active file and wait for one passive evidence event;
4. display the first useful graph/impact or recall result;
5. offer coordination only when the user has an objective, task, or proposed
   design; and
6. label local-model and shell-integration enhancements as optional.

The user can skip or dismiss guidance. No model, account, network service, or
constitution is required to reach first value.

### Workflow-oriented surfaces

The extension groups commands and views by developer intent rather than by MCP
module:

- **Remember and understand:** graph, recall, impact, working set.
- **Coordinate work:** next/claim/release/pause/resume/retire and task evidence.
- **Review intent:** Constitution, Design Board, accept/reject/promote.
- **Operate locally:** health, backup, export, reset, optional-model diagnostics.

Client-side orchestration calls existing MCP tools and preserves their typed
errors. It must not reproduce graph, task, conformance, or design rules in UI
state. Pure display/eligibility derivations remain unit-tested outside the VS Code
API.

### Headless clients remain first-class

README/USAGE recipes describe the same golden paths as ordered MCP calls.
Copilot, Claude, Cursor, and CLI users can operate both planes without the
extension. Tool descriptions remain sufficient for an agent to recover from a
partial workflow; the extension is convenience and visibility, not authority.

## Acceptance gates

- A clean-workspace Extension Host scenario reaches a non-empty graph and both
  healthy planes without a Rust toolchain or local model.
- An unfamiliar pilot user can install and reach one useful graph/impact result
  in five minutes without editing JSON manually.
- Every degraded state names the failed optional/core capability and a concrete
  remediation; optional failures never present the whole product as offline.
- Design review, task allocation, completion evidence, and retirement are
  executable from the extension while remaining available through MCP.
- No extension-owned copy of goals, tasks, graph facts, or conformance verdicts
  survives independently of the authoritative stores.

## Rejected alternatives

- **Merge MindLeak and Lodestar into one server:** erases their inverted
  durability/write-path invariants and increases failure coupling.
- **Replace primitives with a single autonomous workflow tool:** hides authority
  transitions, makes retries ambiguous, and reduces client portability.
- **Make the extension mandatory:** excludes other MCP clients and headless use.
- **Require an embedding or chat model during onboarding:** turns optional
  enrichment into a core dependency.
- **Build a marketing landing page inside VS Code:** does not reduce operational
  uncertainty or help a user reach first value.

## Consequences

- Product work prioritizes first-run readiness, Design Board completion, task
  controls, and visible remediation before adding more conceptual primitives.
- Extension Host tests grow from connectivity smoke to one realistic first-value
  workflow and key board actions.
- GitHub Releases remain the source of packaged binaries per ADR-0016; publishing
  to an extension marketplace is a distribution-channel decision after the
  product shell is complete, not a new source of truth.
- The MCP surface remains broad but composable; users see complexity gradually,
  while advanced agents retain precise control.
