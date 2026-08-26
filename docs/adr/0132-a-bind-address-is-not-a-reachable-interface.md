# ADR-0132: A bind address is not a reachable interface

- Status: Accepted
- Date: 2026-08-26
- Deciders: MindLeak maintainers
- Accepted: 2026-08-26 by the repository owner, authorized directly in session
  — attributed human adoption after review.
- Refines: [ADR-0083](0083-grpc-is-the-ackplane-node-protocol.md) clause 8 (TLS
  is mandatory outside loopback)
- Related: [ADR-0088](0088-the-ackplane-runs-in-containers-the-planes-do-not.md)
  (the Ackplane runs in containers),
  [ADR-0082](0082-ackplane-is-a-standalone-federation-service.md) (Ackplane is a
  standalone federation service)

## Context

ADR-0083 clause 8 says **"TLS is mandatory outside loopback."** That is a
statement about *reachability*. The implementation tests
`listen.ip().is_loopback()`, which is a statement about the *bind address*.
Everywhere the two agree, nobody notices. Inside a container they cannot agree,
by construction.

A published container port forwards to the container's address on the bridge
network. A process bound to the container's own loopback is therefore reachable
from nowhere at all — not from the host, not from a sibling service. To be
reachable through `127.0.0.1:8443` on the host, the process inside must bind
`0.0.0.0`. The supported Compose topology says exactly this, and is right to:

> the container listens on every interface internally (ACKPLANE_LISTEN=0.0.0.0),
> but the published port below is what actually decides reachability from the
> host, and that stays 127.0.0.1

So the topology set `0.0.0.0`, the server refused it, and the container
crash-restarted forever. **The standalone federation service (ADR-0082) had no
working documented bring-up at all**, and had not had one for as long as the
gRPC service has been a real long-running process. Both sides were individually
correct, which is why neither looked like the bug.

Two further facts shaped this decision. The server cannot verify a confinement
claim from inside the container: it cannot see Docker's published-port mapping,
a service mesh, or a host firewall. And a `0.0.0.0` bind is not evidence of
exposure either — it is the only way a container is reachable *at all*.

## Decision

**Clause 8 binds on reachability, and a deployment that cannot be asked must
say which it is.**

1. **Serving TLS is the default answer, including in development.** The
   supported Compose topology generates a self-signed certificate into a named
   volume and serves it. A development deployment that satisfies clause 8
   honestly needs no exception and makes no claim; this is the path a reader
   copies.

2. **A deployment may declare what confines a plaintext listener, and must name
   it.** `ACKPLANE_LISTEN_CONFINED_BY` takes a non-empty description — a
   container's published port, a service mesh, a host firewall. It is not a
   boolean: an operator who cannot name the mechanism does not have one, and a
   bare `=true` records nothing a reviewer can weigh.

3. **The declaration is narrow and cannot widen.** It permits exactly one
   thing: a listener bound outside loopback with no TLS material. It never
   disables TLS that is configured, never affects authentication, tenant
   binding, signatures, or evidence trust, and is ignored entirely when a
   certificate is present.

4. **A plaintext endpoint is never quiet.** The startup banner names both the
   fact and the claim: `serving PLAINTEXT outside loopback, confined by <what>`.
   The whole basis for permitting it is an assertion about the environment, and
   an assertion nobody can read is indistinguishable from a misconfiguration.

5. **Clause 8 is unchanged in substance.** TLS remains mandatory wherever the
   endpoint is genuinely reachable. This decision refines what "outside
   loopback" means when the process cannot observe its own reachability; it does
   not license plaintext on a network.

## Consequences

`docker compose up` brings up a stack that serves TLS 1.3 with ALPN `h2`, which
is what the protocol decision asked for in the first place — the dev topology
now demonstrates the production property instead of sidestepping it.

The escape hatch is real and can be misused: an operator can write
`ACKPLANE_LISTEN_CONFINED_BY="nothing at all"` and serve plaintext on a public
interface. That is deliberate. A confinement that only something outside the
process can know is not checkable from inside it, so the honest options were an
unusable server or an auditable declaration. Decision 4 is what keeps it
auditable, and decision 1 is what keeps it unnecessary.

The self-signed development certificate is not a production answer, and clients
must choose to trust it. Certificate distribution for a real deployment is
untouched here and remains open.

## Alternatives considered

**Leave clause 8 as written and require a certificate always.** This is
decision 1, and it is the default — but as the *only* option it forbids every
deployment that terminates TLS at a mesh sidecar or load balancer, which is the
common production shape. Refusing to start there would push operators to fork
the config check, which is worse than an auditable declaration.

**Detect the confinement instead of declaring it.** The server would have to
inspect Docker's port mappings, the host firewall, and any mesh — from inside a
container, for every environment it might run in. Wrong answers here fail in the
dangerous direction, and a detector that is confidently wrong is worse than a
declaration that is merely trusted.

**Treat `0.0.0.0` as loopback when a container is detected.** Container
detection is a heuristic (`/.dockerenv`, cgroup paths) that a nested or
rootless runtime breaks, and the conclusion it feeds is a security one. It also
answers the wrong question: being in a container says nothing about whether the
port is published to the world.

**A boolean opt-out (`ACKPLANE_ALLOW_PLAINTEXT=true`).** Records that someone
wanted it, never why. Decision 2's free text costs the operator one sentence and
gives a reviewer the only thing worth reading.
