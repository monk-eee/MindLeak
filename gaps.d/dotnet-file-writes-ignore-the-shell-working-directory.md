- **`[System.IO.File]::WriteAllText` with a relative path writes to a different
  directory than the one you are standing in, silently — MEASURED 2026-08-30,
  OPEN: nothing detects it.**
  PowerShell's current directory and .NET's process-wide current directory are
  separate. `Set-Location`/`cd` moves the first and not the second, so any
  `[System.IO.File]::*` call given a relative path resolves against wherever the
  process started — not against the directory the surrounding commands are
  operating on.

  Measured while working in a second worktree: three edits made with
  `ReadAllText`/`Replace`/`WriteAllText` on `crates/...`-relative paths reported
  success, and `git status` in that worktree showed nothing changed. They had
  landed in the *first* worktree, which is where the shell process had started.
  Both files there were modified — including a Rust source file — and the only
  reason it was caught is that a later grep for the inserted text came back
  empty.

  **Impact, and why it is worse than a wasted edit.** The write is silent in
  both directions: the intended file is unchanged with no error, and an
  unrelated checkout is modified with no indication. In a fleet that is a
  worktree-ownership violation waiting to happen — had the other checkout
  belonged to a peer, this would have written into their branch, which is
  exactly the corruption `worktree-owner` (exit 4) exists to prevent, arriving
  by a route that hook cannot see because no commit is involved. It also
  produces a genuinely confusing failure mode: a mutation check appeared to
  *pass* because the mutation never reached the file under test, which reads as
  "the test does not cover this" rather than "the edit did not happen".

  **What to do instead.** Pass absolute paths to any `[System.IO.File]::*` call,
  or prefer the editing tools, which take absolute paths and report what they
  changed. `Get-Content`/`Set-Content` are PowerShell cmdlets and do honour
  `cd`, so the trap is specific to the .NET APIs — which is why it is easy to
  hit after those cmdlets have worked fine all session.

  Not fixed beyond this: nothing mechanically prevents it. A guard would have to
  inspect shell command text for relative-path .NET calls, which is a linting
  problem on an unbounded surface, so this is recorded as a hazard rather than
  automated away.
