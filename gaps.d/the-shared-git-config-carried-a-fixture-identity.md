- **A test-fixture identity authored 204 real commits, and nothing noticed —
  MEASURED, LEAK FIXED AND GUARDED, RESIDUAL OPEN.** On 2026-07-30 the shared
  repository config
  (`C:/Users/lyndonswan/Repos/MindLeak/.git/config`) held `user.name = "Sha
  Test"`, `user.email = "sha@example.invalid"`. That file is the *common dir* for
  all 76 linked worktrees, and a repository-level override outranks a global
  one, so every agent in every worktree committed under it while the correct
  global identity (`Lyndon Swan <lyndonswan@microsoft.com>`) sat unused. 204
  commits on `origin/main` since 2026-07-01 carry the fixture identity,
  including one merged the same day.
  — *The impact.* A commit records who did the work, and more than politeness
  rests on it: `git blame` credits nobody, the commit cannot be linked to a
  GitHub account, and ADR-0038's premise that a commit id is *evidence-bearing*
  is undermined at the git layer, beneath every receipt built on top of it. The
  failure is silent by construction — git commits happily under any identity and
  nothing downstream ever questions the author.
  — *Where it comes from.* `git config user.email <x>@example.invalid` writes
  the **local repository** config by default, so a git fixture whose `GIT_DIR`
  or `GIT_COMMON_DIR` is still inherited from the real repository configures the
  real repository. This is a **regression**: the same contamination was cleaned
  up once before (PR #21, "local fixture identity overrides removed") and came
  back.
  — *Why the existing defences missed it.* They are all in the wrong place, and
  each is individually correct. `scripts/script-tests.mjs` scrubs the six `GIT_*`
  variables for the whole node suite, and its comment records the earlier
  incident that earned it; the vitest fixtures and the Rust fixtures each scrub
  `GIT_COMMON_DIR` themselves. Every *committed* suite is careful. The hole is
  ad-hoc scratch scripts under `target/tmp/`, which agents write constantly to
  drive git fixtures and which inherit no such discipline. The offending string
  appears in no tracked file, which is exactly why grepping the repository never
  found it and it survived for a month.
  — *Fixed this run.* The override is unset (verified from a second worktree),
  and `scripts/commit-identity.mjs` now refuses at pre-commit when the effective
  author sits in a domain the IETF reserves for documentation and testing (RFC
  2606, RFC 6761). Those domains can never receive mail, so they are never a
  real contributor — which makes the rule unambiguous rather than a heuristic
  about what a name ought to look like. It checks the *effective* identity,
  honouring the `GIT_AUTHOR_EMAIL`/`EMAIL` overrides that outrank config, and
  the refusal names the offending address, the file holding it, and the two
  commands that clear it.
  — *Deliberately not done: rewriting the 204 commits.* Their authorship is
  wrong and stays wrong. Rewriting published history is forbidden by ADR-0038
  and would replace evidence-bearing commit identities wholesale — a worse
  defect than the one being repaired, and one that would invalidate every
  conformance receipt keyed on those shas. The guard is forward-looking on
  purpose.
  — *Residual.* A commit-time guard cannot catch a fixture that writes the
  config without committing, so the contamination can still be introduced and
  will simply be caught at the next commit rather than prevented. Scrubbing
  `GIT_*` centrally for scratch scripts is not possible by construction, since
  they are untracked and ad hoc.
