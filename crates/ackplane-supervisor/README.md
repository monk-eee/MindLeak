# ackplane-supervisor

The enrolled supervisor daemon: the Industrial runtime endpoint an operator can
actually run (ADR-0116).

It connects to Ackplane over authenticated gRPC, registers, opens a session,
heartbeats, and receives directives — durably receipting each one and returning
the receipt over the same stream. On reconnect it reconciles its position and
reports a genuine gap rather than resuming through one.

## What it cannot do yet

**It has no worker adapter, so it cannot drive a worker process.** It says so in
its own registration rather than by refusing after the fact.

It declares exactly one capability, `notify`. That is not a placeholder: a
`NotifyDirective` carries a message *to the supervisor*, so receiving it and
durably recording it is the whole action, and an `accepted` receipt for one is
truthful.

Every worker-driving capability — `prompt`, `assign`, `steer`, `pause`,
`resume`, `drain`, `terminate` — is deliberately **not** declared. Ackplane
refuses to enqueue a directive whose capability the target never declared, so
that work never reaches the queue; and if one arrives anyway, it is durably
receipted `refused` / `capability_missing`.

An `accepted` receipt for work nothing performed is therefore unreachable, not
merely unlikely. Wiring a real `WorkerAdapter` in is a separate, deliberate
change.

## Running it against the local stack

Bring up the Compose topology:

```bash
docker compose up -d postgres migrate ackplane
```

Enrol this node, if you have not already. `register-me` prints the tenant id and
the signing key id you will need:

```bash
cargo run -p ackplane-server --bin register-me -- request \
  --repo my-repo --node my-node --tenant-name my-tenant --salt-path .mindleak/bridge.salt
cargo run -p ackplane-server --bin register-me -- approve \
  --request-id <ID> --repo my-repo --fingerprint <FP> \
  --tenant-name my-tenant --salt-path .mindleak/bridge.salt \
  --admin-database-url postgres://ackplane:...@127.0.0.1:5432/ackplane
cargo run -p ackplane-server --bin register-me -- activate --request-id <ID>
```

Then declare the same identity and run the daemon:

```bash
export MINDLEAK_ACKPLANE_ENDPOINT=http://127.0.0.1:8443
export MINDLEAK_ACKPLANE_TENANT_ID=<from register-me>
export MINDLEAK_ACKPLANE_REPOSITORY_ID=my-repo
export MINDLEAK_ACKPLANE_NODE_ID=my-node
export MINDLEAK_ACKPLANE_SIGNING_KEY_ID=<from register-me activate>
export ACKPLANE_SUPERVISOR_ID=supervisor-1

cargo run -p ackplane-supervisor
```

On Windows PowerShell, use `$env:NAME = "value"` instead of `export`.

It should log `starting the Ackplane supervisor`, warn that no worker adapter is
wired in, and then hold the connection. The supervisor and its session appear on
the Bridge's Supervisors page.

## Configuration

Every variable is one this repository already uses; none is invented here.

| Variable | Required | Meaning |
|---|---|---|
| `MINDLEAK_ACKPLANE_ENDPOINT` | yes | Ackplane's gRPC endpoint |
| `MINDLEAK_ACKPLANE_TENANT_ID` | yes | Enrolled tenant |
| `MINDLEAK_ACKPLANE_REPOSITORY_ID` | yes | Enrolled repository |
| `MINDLEAK_ACKPLANE_NODE_ID` | yes | Enrolled node |
| `MINDLEAK_ACKPLANE_SIGNING_KEY_ID` | yes | Key id from activation |
| `ACKPLANE_SUPERVISOR_ID` | yes | This supervisor's id; one node may run several |
| `MINDLEAK_ACKPLANE_NODE_SIGNING_KEY_SEED` | no | Hex seed override. Unset uses the OS credential facility |
| `ACKPLANE_SUPERVISOR_STATE_DIR` | no | Durable inbox/outbox directory (default `.mindleak/supervisor`) |
| `ACKPLANE_SUPERVISOR_HEARTBEAT_SECONDS` | no | Heartbeat interval (default `30`) |
| `RUST_LOG` | no | Log filter (default `info`) |

A missing variable is refused at startup, and **every** missing one is named at
once — configuring a new node otherwise means learning about the next omission
only after fixing the previous one.

## When it stops

- **Connection dropped** — reconnects after a short delay.
- **`Ackplane holds supervisor evidence this node cannot account for`** — stops
  deliberately. The server has accepted more than this supervisor's durable
  state can describe, which means local state was lost (restored from an older
  copy, or truncated). Reconnecting cannot restore it, and resuming would hide
  it, so the daemon reports and exits for a person to investigate.
