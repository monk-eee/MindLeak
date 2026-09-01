- **A Lodestar task whose real deliverable is a `constitution_define` call — not a
  git commit or a test execution — has no evidence shape that can complete it,
  even when the work is genuinely done and independently verifiable. MEASURED
  2026-09-02, OPEN.** `validate_evidence_shape`
  (`crates/lodestar-core/src/facade/conformance/evidence.rs`) requires every
  `changed_node_ids` entry to carry provenance whose `relation` is `"modified"`
  or `"refactored"` and whose `source_id` is one of the bundle's own
  `commit_ids`/`execution_ids`. A `constitution_define(action="bind", ...)` call
  produces neither: it writes directly to `spec.db` and leaves no commit, no
  test run, nothing MindLeak would ever ingest as an execution. There is
  therefore no way to construct a `changed_node_ids`/`provenance` pair that
  `task_transition(to="complete")` will accept for this class of work, no
  matter how real or how verified the underlying fix is.

  Measured concretely: closed a genuine, measured binding-coverage gap this
  session (`node scripts/binding-audit.mjs --check` went from 40 unbound
  source files to 0, re-verified by re-running the same command and reading
  its exit code) by binding each file to the goal already governing its
  siblings. Created `task:58a6563a52a5` to track it, claimed it, did the work,
  then could not complete it:
  - Submitting `changed_node_ids` for the 40 bound files with `provenance`
    linking each to the agent directly (`relation: "bound"`, `source_id:
    "agent:..."`) was refused: *"changed node ... lacks mutation provenance"* —
    the relation name is not in the accepted set, and even the accepted
    relations require a `commit_id`/`execution_id` source, not a bare agent.
  - Submitting an empty `changed_node_ids`/`commit_ids`/`execution_ids` bundle
    was refused differently: *"evidence interval falls outside the live
    claim"* — an evidence bundle asserting nothing does not pass the window
    check either.
  - `resolve` (human-accepts to done) requires a `human` reviewer label
    distinct from the acting agent; fabricating one to close your own task
    would misrepresent the record as human-reviewed when it was not, which is
    the exact laundering ADR-0048/ADR-0009 exist to prevent elsewhere.
  - `abandon` was the only transition that succeeded, and it is the wrong
    word for what happened: the task's acceptance criteria were fully met and
    independently verified (`binding-audit.mjs --check` exits 0), but
    "abandoned" reads as "this was given up on," not "this was done outside
    the tool's own proof mechanism." The real fix survives only because it was
    separately recorded via `record_knowledge` (`knowledge:c4a0edbf8e6f`)
    before abandoning — an agent that skipped that step, or that did not know
    to, would have left the fix invisible: real in `spec.db`, absent from the
    task ledger, and named nowhere.

  **Impact.** Any future governance-only fix — binding a newly split module,
  unbinding a manifest from too narrow a goal (as `a-shared-manifest-...md`
  records), retiring a stranded binding — hits this exact wall if it is
  tracked as a claimable task at all. The honest options right now are: (a) do
  not create a formal task for pure `constitution_define` work, and rely on
  `record_knowledge` alone for durability, or (b) create one anyway and accept
  that its only true closing state is `abandon`, which under-describes what
  happened and would read as a red flag to anyone auditing the board later.
  Neither is satisfying, and this fragment does not choose between them.

  **Not fixed here — this is a decision, not a patch.** Two directions worth
  naming: either broaden `validate_evidence_shape` to accept a governance-only
  provenance shape (e.g. a `"defined"` relation sourced directly from the
  agent, distinct from a code mutation, so `changed_node_ids` can name a
  *binding* changed rather than only a *file*), or add a tenth
  `task_transition` verb for "done, but by an action this evidence contract
  does not model" that is honest about the distinction `resolve`/`abandon`
  currently blur. Either changes the evidence contract's shape, which is
  exactly the kind of thing ADR-0009 exists to keep deliberate rather than
  patched around.
