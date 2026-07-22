## Problem statement

<!-- What is broken or missing? Why does this change need to happen? -->

## Proposed change

<!-- What did you change and why? Structure this so a reviewer can follow it safely. -->

## Design principles check

<!-- MindLeak has load-bearing rules — confirm this change respects them. -->

- [ ] **Zero-token write path** preserved (no LLM calls added to ingest/query hot paths)
- [ ] **Effective weight stays derived** (no background job rewriting edge weights)
- [ ] Decay half-lives tuned rather than disabled, if relevant

## Risk & rollback

| Risk | Severity | Mitigation |
|------|----------|------------|
|      |          |            |

**Rollback:** <!-- the graph db is regenerable; note any migration concerns -->

## Test evidence

<!-- Paste `cargo test` / eslint / compile output. Bug fixes need a regression test. -->

## Docs updated

- [ ] `CHANGELOG.md` (any user- or operator-visible change)
- [ ] `docs/SPEC.md` / `docs/ARCHITECTURE.md` (design or module change)
- [ ] `README.md` tool table (added/removed an MCP tool)
- [ ] `docs/adr/` (a decision that is hard to reverse or surprising)
- [ ] N/A — purely internal, non-observable refactor

## Pre-merge checklist

- [ ] Scoped to one clear outcome; unrelated changes split out
- [ ] `cargo fmt --check`, `cargo clippy -D warnings`, `cargo test` all green
- [ ] Extension `lint` + `compile` green (if `editors/vscode` touched)
- [ ] Tests cover the changed paths, not just happy-path
- [ ] No secrets or credentials committed
- [ ] AI-generated content (if any) reviewed — I can explain and defend it
