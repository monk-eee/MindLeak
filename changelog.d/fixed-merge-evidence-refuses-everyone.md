- **`merge_evidence` could not succeed for anyone, in three independent ways.**
  ADR-0058's promise is that a merge which passed review and CI is stronger
  evidence than a hand-assembled bundle. The verb shipped unable to accept a
  single one. Found by being its first user, trying to certify five tasks that
  were parked precisely because their work had merged and could not be proven.
  1. **It compared the raw session token against a resolved agent id.** The
     dispatch read `req_str(args, "session_id")` and handed that straight to the
     facade as the agent, while `resolve_agent` is a pass-through and
     `task.owner` is a `session:v1:` id. The comparison could never match, so
     every caller was refused — and the message accused the rightful holder of
     claiming credit for someone else's work. `merge_evidence` is now in
     `requires_session`, so `bind_session` resolves the token and injects the
     agent the facade actually compares.
  2. **It measured reachability against the local `main`.** Under ADR-0038
     nobody checks `main` out, so it sits wherever the clone left it — measured
     294 commits behind here, which refused a commit that was demonstrably on
     the integration branch. The ref is now resolved, preferring `origin/main`
     and falling back to `main` where there is no remote. That is still not the
     "whatever branch I am on" trust ADR-0058 removes: it is the protected
     branch's remote-tracking ref, which is the thing the ADR is about.
  3. **It could not see what a merge changed.** `git show --name-only` prints
     nothing for a two-parent commit unless asked with `-m`, `-c` or
     `--first-parent`, so the verb whose whole premise is "a merge is evidence"
     read an empty file list for exactly the commits it exists for, and rejected
     them as touching nothing in scope. Now `diff-tree -m --first-parent`, which
     is also correct for an ordinary commit, so one command serves both shapes.
  The tests missed all three for one reason worth keeping: the fixture built a
  real merge but captured the *feature* commit and named it `merged`, while its
  doc comment called it "the merge commit on `main`" — the shape the tool asks
  callers for was the one shape never exercised. The fixture now returns both,
  and a test asserts the merge really has two parents before verifying it.
  Verified end to end against the live ledger: `merge_evidence` now returns a
  bundle naming the merge and all five changed nodes.
