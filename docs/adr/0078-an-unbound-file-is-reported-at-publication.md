# ADR-0078: An unbound file is reported at publication

- Status: Accepted
- Date: 2026-07-31
- Deciders: monk-eee
- Related: [ADR-0009](0009-evidence-backed-conformance.md) (evidence-backed
  conformance), [ADR-0029](0029-proactive-constitutional-advice.md) (proactive
  constitutional advice), [ADR-0034](0034-typed-controls-and-enforcement-ceilings.md)
  (reporting must not enforce harder than its rule),
  [ADR-0065](0065-completion-belongs-at-the-publication-boundary.md)
  (publication is when work becomes visible),
  [ADR-0074](0074-coverage-is-a-prediction-until-conformance-speaks.md)
  (declared coverage remains a prediction)

## Context

Goal-to-code bindings name concrete artifacts. A source file added after those
bindings were recorded starts with no governing goal, and nothing currently
reports that fact unless somebody remembers to run `scripts/binding-audit.mjs`.
Measured on 2026-07-31, the audit reports twelve unbound Rust source files, zero
stale bindings, and zero bindings stranded on superseded goals.

This is not merely catalogue hygiene. Conformance asks which goal governs a
changed artifact. When no goal binds a file, a receipt can describe the change
but cannot establish structural coverage for it. `advise` correctly reports no
governing clause, which is indistinguishable from an intentionally ungoverned
file unless the absence itself is surfaced.

Automatically binding a new file to the task that created it would make the
receipt look stronger by inventing intent from proximity. Refusing publication
would be the opposite error: adding a module before its long-term owner is
decided is ordinary work, not a policy violation.

## Decision

**Canonical publication reports every Rust source file newly added by the
branch and names whether the Intent Plane binds it. The report is advisory: it
neither creates a binding nor blocks publication.**

1. `binding-audit` gains `--new-since <git-ref>`. It asks Git for added paths in
   `<git-ref>...HEAD`, keeps `crates/*/src/**/*.rs`, and compares their artifact
   ids with `goal_code` in the repository's existing read-only Lodestar store.
2. The output is per-file: every added source is labelled `BOUND` or `UNBOUND`,
   followed by a summary. A named path is actionable; a count alone is not.
3. `canonical-push` invokes this mode after a successful push with
   `origin/main` as the comparison ref. Publication is the first point at which
   the branch is visible to the fleet and already owns the observer pattern for
   local Intent Plane state.
4. The observer never changes the push result. If the audit itself cannot run,
   `canonical-push` warns that binding coverage was not observed rather than
   silently implying success.
5. The existing full audit and `--check` behavior remain unchanged.

## Alternatives considered

**Automatically bind the file to the task's goal.** Rejected. A task explains
why one change happened; it does not necessarily define the artifact's durable
owner. Inferring that relationship manufactures constitutional coverage.

**Move to symbol-level bindings.** Deferred. Symbol-level bindings may improve
precision inside large modules, but a new file begins with neither file nor
symbol bindings. It does not solve silent arrival.

**Run the full audit on every publication.** Rejected. Reprinting twelve known
items on every push trains readers to ignore the thirteenth. Comparing the
published branch to `origin/main` isolates the files this publication adds.

**Ratchet only the unbound count.** Rejected. Removing one unbound file while
adding another preserves the count and loses the identity that needs a
decision.

**Run at commit time or only in CI.** Rejected. A commit is still private and
may be amended; CI has no durable developer-local Intent Plane to query.
Publication is both visible and attached to the real local ledger.

## Consequences

- A newly published Rust module that no goal binds is named immediately.
- The existing twelve-item backlog is not repeated when a branch adds no Rust
  source file.
- The report remains evidence, not authority. A person still decides whether
  and where to bind each file.
- Developers whose local ledger or compatible Node runtime is unavailable see
  an explicit observer warning, while their already-successful push remains
  successful.
