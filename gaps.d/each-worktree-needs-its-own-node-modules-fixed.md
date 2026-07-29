- **Each worktree needs its own `node_modules` — FIXED.** — `npm ci` in
  `editors/vscode` costs ~13s and ~449 packages per worktree. Worse than the
  cost was the symptom: a fresh worktree failed at *push* time with
  `Cannot find module .../prettier/bin/prettier.cjs`, which says nothing about
  the real cause, and failed extension tests with an `npx` prompt offering to
  install `vitest` rather than a clear "dependencies not installed". Hit four
  times in one session. — Low impact, real friction: it made spinning up a
  worktree for a small docs change feel disproportionate, which is exactly the
  pressure that pushes agents back into the shared checkout ADR-0038 moved them
  out of. — Fixed Jul 2026: `make worktree-setup` installs just the extension
  deps. Hooks and cargo tools are shared through the common `.git` dir and the
  user's cargo bin, so a linked worktree needs nothing else, and running the
  full `make setup` per worktree would re-run `pip install` and
  `cargo install` for no reason.
