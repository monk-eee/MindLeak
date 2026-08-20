### Added

- `scripts/worktree-owner.mjs --adopt-worktree` now checks `gh pr list --head <branch> --state all` before recording a deliberate handover, and prints a named warning when the branch already has a published pull request — advisory, not a hard gate, so it never blocks a genuine rescue. Degrades visibly (never silently) when `gh` is unavailable or unauthenticated.
