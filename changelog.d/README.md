# Changelog fragments

A change describes itself here, in its own file. `CHANGELOG.md` is assembled
from these at release time (ADR-0056).

**Do not edit `CHANGELOG.md` in a pull request.** It is a shared append-only
file, and `.gitattributes` marks it `merge=union` — which git honours in a
checkout and GitHub's merge machinery ignores. Five pull requests in one day
reported a conflict that did not exist, and auto-merge silently stopped working
on each. Two branches never write the same fragment path, so there is nothing to
merge.

## Writing one

Create `<section>-<slug>.md`, where `<section>` is one of `added`, `changed`,
`deprecated`, `removed`, `fixed`, `security`, and the body is the entry exactly
as it should read in the changelog:

```
changelog.d/fixed-empty-parent-startup.md
```

```markdown
- **The server no longer exits at startup when the database path has no
  directory.** `MINDLEAK_DB=":memory:"` resolves to a path whose `parent()` is
  `Some("")`, and the Unix branch then called `set_permissions` on it.
```

Same voice as the existing changelog: what changed, and why it mattered. The
section prefix is part of the filename so a reviewer reading a pull request's
file list sees both the section and the subject without opening anything.

## Commands

| | |
|---|---|
| `node scripts/changelog.mjs --check` | validate every fragment (pre-commit and CI) |
| `node scripts/changelog.mjs` | print what the next release would contain |
| `node scripts/changelog.mjs --release 0.1.4` | fold fragments into a dated section and delete them |

The release command also folds anything already sitting under
`## [Unreleased]`, so entries written before this convention are not lost.
