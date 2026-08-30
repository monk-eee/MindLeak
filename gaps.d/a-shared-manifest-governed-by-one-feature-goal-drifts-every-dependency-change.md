- **The workspace root `Cargo.toml` is governed by a single feature's goal, so
  every dependency addition drifts no matter what work it serves. MEASURED,
  LEFT FOR LATER — the repair is a decision, not a patch.** `advise` on
  `artifact:Cargo.toml` returns exactly one governing clause:
  `goal:adr-0030-unique-per-process-agent-identity@constitution:v4`, mode
  `governed`. ADR-0030 is about a session resolving one stable opaque identity
  across MindLeak attribution and Lodestar ownership. It has nothing to do with
  `[workspace.dependencies]`, which is what almost every change to that file
  actually touches — the manifest currently declares 31 of them.

  The binding is not wrong because the goal is unimportant; it is wrong because
  the file is *shared* and the goal is *narrow*. The root manifest is where
  every crate in the workspace declares a dependency it wants to share, so its
  writers are unrelated to each other and to ADR-0030: `git log --since='90
  days ago' -- Cargo.toml` is 33 commits, and MindLeak's own
  `check_overlap(["Cargo.toml"])` reports **10 distinct agent session
  footprints** on it. Every one of those authors, unless they happened to be
  doing agent-identity work, changed governed code under a goal their task did
  not serve.

  **Impact, and why it is worse than a stray finding.** Coverage is a
  *prediction* — it can only be declared at claim time, in
  `task_claim(also_serves=[...])`, and the engine explicitly refuses to accept
  it afterwards ("conformance has already judged `task:...`; coverage declared
  after a finding is raised is a rationalisation, not a plan", ADR-0074). So
  the cost is not a warning you can clear up at the end: by the time
  `canonical-push` runs `check_conformance` and tells you, the verdict is
  already earned and the task can only complete `drift`. The only way to avoid
  it is to have run `advise` over the root manifest *before* claiming, and to
  have guessed that a build manifest would be governed by an agent-identity
  goal. Nothing in `AGENTS.md`'s before-you-write checklist points at that, and
  the goal's own `statement` gives a reader no reason to expect it.

  That is the expensive part. A `drift` verdict is supposed to mean "governed
  code changed outside its goal", and a reviewer is supposed to take it
  seriously. When the most common way to earn one is "you added a dependency",
  the verdict stops carrying information, and agents learn to discount it —
  the same failure `gaps.d/` itself suffered when it mixed real defects with
  limitations (`knowledge:beb8ef45e46f`), and the same one
  `docs/KNOWN-LIMITATIONS.md` records for the misleading Postgres ceiling: a
  signal that fires for the wrong reason is worse than no signal, because it
  trains people to ignore the times it fires for the right one.

  **Observed on `task:858cefa3cf44`** (ADR-0143 slice 1, PR #856), which added
  `deadpool-postgres` to `[workspace.dependencies]`. Every other file in that
  change was correctly governed by `goal:ackplane-federation-service@constitution:v4`
  and covered by the task; the root manifest alone produced
  `governed code changed without a covering task:
  goal:adr-0030-unique-per-process-agent-identity@constitution:v4`, and the
  task completed `drift` on that one finding. Note that the *sibling* manifests
  are not bound at all — `advise` on `crates/ackplane-server/Cargo.toml` and on
  `Cargo.lock` returns no governing clause — so this is one file's binding, not
  a policy about manifests.

  **Fixed 2026-08-30: unbound (option 1).** `constitution_define(action="unbind",
  goal_id="goal:adr-0030-unique-per-process-agent-identity@constitution:v4",
  node_ids=["artifact:Cargo.toml"])` removed the binding; `advise` on
  `artifact:Cargo.toml` now returns "no active clause governs this change" and
  `binding-audit.mjs` is unaffected, exactly as predicted, since it measures
  Rust source coverage only. Checked before unbinding, per option 3's own
  caveat: ADR-0030's text (`docs/adr/0030-discrete-per-agent-identity.md`)
  never mentions `Cargo.toml`, `[workspace.dependencies]`, or any manifest
  entry, so nothing in that decision actually depended on the binding. Option
  2 (rebind to a build-scoped goal) was not pursued: nothing here needs a new
  goal to exist, and the manifest is simplest left ungoverned rather than
  invented a governing goal for the sake of having one.

  Three candidate repairs were named before this fix; kept for the record:

  1. *Unbind it.* Defensible: `scripts/binding-audit.mjs` measures coverage of
     Rust **source** files, so a manifest is outside the bar it enforces, and
     unbinding costs nothing that audit was protecting.
  2. *Rebind it to a goal that is actually about the build.* There is no
     obvious existing candidate — the closest,
     `goal:platform-agnostic-operation@constitution:v4`, governs shell
     portability rather than dependency choice — so this route means defining
     a new goal, which is exactly the kind of thing
     `goal:evolve-policy-explicitly@constitution:v4` exists to make deliberate.
  3. *Leave the binding and narrow the clause*, if some part of ADR-0030
     genuinely does depend on a manifest entry. Worth one look before choosing
     1 or 2: if it does, the honest fix is to say so in the goal's statement so
     the next reader is not surprised.

  Whichever is chosen, the general shape is the thing to take away, and it is
  not specific to this repository. **PORTABLE: binding a shared, high-churn
  file to one feature's goal makes that goal's enforcement fire on unrelated
  work forever. Scope a governance binding to who *writes* a file, not to
  whichever change happened to create it** — the binding here was almost
  certainly applied by an agent that touched the manifest while implementing
  ADR-0030, which is a description of one commit's history rather than of the
  file's ongoing ownership.
