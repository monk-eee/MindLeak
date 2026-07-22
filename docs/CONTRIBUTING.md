# Contributing

Thanks for helping build MindLeak. This project favours small, principled diffs
over large speculative ones.

## Ground rules

- **Tests are how we ship.** Every new collector, tool, parser, or bug fix comes
  with a test. Bug fixes get a regression test that fails before the fix and
  passes after.
- **Do the right thing, not the expedient thing.** No back-compat shims or
  "temporary" bridges — migrate callers properly.
- **Zero-token write path is sacred.** Ingestion must stay deterministic (regex /
  path / exit code). LLM calls belong only in the async consolidation layer.
- **No `--no-verify`.** Pre-commit hooks are the first line of defence; CI is the
  safety net.

## Workflow

1. Create a branch.
2. Make the change + tests.
3. `make lint && make test` (or the direct commands in
   [DEVELOPERS.md](../DEVELOPERS.md)) must be green.
4. Commit — pre-commit runs fmt, clippy, eslint, prettier.
5. Open a PR. CI runs the same checks on Linux.

## Style

- **Rust:** `cargo fmt` (max width 100), clippy clean with `-D warnings`. Prefer
  small modules; keep files focused.
- **TypeScript:** eslint + prettier (100 columns). No `any` unless justified.
- **Commits:** one meaningful unit of work each; imperative subject lines.

## Commit-message rationale markers

MindLeak ingests `DECISION:`, `HACK:`, `WHY:`, `NOTE:`, and `FIXME:` markers from
commit messages into intent-node content. Use them — they become queryable graph
context:

```
Fix session drop on refresh

// DECISION: null-guard the JWT path instead of retrying, to avoid a refresh loop
```

## Reporting bugs

Open an issue with: what you observed, where (file + symbol or test name), and
the impact if known.
