# ADR-0001 — Record architecture decisions

- **Status:** Accepted
- **Date:** 2026-07-22

## Context

MindLeak has a few load-bearing design choices (a decay-weighted graph, a
zero-token write path, derived edge weights) that are easy to misread as
accidental and "simplify" into regressions. Code shows *what*; it does not
preserve *why*.

## Decision

We keep a lightweight ADR log under `docs/adr/`. We record a decision as an ADR
when it is **hard to reverse or surprising** — not for routine changes. ADRs are
immutable once accepted; a later decision supersedes an earlier one rather than
editing it.

## Consequences

- New contributors and agents can find the rationale behind non-obvious choices.
- Settled decisions stop being re-litigated.
- The log must be kept honest: an empty or stale ADR folder is worse than none,
  so we only record decisions we would actually defend.
