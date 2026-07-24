# MindLeak Walkthrough — a normal day

This is what using MindLeak actually looks like: the VS Code panels you watch,
and four end-to-end scenarios taken from a normal coding session. Each scenario
shows both the **agent tool calls** and the **extension buttons** that do the
same thing, so it works whether you drive MindLeak from chat or from the sidebar.

New here? Do the **[Quickstart](QUICKSTART.md)** first (install → first prompt),
then come back. For the full panel reference and settings, see the
**[extension guide](../editors/vscode/README.md)**; for every tool and its
arguments, see **[USAGE.md](USAGE.md)**.

---

## The panels you watch

Click the **MindLeak icon in the activity bar** to open four views. You mostly
*watch* these while your agent works; a few actions (accept an ADR, complete a
task with evidence, pause/resume) are yours to click.

![MindLeak's four views open in the VS Code sidebar next to an editor](../editors/vscode/media/screenshots/overview.png)

| View | Type | You use it to… |
|---|---|---|
| **Context Graph** | live graph | See the decay-weighted subgraph — what connects to what, right now. |
| **Intent Board** | task tree | Watch who owns which task; complete with evidence, pause, resume, answer, retire. |
| **Telemetry** | live readout | Verify what actually ran — call counts, error rates, latency, recent events. |
| **Design Board** | review tree | Turn ADRs into work: accept / reject a proposal, promote it under a goal. |

> **Claiming is an agent action, not a button.** Agents claim and complete tasks
> through the MCP tools; the Intent Board shows you the resulting ownership and
> lets you intervene (pause, answer a question, complete with evidence).

---

## Scenario 1 — Look before you leap

*You are about to change session validation in `src/auth.ts`.*

Ask the graph what the change could break **before** editing:

```text
get_impact_radius(target_artifact = "artifact:src/auth.ts")
```

It returns the blast radius — dependent symbols, importing files, prior failing
runs, and related decisions, grouped rather than flattened. Pull the reasoning in
by meaning, then walk the connections:

```text
recall(query = "why session validation is strict", limit = 5)
graph_multi_hop_query(seed_entity = "artifact:src/auth.ts", max_depth = 2)
```

Make the edit, then **write back what happened** so the next session inherits it:

```text
ingest_execution(command = "npm test", exit_code = 0, changed_files = ["src/auth.ts"])
record_architectural_decision(
  decision_text = "Session tokens now expire after 30m; refresh on activity.",
  related_nodes = ["artifact:src/auth.ts"],
)
```

Decisions decay far slower than raw run noise, so the *why* outlives the events
that prompted it. In the sidebar, the **Context Graph** now shows `src/auth.ts`
wired to the new decision node, and **Telemetry** confirms the calls landed.

---

## Scenario 2 — Turn an ADR into work (Design Board)

*You wrote up a decision and want it to become claimable tasks — safely.*

1. Add `docs/adr/0031-rotate-session-tokens.md` with a front-matter `Status:
   Proposed` and an H1 title.
2. In VS Code, open the **Design Board** and click **Sync ADRs**
   (`mindleak.design.sync`). The ADR appears as a **Proposed** row. Nothing is
   inferred from the Markdown body — only the path, title, and status.
3. A **human reviewer** (an identity different from the proposing agent) clicks
   **Accept**. Acceptance is the decision *only* — it creates no tasks and runs no
   code conformance.
4. On the now-**pending** row, click **Promote** and pick an active objective
   goal. Lodestar decomposes the reviewed design into tasks under that goal,
   registers any mandated constraints into the constitution, and records
   design→goal / design→task provenance — **exactly once** (promotion is
   idempotent, so a retry returns the same plan).

The new tasks show up on the **Intent Board**, ready to claim. The same flow from
an agent:

```text
reconcile_designs()                              # or register_design(...)
accept_design(design_id, reviewer = "you")       # human-in-the-loop; not self-decided
promote_design(design_id, objective_goal_id)     # idempotent materialisation
```

No agent may accept its own design, and ADR discovery never auto-accepts or
auto-promotes.

---

## Scenario 3 — Two agents split a goal without clobbering

*Two agents work the same goal in parallel and must not collide.*

Write the goal, break it down, and let each agent pull work:

```text
define_goal(kind = "feature", title = "Token rotation", statement = "...")
decompose_goal(goal_id)        # produces claimable tasks
```

Each agent runs its own loop against the one shared `.lodestar/spec.db`:

```text
next_task()                          # what should I pick up?
claim_task(task_id, agent = "agent-a")   # compare-and-swap — no two agents win the same task
renew_lease(task_id, agent = "agent-a")  # keep the claim alive while working
complete_task(task_id, agent = "agent-a", evidence)  # owner-guarded; runs conformance
```

`claim_task` is atomic: if `agent-b` races for the same task, exactly one wins.
When two tasks touch **the same file**, don't hand them out concurrently —
serialize them so only an *aligned* completion opens the next:

```text
first  = create_task(goal_id, "Edit Router", acceptance = "...")
second = create_task(goal_id, "Edit helper", acceptance = "...", blocked_by = first.id)
```

Watch it on the **Intent Board**: each task shows its owner and lease; hover a
`claimed` task to **Pause**/**Complete With Evidence**, answer a `needs_input`
task, or **Retire** a stale one. A completion that drifts or violates the
constitution is blocked and leaves the successor blocked too.

---

## Scenario 4 — A normal edit/test/commit loop (passive capture)

*You just work. The extension records the evidence — no manual ingestion.*

With the extension installed, ordinary actions feed the graph:

- **Save a file** → its symbols and imports are ingested (`autoIngestOnSave`).
- **Run tests in the integrated terminal** → the command, exit code, and changed
  files become an execution node; a failing run adds `failed_on` edges parsed
  from the stack trace. (Requires VS Code **shell integration**; the Context
  Graph status shows a concrete reason if capture is degraded.)
- **Commit** → the built-in Git event records commit evidence and extracts
  `DECISION:` / `HACK:` / `WHY:` markers from the message.

The **Telemetry** pane shows these captures arriving live. Privacy stays default:
command metadata is on, but terminal **output** storage is off unless you opt in
(then it is redacted and capped). Next session, your agent just asks:

```text
recall(query = "what failed last time I touched auth", limit = 5)
```

…and picks up where you left off, with the stale noise already decayed away.

---

## Where to go next

- **[USAGE.md](USAGE.md)** — every tool, the full memory loop, and the config
  reference.
- **[extension guide](../editors/vscode/README.md)** — all panels, controls, and
  settings.
- **[DATA-LIFECYCLE.md](DATA-LIFECYCLE.md)** — backup, export, reset, retention,
  and privacy.
