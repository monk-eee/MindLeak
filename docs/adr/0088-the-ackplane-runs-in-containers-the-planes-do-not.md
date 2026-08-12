# ADR-0088: Ackplane runs in containers; the planes do not

- Status: Accepted
- Date: 2026-08-10
- Deciders: MindLeak maintainers
- Accepted: 2026-08-10 by the repository owner — attributed human adoption after
  review.
- Depends on: [ADR-0082](0082-ackplane-is-a-standalone-federation-service.md)
  (local-first boundary),
  [ADR-0086](0086-postgresql-is-the-ackplane-ledger-and-arbiter.md)
  (PostgreSQL authority and durability reporting)
- Related: [ADR-0016](0016-platform-packaging-and-registration.md) (packaging and
  registration), [ADR-0013](0013-local-data-lifecycle.md) (backup, export, reset),
  [ADR-0085](0085-node-enrolment-requires-proof-of-possession.md) (enrolment
  credentials)

## Context

Ackplane needs PostgreSQL with a specific version, extensions, roles, and
migrations. Asking a contributor to install and configure that by hand makes the
first run unreliable and the second run different. It also makes review harder:
a reported failure cannot be reproduced when everyone's database differs.

The obvious answer is containers, and the obvious risk is that containers become
the product's assumption. MindLeak's local planes are deliberately a pair of
stdio binaries with an embedded SQLite store. If a developer starts needing
Docker to run `mindleak-mcp`, index a repository, or execute the test suite, the
local-first property has been traded away by accident rather than by decision.

There is a second, quieter risk. A development Compose file grows production
features one convenience at a time — a published port, a persistent volume, a
default password everyone reuses — until it becomes an unofficial deployment
topology that no availability or fencing decision ever reviewed.

## Decision

1. **Docker Compose is the supported topology for local development and small
   self-hosting of Ackplane.** The stack is a `postgres` service, a one-shot
   `migrate` service, and the `ackplane` service. The Bridge's static
   assets ship with the server or as one additional service.

2. **The repository-local planes never require a container runtime.**
   `mindleak-mcp`, `lodestar-mcp`, the VS Code extension, `cargo build`,
   `cargo test`, and the extension test suite must all run on a machine with no
   Docker, no PostgreSQL, and no network. CI proves this with a job that has no
   container runtime available, so the property fails loudly rather than
   eroding.

3. **Host commands stay platform-agnostic.** A developer types `docker compose`,
   `cargo`, `npm`, or `node` and nothing else. No host-side bash, PowerShell, or
   `cmd` entrypoint is committed. Scripts *inside* an image are Linux and may use
   a shell; the portability rule governs what a person runs on their own machine.

4. **Images are pinned, minimal, and non-root.** The PostgreSQL image is pinned
   by tag and digest. The server image is a multi-stage build from a pinned Rust
   toolchain onto a minimal runtime, runs as an unprivileged user, contains no
   secrets or credentials, and carries the version and commit it was built from.

5. **Migrations are an explicit step that must finish before traffic.** The
   `migrate` service runs to completion; the server starts only after it
   succeeds and PostgreSQL reports healthy. Service ordering uses healthchecks
   rather than sleeps. The server does not silently migrate a production
   database at boot.

6. **Development defaults are obviously non-production.** Generated or clearly
   labelled development credentials, binding to loopback by default, and a
   startup banner naming the profile. The Compose stack reports ADR-0086
   `single_node` durability, because one PostgreSQL container is not a fenced
   primary with synchronous standbys, and it never presents itself as an
   availability guarantee.

7. **State is a named volume with documented lifecycle commands.** Backup,
   restore, and reset are explicit operations following the spirit of ADR-0013.
   Recreating a container never destroys the ledger as a side effect, and reset
   requires an unambiguous confirmation.

8. **Compose is not the production deployment contract.** Orchestrated
   production deployment, managed PostgreSQL, secret management, and TLS
   termination are a separate decision. Compose must not accrete production-only
   features until it becomes the de-facto answer that no one chose.

## Consequences

- A contributor gets a working Ackplane and database with one command, and
  reviewers reproduce failures against the same versions and extensions.
- The local-first guarantee becomes testable rather than aspirational: a CI job
  without a container runtime either passes or the claim was false.
- Pinned images make the stack reproducible and give a later supply-chain
  attestation something concrete to sign.
- The project takes on image maintenance: base image updates, vulnerability
  patching, and keeping the pinned PostgreSQL aligned with any extension the
  deployment enables.
- Developers who only work on the local planes are unaffected and never install
  Docker.
- A production deployment story is still owed. Naming that gap keeps Compose
  from becoming it by default.

## Rejected alternatives

**Document manual PostgreSQL setup instead.** Rejected because environment drift
is the failure it causes, and it makes every unreproducible bug the reviewer's
problem.

**Containerise the local planes too, for symmetry.** Rejected because it would
add a runtime dependency to the offline, zero-token local path and put a
container boundary between an editor's stdio client and its server for no gain.

**Embed PostgreSQL in the Ackplane image.** Rejected because it couples the
stateless service to its durable store, breaks the horizontally replaceable
instance model, and produces a container whose restart risks the ledger.

**Use Kubernetes manifests as the entry point.** Rejected as the starting point
because it requires a cluster to run one service locally. It remains the likely
production answer and gets its own decision.

**Auto-migrate on server boot.** Rejected because concurrent instances would
race the same schema change and a rollback would have no defined point of
control.
