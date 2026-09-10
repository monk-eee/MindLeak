# MindLeak Industrial Quickstart

Bring up the shared Ackplane server, enroll a node, and prove it end-to-end:
gRPC over TLS, the Bridge UI, and a running supervisor.

This is the **Industrial** profile — a shared Postgres-backed server several
repositories/nodes enroll into (ADR-0082 onward). If you only need one
repository talking to itself, you want the **Local** profile instead: see
[QUICKSTART.md](QUICKSTART.md). Everything below builds on top of it and does
not replace it — `mindleak-mcp`/`lodestar-mcp` keep working exactly as
QUICKSTART.md describes whether or not you ever touch Ackplane.

Ackplane, the Bridge, `register-me`, `ackplane-mcp`, and `ackplane-supervisor`
are not distributed in the release archive today; every binary below is built
from source.

---

## 1. Prerequisites

- Stable Rust 1.88+ (`cargo build` for the binaries below).
- Docker, or a drop-in `docker compose` (Podman's `podman compose` works the
  same way; substitute it for every `docker compose` command below).

---

## 2. Bring up the stack

```bash
node scripts/ackplane-compose.mjs up
```

Equivalent to `docker compose up -d --build`. One command starts everything
the Compose topology defines (ADR-0088): Postgres, schema migration, a
generated development TLS certificate, the `ackplane` gRPC service
(`https://127.0.0.1:8443`), and the Bridge UI (`http://127.0.0.1:3000`).

Confirm it's up:

```bash
curl -sS http://127.0.0.1:3000/live -o /dev/null -w "%{http_code}\n"
```

If anything is not healthy yet: `ackplane` waits for the schema migration and
the generated TLS material; `bridge` waits for the schema migration only.
`docker compose logs -f ackplane bridge` to see why.

---

## 3. Trust the development TLS certificate

`ackplane` serves real TLS by default (ADR-0132), using a certificate
`tls-init` generated into a named volume. Every client in this repo
(`register-me`, `ackplane-mcp`, `ackplane-supervisor`, `lodestar-mcp`'s
federation client) trusts a CA the same way — extract it once and point
`MINDLEAK_ACKPLANE_TLS_CA_PATH` at it:

```bash
mkdir -p .mindleak
docker compose cp ackplane:/tls/ca.crt .mindleak/ackplane-dev-ca.pem
export MINDLEAK_ACKPLANE_TLS_CA_PATH="$PWD/.mindleak/ackplane-dev-ca.pem"
```

Skipping this produces an opaque transport/h2 error, not a clear
"certificate untrusted" message — if a client below can't connect, check this
variable first.

---

## 4. Match the Bridge's tenant, or enrollment stays invisible

The Bridge derives its tenant id as `hex(SHA-256(salt || tenant_name))`
(ADR-0098 decision 3) from its own salt file and
`ACKPLANE_BRIDGE_DEVELOPMENT_TENANT` (`local-development` in the Compose
default). A node enrolled under a *different* name or salt derives a
*different* tenant id, enrolls successfully, and then never appears anywhere
in the Bridge — silently, with no error at any step. Extract the Bridge's
actual salt and reuse it instead of inventing your own:

```bash
docker compose cp bridge:/var/lib/ackplane-bridge/salt .mindleak/bridge.salt
```

Pass `--tenant-name local-development --salt-path .mindleak/bridge.salt` to
every `register-me` step below.

---

## 5. Build the client binaries

```bash
cargo build --release -p ackplane-server --bin register-me -p ackplane-mcp -p ackplane-supervisor
```

---

## 6. Enroll a node

Three explicit steps, mirroring the real actors (ADR-0085) — a node requests,
an administrator approves, the node activates:

```bash
BIN=target/release

$BIN/register-me request \
  --repo my-repo --node my-node \
  --tenant-name local-development --salt-path .mindleak/bridge.salt \
  --grpc-endpoint https://127.0.0.1:8443
# prints a request-id, the tenant id, and the exact `approve` command to run next

$BIN/register-me approve \
  --request-id <request-id printed above> \
  --tenant-name local-development --salt-path .mindleak/bridge.salt \
  --repo my-repo --fingerprint <fingerprint printed above> \
  --admin-database-url postgresql://ackplane:ackplane-development-only-not-for-production@127.0.0.1:5432/ackplane

$BIN/register-me activate --request-id <request-id>
# prints `activated: EnrollmentActivationResult { signing_key_id: "...", ... }` -- save that id
```

`approve` stands in for an administrative RPC/UI that doesn't exist yet — a
direct database connection, not how a real deployment approves nodes.
`activate` also opens one real `NodeSync` stream and sends a signed heartbeat,
so the node is immediately visible on the Bridge's Fleet page.

---

## 7. Run something real against the enrolled identity

Every consumer below wants the same five variables:

| Variable | Value |
|---|---|
| `MINDLEAK_ACKPLANE_TLS_CA_PATH` | from step 3 |
| `MINDLEAK_ACKPLANE_TENANT_ID` | the tenant id `register-me request` printed |
| `MINDLEAK_ACKPLANE_REPOSITORY_ID` | `my-repo` |
| `MINDLEAK_ACKPLANE_NODE_ID` | `my-node` |
| `MINDLEAK_ACKPLANE_SIGNING_KEY_ID` | the `signing_key_id` `activate` printed |

`MINDLEAK_ACKPLANE_NODE_SIGNING_KEY_SEED` (hex) is optional — unset, each
consumer falls back to the OS credential facility instead.

### Option A — a supervisor daemon

```bash
export ACKPLANE_SUPERVISOR_ID=supervisor-1
export MINDLEAK_ACKPLANE_ENDPOINT=https://127.0.0.1:8443
$BIN/ackplane-supervisor
```

Without worker definitions this is notification-only. To run multiple agents,
provide the worker map described in
[ackplane-supervisor's README](../crates/ackplane-supervisor/README.md) and run
`ackplane-supervisor --workers workers.json`. Rebuild the server and supervisor
from the same revision so the authenticated context exchange is available.
Each configured agent needs its own executable, working directory and branch.
Once it's
running, it appears on the Bridge's Supervisors page (or
`curl http://127.0.0.1:3000/api/v1/repositories/my-repo/supervisors`).

### Option B — `ackplane-mcp` (an MCP tool surface over the gRPC services)

Register it like any other MCP server, with the same five variables plus its
own endpoint variable — **note this is `ACKPLANE_MCP_ENDPOINT`, not
`MINDLEAK_ACKPLANE_ENDPOINT`**; the two crates name it differently for the
same value:

```jsonc
{
  "servers": {
    "ackplane": {
      "command": "/absolute/path/to/target/release/ackplane-mcp",
      "cwd": "/absolute/path/to/this/repo",
      "env": {
        "ACKPLANE_MCP_ENDPOINT": "https://127.0.0.1:8443",
        "MINDLEAK_ACKPLANE_TLS_CA_PATH": "/absolute/path/to/.mindleak/ackplane-dev-ca.pem",
        "MINDLEAK_ACKPLANE_TENANT_ID": "...",
        "MINDLEAK_ACKPLANE_REPOSITORY_ID": "my-repo",
        "MINDLEAK_ACKPLANE_NODE_ID": "my-node",
        "MINDLEAK_ACKPLANE_SIGNING_KEY_ID": "..."
      }
    }
  }
}
```

It refuses any non-loopback endpoint (ADR-0136 clause 4) and serves four
tools only — see [TOOLS.md](TOOLS.md#industrial-front-door-tools-ackplane-mcp).

### Option C — `lodestar-mcp`'s federation client

Build with the feature, then add `MINDLEAK_COORDINATION_MODE=federated` and
`MINDLEAK_ACKPLANE_ENDPOINT` (its own name for the endpoint, distinct from
`ackplane-mcp`'s `ACKPLANE_MCP_ENDPOINT` above) beside the same five
variables:

```bash
cargo build --release --features lodestar-mcp/federation-client -p lodestar-mcp
```

This is what lets `task_claim` arbitrate through Ackplane's leased delegation
(ADR-0096) instead of the Local profile's own SQLite claim table.

**Always set `"cwd"` explicitly in the MCP registration for both B and C.**
Without it, the process's working directory can differ from your repository
checkout, `git config`-based repository-id resolution silently resolves
against the wrong (or no) repository, and tool calls fail with confusing
"not found" errors for things you know exist.

---

## 8. Tear down

```bash
node scripts/ackplane-compose.mjs down              # stop the stack, keep the Postgres volume
node scripts/ackplane-compose.mjs reset --confirm   # stop it and delete the volume too
```

---

## See also

- [QUICKSTART.md](QUICKSTART.md) — the Local profile this builds on.
- [ackplane-supervisor's README](../crates/ackplane-supervisor/README.md) —
  supervisor configuration and failure modes in full.
- [TOOLS.md](TOOLS.md#industrial-front-door-tools-ackplane-mcp) —
  `ackplane-mcp`'s tool reference.
- [ARCHITECTURE.md](ARCHITECTURE.md) — how Ackplane, the Bridge, and the
  supervisor fit together, and every ADR cited above.
