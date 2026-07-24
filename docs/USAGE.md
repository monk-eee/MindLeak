# Using MindLeak

MindLeak gives a coding agent **durable, decaying memory of the work** — what was
run, what changed, what failed, what was decided — and lets it query that memory
before it acts. This guide covers how the tools are used in practice and how to
configure the servers.

New here? Start with **[QUICKSTART.md](QUICKSTART.md)**. Want worked examples
instead of a reference? See **[WALKTHROUGH.md](WALKTHROUGH.md)** for four
end-to-end scenarios with the VS Code panels shown.

---

## Mental model: two planes

| Plane | Server | Question it answers | Lifetime |
|---|---|---|---|
| **Memory** | `mindleak-mcp` | "What happened, how does the code connect, what breaks if I change this?" | **Decays** — stale context fades on an exponential half-life |
| **Intent** | `lodestar-mcp` | "What are we trying to do, who owns which task, does this change conform?" | **Durable** — goals and coordination persist |

You can run just the memory plane. Add the intent plane when multiple agents (or
worktrees) need to coordinate without diluting shared intent.

Everything on the **write path is deterministic** (pattern matching, zero LLM
tokens). Optional local models only run asynchronously, off the hot path.

## First value without VS Code

The extension's Workspace view is a projection over these same MCP primitives;
headless clients remain first-class. A new workspace can reach useful context
without a model:

```text
# 1. Initialize both stdio servers and retain each initialize.serverInfo build.
#    Give both processes the same explicit agent id for one coordinated session.
MINDLEAK_AGENT_ID = "client-a1b2c3d4"
LODESTAR_AGENT_ID = "client-a1b2c3d4"
# 2. Create the first deterministic structural context.
ingest_file(path = "src/main.ts", content = "<current file content>")

# 3. Confirm and inspect that context.
graph_stats()
graph_snapshot(seed = "artifact:src/main.ts", limit = 60)

# 4. Surface coordination only when durable work exists.
board(include_terminal = false)
design_board()
```

If a server cannot initialize, report its command/path and initialize error. If
terminal, Git, embeddings, or consolidation are unavailable, name that optional
capability while continuing to use deterministic ingestion and graph queries.
`recall` is optional; `graph_snapshot`, FTS-seeded traversal, and impact queries
do not require an embedding model.

---

## The memory loop

A productive agent weaves these calls into its normal work. None of them require
a model.

### Before you edit — look before you leap

```text
get_impact_radius(target_artifact = "artifact:src/auth.ts")
```

Returns the blast radius of a change: dependent symbols, importing files, prior
failing executions, and related decisions — grouped, not flattened. Ask this
*before* touching a file so the agent knows what it might break.

### Pull in the right context

```text
recall(query = "why is login failing", limit = 5)      # semantic, needs the embedding index
graph_multi_hop_query(seed_entity = "<node id or phrase>", max_depth = 2)
```

The recommended pattern is **recall → traverse**: `recall` finds the best entry
nodes by *meaning* (embeddings), then you seed those node ids into
`graph_multi_hop_query`, which walks the decay-weighted graph to assemble the
connected context. Similarity finds the door; the graph walks the house.

`recall` needs vectors first — run `index` once (and after big changes) to embed
nodes that lack a current vector. Without an embedding model, skip `recall` and
seed `graph_multi_hop_query` with a search phrase directly (it falls back to
full-text search).

### After something happens — record it (deterministically)

```text
ingest_execution(command, exit_code, output, changed_files)   # a terminal run / test
ingest_commit(message, sha, changed_files)                    # a git commit
ingest_file(path, content)                                    # a file's symbols + imports
```

- `ingest_execution` creates an execution node, `modified` edges to changed
  files, and `failed_on` edges parsed from stack traces in the output.
- `ingest_commit` creates an intent node and extracts `DECISION:` / `HACK:` /
  `WHY:` rationale markers from the message.
- `ingest_file` replaces the file's structural snapshot: its symbols
  (`contains`) and, for JS/TS, static `import`/`require` edges and named
  cross-file `calls`.

> In the VS Code extension these fire passively: focus boosts a node, save
> ingests symbols, shell-integrated terminal events ingest command outcomes, and
> built-in Git events ingest commits. Unsupported capture paths report a visible
> degraded status; agents can still call the tools directly.

### Record the *why*

```text
record_architectural_decision(decision_text, related_nodes = ["artifact:src/auth.ts"])
```

Persists a decision/tradeoff as an intent node linked to what it affects.
Decisions decay far slower than raw execution noise, so the reasoning outlives
the events that prompted it.

### Confirm what actually ran — the record

```text
telemetry_snapshot(limit = 20)
```

Returns a durable audit trail of every tool call — per-tool lifetime counts,
error counts, latency, and the most recent invocations. Lifetime error counts
are cumulative history and never shrink; each tool also reports current health
(`currently_failing`, with the last error's timestamp and detail) so a resolved
past failure is not mistaken for a live one. This is how you verify an agent did
what it claimed, independent of its own narration (ADR-0010).

### Housekeeping

```text
graph_stats()      # node / active-edge counts
prune_graph()      # purge decayed edges + unreferenced stubs
promotion_candidates() # proven signal -> subject-level candidates for Lodestar promote_signals (ADR-0022)
boost_entity(id)   # mark a node as recently focused, without rewriting evidence
list_agents()      # roster + per-agent attention (needs MINDLEAK_AGENT set)
working_set()      # current agent's small ranked focus (default hard cap 7)
export_graph()     # complete active graph JSON for review (not a backup)
backup_database(path) # verified online backup; destination must not exist
```

Decay is the point — don't fight it. If context fades too fast or too slow, tune
half-lives rather than disabling decay.

`working_set(limit?)` is deliberately small. It requires `MINDLEAK_AGENT`, never
returns another agent's attention, and a requested limit can reduce but cannot
exceed `MINDLEAK_WORKING_SET_SIZE`. Each item includes effective attention,
observation count/span, and last-observed time. It is a derived focus view, not a
stored queue or a replacement for impact/graph traversal.

Destructive reset requires exact, plane-specific confirmation tokens. See
**[DATA-LIFECYCLE.md](DATA-LIFECYCLE.md)** for backup, upgrade/rollback, export,
reset, retention, and privacy procedures.

---

## The intent plane (Lodestar)

Use `lodestar-mcp` when work spans multiple agents or worktrees. It keeps a
versioned **constitution** (goals/constraints/invariants) and an **executive task
ledger** with atomic claim/lease coordination.

Typical flow:

```text
define_goal(kind, title, statement)          # write the constitution
get_constitution()                           # read this BEFORE acting
decompose_goal(goal_id)  /  create_task(...) # produce claimable work
next_task()                                  # what should I pick up?
claim_task(task_id, agent, paths?, symbols?) # atomic claim + optional advisory scope
advise(task_id, node_ids)                    # ADR-0029: what governs this change? (advise/review/block) — before acting
renew_lease(task_id, agent)                  # keep your claim alive while working
complete_task(task_id, agent, evidence)      # owner-guarded; runs conformance
board()                                      # live who-owns-what
```

`claim_task` is a compare-and-swap: parallel agents coordinate through one shared
`.lodestar/spec.db` with **no duplicate winners**. `complete_task` runs a
conformance check (aligned / drift / violation) and a violation blocks the
transition.

### Pre-flight overlap awareness

Before claiming work with known files or symbols, query both planes and combine
their results:

```text
lodestar.check_overlap(paths=["src/auth.ts"], symbols=[...]) # live declared claims
mindleak.check_overlap(paths=["src/auth.ts"], symbols=[...], exclude_agent="agent-b")
claim_task(task_id, "agent-b", paths=["src/auth.ts"], symbols=[...])
```

Lodestar compares concrete requested paths with path globs declared by active
claims and compares symbol ids exactly. MindLeak normalizes paths to `artifact:`
ids and returns other agents' direct or mutation-linked footprint only while its
derived effective weight remains active. Neither tool locks anything. On a
warning, coordinate, choose different work, or create a `blocked_by` handoff;
claiming anyway remains possible. The VS Code allocator runs this pre-flight,
asks before proceeding on overlap or a failed check, and shows persisted scope on
the Intent Board.

### The learned-knowledge loop (cross-plane, ADR-0022)

This is what makes a *fleet* of agents compound instead of running as N amnesiac
sessions. It spans both planes and needs no model:

```text
promotion_candidates()        # MindLeak: expiring proven signal -> subject-level candidates
promote_signals(candidates)   # Lodestar: the same candidates through the count + span gate
```

`promotion_candidates` (memory plane) aggregates proven-signal edges that are
about to decay into subject-level candidates — the distinct corroborating node
ids plus their provenance span — in exactly the shape `promote_signals` (intent
plane) consumes. Pipe one straight into the other. The existing **count + span
gate** decides what becomes durable knowledge (no new threshold, no laundering of
same-session coincidence), and that knowledge then informs conformance as an
**advisory** finding only — it can nudge an otherwise-`aligned` verdict to
`needs_human`, but never emits a `violation` (only the constitution hard-fails).
So agent A's hard-won regularity ("changes to X break Y") steers agent B before
the raw episodes fade.

### Progressive same-file handoff

Do not assign different symbols in one file to concurrent writers: symbol
boundaries are not text locks. Serialize them with task dependencies instead:

```text
first  = create_task(goal_id, "Edit Router", acceptance = "...")
second = create_task(goal_id, "Edit helper", acceptance = "...", blocked_by = first.id)

claim_task(first.id, "agent-a")
complete_task(first.id, "agent-a", evidence)  # aligned completion opens second
claim_task(second.id, "agent-b")
```

Only an aligned `done` transition opens the successor; review/violation leaves it
blocked. Chains are same-goal, acyclic, and one-to-one (`A -> B -> C`, not
fan-out). The deterministic two-connection benchmark holds maximum same-file
ownership at one with this pattern, versus two for independent tasks. It uses
synthetic schema-valid evidence to test mechanics. This is task serialization,
not a filesystem mutex (ADR-0015).

Full tool list: see the **Intent Plane tools** table in
[../README.md](../README.md); design in
[SPEC-INTENT.md](SPEC-INTENT.md) and [ADR-0004](adr/0004-intent-plane-spec-brain.md).

---

## Configuration reference

Runtime endpoints and identities use environment variables. Decay tuning also
supports a committable `<workspace>/.mindleak.toml`; environment overrides win
over file values, and built-in defaults remain when neither is set.

```toml
[decay]
prune_threshold = 0.05

[decay.half_life_hours]
modified = 24
failed_on = 24
calls = 168
contains = 168
imports = 168
depends_on = 168
```

All relation keys are supported: `modified`, `failed_on`, `calls`, `refactored`,
`relates_to`, `contains`, `observed`, `imports`, `extends`, `implements`, and
`depends_on`. Half-lives clamp to 1-8760 hours; the threshold clamps to
0.001-0.999. Non-finite, non-positive, or unparseable overrides are ignored so
the next valid layer wins. Unknown TOML keys fail startup rather than silently
selecting a default.

### `mindleak-mcp`

| Variable | Default | Meaning |
|---|---|---|
| `MINDLEAK_WORKSPACE` | process working directory | project root used for default database/config paths |
| `MINDLEAK_DB` | `<workspace>/.mindleak/graph.db` | graph database path |
| `MINDLEAK_AGENT` | *(empty = off)* | agent **base label** for attribution (`observed` edges); the running id is `<base>-<nonce>`, unique per process so concurrent agents never alias onto one id (ADR-0030) |
| `MINDLEAK_AGENT_ID` | *(empty)* | explicit, verbatim per-process id (pin); wins over the `MINDLEAK_AGENT` base + nonce, for tests and deliberately fixed identities (ADR-0030) |
| `MINDLEAK_WORKING_SET_SIZE` | `7` | hard cap for `working_set` results, bounded 1-32 |
| `MINDLEAK_CONFIG` | `<workspace>/.mindleak.toml` | explicit config path; relative paths resolve from the workspace |
| `MINDLEAK_PRUNE_THRESHOLD` | file or `0.05` | active-edge/prune threshold override |
| `MINDLEAK_HALFLIFE_<RELATION>_HOURS` | file or relation default | base half-life override, e.g. `MINDLEAK_HALFLIFE_FAILED_ON_HOURS` |
| `MINDLEAK_LLM_URL` | `http://localhost:11434/v1` | OpenAI-compatible consolidation server |
| `MINDLEAK_MODEL` | `glm4:9b` | consolidation model |
| `MINDLEAK_LLM_API_KEY` | *(empty)* | bearer token for hosted servers |
| `MINDLEAK_AUTONOMOUS_CONSOLIDATION` | `false` | explicit opt-in for idle model-backed consolidation; requires a file-backed database |
| `MINDLEAK_CONSOLIDATE_IDLE_SECS` | `300` | idle trigger, bounded 30-86400 seconds |
| `MINDLEAK_CONSOLIDATE_MIN_INTERVAL_SECS` | `3600` | minimum time between attempts, bounded 60-86400 seconds |
| `MINDLEAK_CONSOLIDATE_MAX_NODES` | `20` | candidates per pass, bounded 1-200 |
| `MINDLEAK_EMBED_URL` | `http://localhost:11434/v1` | embeddings server (for `recall`/`index`) |
| `MINDLEAK_EMBED_MODEL` | `nomic-embed-text` | embedding model |
| `MINDLEAK_EMBED_API_KEY` | *(empty)* | bearer token for hosted servers |
| `MINDLEAK_LOG` | `info` | tracing filter (`off`, `warn`, `debug`, `mindleak_core=debug`, …) — **stderr only** |
| `MINDLEAK_LOG_FORMAT` | `pretty` | `pretty` or `json` |
| `MINDLEAK_HTTP_TIMEOUT_MS` | `30000` | overall timeout per optional HTTP attempt, bounded 100-300000 ms |
| `MINDLEAK_HTTP_RETRIES` | `2` | extra transient-failure retries, bounded 0-5 |
| `MINDLEAK_BREAKER_THRESHOLD` | `5` | consecutive failures before the circuit opens |
| `MINDLEAK_BREAKER_COOLDOWN_MS` | `30000` | how long the circuit stays open before a probe |

### `lodestar-mcp`

| Variable | Default | Meaning |
|---|---|---|
| `LODESTAR_DB` | `<cwd>/.lodestar/spec.db` | intent-plane database path (share across worktrees) |
| `LODESTAR_AGENT` | *(empty)* | agent **base label** for task ownership; the running id is `<base>-<nonce>`, unique per process so concurrent agents never alias onto one claim (ADR-0030) |
| `LODESTAR_AGENT_ID` | *(empty)* | explicit, verbatim per-process id (pin); wins over the `LODESTAR_AGENT` base + nonce (ADR-0030) |
| `LODESTAR_LLM_URL` | `http://localhost:11434/v1` | OpenAI-compatible server for `decompose_goal` / semantic conformance |
| `LODESTAR_MODEL` | `glm4:9b` | model |
| `LODESTAR_LLM_API_KEY` | *(empty)* | bearer token for hosted servers |
| `MINDLEAK_HTTP_TIMEOUT_MS` | `30000` | shared HTTP timeout (also honoured here) |

---

## Principles & gotchas

- **Zero-token write path.** Ingestion never calls a model. Models are optional
  and asynchronous.
- **stdout is sacred.** The MCP protocol runs on stdout; all logs go to stderr.
  Never expect diagnostics on stdout.
- **Derived, not stored.** Effective edge weight is computed at query time from
  decay — it is never written back to the row.
- **Autonomous model spend is opt-in.** A configured model does not start the
  idle worker. When explicitly enabled, every pass is visible as `maintenance`
  telemetry; no-candidate passes use no model. Manual and idle signal
  consolidation share the configured minimum interval and workspace lease.
- **Local & unauthenticated by design.** The servers have no network listener;
  any process with stdio access can write. Do not expose them over a network
  without adding an auth layer.
- **The databases are regenerable.** `.mindleak/graph.db` and `.lodestar/spec.db`
  are gitignored and can be deleted and rebuilt from your work.
