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
- Node.js 20+ for the portable helper commands.
- Docker, or a drop-in `docker compose` (Podman's `podman compose` works the
  same way; substitute it for every `docker compose` command below).
- An available OS credential facility for the explicitly selected software
  provider. Keep its state in a user-local directory, outside Git and cloud sync.

---

## 2. Bring up the stack

```text
node scripts/ackplane-compose.mjs up
```

Equivalent to `docker compose up -d --build`. One command starts everything
the Compose topology defines (ADR-0088): Postgres, schema migration, a
generated development TLS certificate, the `ackplane` gRPC service
(`https://127.0.0.1:8443`), and the Bridge UI (`http://127.0.0.1:3000`).

Confirm it's up:

```text
curl --fail http://127.0.0.1:3000/live
```

If anything is not healthy yet: `ackplane` waits for the schema migration and
the generated TLS material; `bridge` waits for the schema migration only.
`docker compose logs -f ackplane bridge` to see why.

---

## 3. Prepare Local Trust

Choose a user-local configuration directory outside Git and cloud sync. Run:

```text
node scripts/ackplane-compose.mjs prepare ABSOLUTE_CONFIG_DIR
```

The helper copies only the public development CA and the Bridge's tenant salt
into `ackplane-dev-ca.pem` and `bridge.salt`. It validates both before publishing
either, creates new files with restrictive permissions, and leaves identical
files unchanged on repeat runs. Different existing trust material is refused,
never overwritten. No private signing key or TLS server key is exported.

Copy or validation failure leaves existing trust untouched. A late filesystem
failure may leave one newly created file; rerun preparation to fill the missing
partner after resolving the error. Do not delete existing trust to suppress a
mismatch. Check the source stack and the intended configuration directory first.

---

## 4. Configure Once

The Bridge derives its tenant id as `hex(SHA-256(salt || tenant_name))`
(ADR-0098 decision 3) from its own salt file and
`ACKPLANE_BRIDGE_DEVELOPMENT_TENANT` (`local-development` in the Compose
default). Enrollment must use that same name and the prepared salt, or its
records belong to a different tenant and will not appear in this Bridge.

In the repository's ignored `.env` file, set the CA path for enrollment and the
companion:

```dotenv
MINDLEAK_ACKPLANE_TLS_CA_PATH=ABSOLUTE_CONFIG_DIR/ackplane-dev-ca.pem
```

Replace the placeholder with a real absolute path; `.env` uses plain values,
without shell quotes or variable expansion. The existing `run-ackplane` launcher
loads this file for every command below, while already-set process environment
values take precedence. The companion owns TLS and remote signing; its
supervisor and MCP consumers do not load the CA or key. Never disable
certificate verification to work around a failed connection.

Protect the salt as local installation state; do not commit or print it.
Activation and serving reuse the recorded tenant instead of deriving another
identity. No key bytes belong in `.env`.

---

## 5. Build the client binaries

```text
cargo build --release --locked -p ackplane-server --bin register-me
cargo build --release --locked -p ackplane-mcp -p ackplane-supervisor
```

Commands below run from the repository root. The launcher selects the native
executable, including `.exe` on Windows, and passes arguments unchanged without
a shell. Replace `ABSOLUTE_CONFIG_DIR` and `ABSOLUTE_STATE_DIR` with real
absolute paths, quoted in commands when they contain spaces. The state directory
must remain the same through request, activation, serving, and consumer setup.

---

## 6. Enroll a node

Three explicit steps, mirroring the real actors (ADR-0085) — a node requests,
an administrator approves, the node activates:

```text
node scripts/run-ackplane.mjs register-me request --repo my-repo --node my-node --tenant-name local-development --salt-path ABSOLUTE_CONFIG_DIR/bridge.salt --grpc-endpoint https://127.0.0.1:8443 --provider credential-facility-software --state-dir ABSOLUTE_STATE_DIR
```

The response prints the request id, tenant id, and fingerprint. An administrator
reviews that exact identity before running the separate approval step:

```text
node scripts/run-ackplane.mjs register-me approve --request-id REQUEST_ID --tenant-name local-development --salt-path ABSOLUTE_CONFIG_DIR/bridge.salt --repo my-repo --fingerprint FINGERPRINT --admin-database-url postgresql://ackplane:ackplane-development-only-not-for-production@127.0.0.1:5432/ackplane
```

After approval, activate the original request with the same provider state:

```text
node scripts/run-ackplane.mjs register-me activate --request-id REQUEST_ID --state-dir ABSOLUTE_STATE_DIR
```

`approve` stands in for an administrative RPC/UI that doesn't exist yet — a
direct database connection, not how a real deployment approves nodes.
`activate` also opens one real `NodeSync` stream and sends a signed heartbeat,
so the node is immediately visible on the Bridge's Fleet page.

The provider persists the assigned key id and activation receipt. Do not copy
private keys, invent a new state directory to recover an existing identity, or
configure a seed override. A repeated identical request or activation reuses its
saved state; changed identity parameters are refused.

## 7. Start The Companion

Keep this process running while any enrolled consumer is in use:

```text
node scripts/run-ackplane.mjs register-me serve --state-dir ABSOLUTE_STATE_DIR
```

Wait for `node companion ready`. It recovers the enrolled provider, verifies
current server authority, and exposes protected local IPC. Use the same
`register-me` executable used for enrollment so native credential access remains
associated with the same executable. Serving is not a new enrollment or approval.

---

## 8. Run Consumers

Every consumer below uses the same three non-secret settings. Add these to the
ignored `.env` for commands launched here, and use the same values in MCP
registrations:

| Variable | Value |
|---|---|
| `MINDLEAK_ACKPLANE_STATE_DIR` | `ABSOLUTE_STATE_DIR`, the companion's provider directory |
| `MINDLEAK_ACKPLANE_TENANT_ID` | the tenant id `register-me request` printed |
| `MINDLEAK_ACKPLANE_REPOSITORY_ID` | `my-repo` |

```dotenv
MINDLEAK_ACKPLANE_STATE_DIR=ABSOLUTE_STATE_DIR
MINDLEAK_ACKPLANE_TENANT_ID=TENANT_ID_FROM_REQUEST
MINDLEAK_ACKPLANE_REPOSITORY_ID=my-repo
```

Remove obsolete node id, key id, key-path and seed environment overrides; the
current runtime rejects them. Only the companion accesses the credential
facility and signs remote operations. Its loss stops active workers and preserves
unconfirmed evidence; it is not permission to restart with a replacement key.

### Option A — a supervisor daemon

Add the supervisor id and a separate absolute user-local queue directory to
`.env`:

```dotenv
ACKPLANE_SUPERVISOR_ID=supervisor-1
ACKPLANE_SUPERVISOR_STATE_DIR=ABSOLUTE_QUEUE_DIR
```

Then run in another terminal, leaving the companion running:

```text
node scripts/run-ackplane.mjs supervisor --workers ABSOLUTE_WORKERS_FILE
```

Use the worker map in [the supervisor guide](../crates/ackplane-supervisor/README.md).
Each configured agent needs an installed, authenticated executable, its own
working directory and the actual branch. Without worker definitions the daemon
is notification-only, not an agent runner. Build the server, companion, and
supervisor from the same revision. Its registered sessions appear on the Bridge's
Supervisors page; assignment still requires an authorized, confirmed Work command.

### Option B — `ackplane-mcp` (an MCP tool surface over the gRPC services)

Register it with the companion directory and scope. `ACKPLANE_MCP_ENDPOINT`
pins the expected loopback endpoint; it must match the companion's endpoint.

```jsonc
{
  "servers": {
    "ackplane": {
      "command": "/absolute/path/to/target/release/ackplane-mcp",
      "cwd": "/absolute/path/to/this/repo",
      "env": {
        "ACKPLANE_MCP_ENDPOINT": "https://127.0.0.1:8443",
        "MINDLEAK_ACKPLANE_STATE_DIR": "/absolute/user-local/provider-state",
        "MINDLEAK_ACKPLANE_TENANT_ID": "...",
        "MINDLEAK_ACKPLANE_REPOSITORY_ID": "my-repo"
      }
    }
  }
}
```

It refuses any non-loopback endpoint (ADR-0136 clause 4) and serves four
tools only — see [TOOLS.md](TOOLS.md#industrial-front-door-tools-ackplane-mcp).

### Option C — `lodestar-mcp`'s federation client

Build with the feature, then add `MINDLEAK_COORDINATION_MODE=federated` beside
the same three companion settings:

```text
cargo build --release --locked --features lodestar-mcp/federation-client -p lodestar-mcp
```

This is what lets `task_claim` arbitrate through Ackplane's leased delegation
(ADR-0096) instead of the Local profile's own SQLite claim table.

**Always set `"cwd"` explicitly in the MCP registration for both B and C.**
Without it, the process's working directory can differ from your repository
checkout, `git config`-based repository-id resolution silently resolves
against the wrong (or no) repository, and tool calls fail with confusing
"not found" errors for things you know exist.

---

## 9. Tear Down

Stop consumers before the companion, then stop the stack while retaining data:

```text
node scripts/ackplane-compose.mjs down
```

Only when you intend to erase this development stack's database, use
`node scripts/ackplane-compose.mjs reset --confirm`. Preserve provider state and
unconfirmed supervisor evidence; deleting a run marker is not recovery.

---

## See also

- [QUICKSTART.md](QUICKSTART.md) — the Local profile this builds on.
- [ackplane-supervisor's README](../crates/ackplane-supervisor/README.md) —
  supervisor configuration and failure modes in full.
- [TOOLS.md](TOOLS.md#industrial-front-door-tools-ackplane-mcp) —
  `ackplane-mcp`'s tool reference.
- [ARCHITECTURE.md](ARCHITECTURE.md) — how Ackplane, the Bridge, and the
  supervisor fit together, and every ADR cited above.
