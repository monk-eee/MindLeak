#!/bin/sh
# Relays the container's real network interface to the Bridge process's own
# loopback bind.
#
# `BridgeConfig::resolve` (crates/ackplane-bridge/src/lib.rs) refuses any
# non-loopback listen address until a production authentication verifier
# exists for the Bridge (ADR-0094), so the process itself always binds
# 127.0.0.1 inside the container. Docker's published-port mechanism forwards
# host traffic to a container's real network interface, never to that
# container's own loopback -- confirmed empirically: `bridge`'s own
# /proc/net/tcp showed it listening only on 127.0.0.1:3000, and a host curl
# against a port published straight from that bind hung rather than
# connecting, even though the container reported healthy (a healthcheck runs
# inside the same network namespace, where loopback is reachable regardless).
#
# This relay does not touch BridgeConfig's validation or the security posture
# it encodes at all -- Bridge still binds and refuses non-loopback exactly as
# ADR-0094 requires. It only makes that already-loopback-bound process
# reachable across the container boundary, the same guarantee Compose's
# published port already gives `ackplane`'s own 0.0.0.0 bind (ADR-0132),
# without Bridge's own code ever seeing or trusting a wider bind address.
#
# Listens on a different port (3001) than Bridge's own (3000): binding both
# 0.0.0.0:3000 and 127.0.0.1:3000 in the same network namespace conflicts,
# since a 0.0.0.0 listener already claims every address on that port,
# including loopback.
set -eu

socat TCP-LISTEN:3001,fork,reuseaddr TCP:127.0.0.1:3000 &

exec /usr/local/bin/ackplane-bridge
