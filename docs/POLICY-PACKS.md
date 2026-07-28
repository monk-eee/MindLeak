# Policy packs: authoring and upgrading

A **policy pack** is immutable, versioned input to drafting a constitution. It
is not law. Nothing in a pack governs anything until a person gives each of its
clauses an explicit disposition and activates the resulting version
([SPEC-CONSTITUTION §6](SPEC-CONSTITUTION.md), [ADR-0026](adr/)).

This page is the authoring and upgrade contract. For what a constitution *is*,
read [SPEC-CONSTITUTION.md](SPEC-CONSTITUTION.md); for the day-to-day verbs, see
[TOOLS.md](TOOLS.md).

---

## The one rule that shapes everything else

**Composition happens at proposal time, never through live inheritance.**

When a clause is adopted it is copied into the project as a local, versioned
constitutional clause carrying the source pack id, version, digest, and clause
key. From then on the local clause *is* the law. Re-publishing the upstream pack
cannot change it.

That is the whole reason packs are immutable and digested: a project's
constitution must be readable and enforceable without reaching for anything it
does not hold. An upstream edit that silently re-wrote local policy would make
every conformance verdict unauditable after the fact.

---

## Authoring a pack

```text
ConstitutionPack {
  id, version, digest, title, description,
  compatible_engine_versions,
  preamble_fragments[], clauses[], conflicts[]
}

PackClause {
  key, kind, title, statement, rationale,
  default_scope, evidence_contract,
  default_consequence, suggested_controls[]
}
```

`common_core_pack()` in [`policy.rs`](../crates/lodestar-core/src/policy.rs) is
the worked example — five principles, keys namespaced `core.*`:

```text
core.evidence         Evidence before claims
core.intent           Preserve project intent
core.safety           Protect the security boundary
core.proportionality  Act proportionally
core.evolution        Evolve policy explicitly
```

Guidance that is not obvious from the schema:

- **Namespace your keys** (`core.*`, `fleet.*`). A key is the identity a local
  clause keeps forever through `pack_clause_provenance`; a collision across two
  packs is a merge nobody asked for.
- **A clause without `default_scope`, `evidence_contract` and
  `default_consequence` is review-only.** That is deliberate, not a defect: a
  clause gains the power to block only through an explicit contract. Ship
  principles without them and let projects complete the contract if they want
  teeth.
- **`suggested_controls` are suggestions.** A control binds to a clause locally
  and versioned; a pack cannot install enforcement into a project.
- **State conflicts explicitly.** Two packs whose clauses contradict must
  declare it. Activation then stops at `needs_human` rather than resolving by
  load order — pack order creates no hidden precedence.
- **The digest covers the content, not the file.** `register_policy_pack`
  rejects a digest that does not match, and rejects duplicate clause keys.

---

## Adopting a pack

```text
register_policy_pack   → the pack exists and its digest is verified
propose_policy_pack    → a proposal per clause; nothing governs yet
review_pack_clause     → Adopted | Tailored | Rejected, per clause, attributed
activate_constitution  → atomic; refuses while any clause is undecided
```

Every clause needs a disposition before activation. There is no bulk accept,
and that is the point: the cost is paid once, deliberately, instead of
discovering later that a project is governed by rules nobody read.

**Rejection is a first-class recorded outcome.** A declined principle is not
forgotten, so the next bootstrap or upgrade does not propose it again — a
project that has decided "not this one" should not be asked every quarter.

**Tailoring is recorded too.** A tailored clause is flagged so a later upgrade
cannot silently undo the local wording.

---

## Upgrading a pack

```text
plan_pack_upgrade(pack_id, to_version)
```

An upgrade is **a proposal, not an event**. It reports added, removed, and
changed clauses. Existing local clauses stay active and enforcing until someone
reviews it, and adopting the result requires an attributed amendment.

So a pack author cannot ship policy into a downstream project by publishing a
new version. The strongest thing a new version can do is ask.

When upgrading:

1. `plan_pack_upgrade` and read the diff.
2. Give each changed clause a disposition, as at adoption.
3. `propose_amendment` → `amend_constitution`, attributed.

A tailored clause appears in the diff as tailored. Accepting the upstream
wording is a choice you make, not a default you inherit.

---

## Extending without authoring a pack

Three supported routes, in increasing order of ceremony:

- **Add local clauses.** `define_goal` plus `complete_clause_contract` when the
  clause should be able to block.
- **Tailor a proposed clause** at review time, keeping its provenance.
- **Publish your local set as a pack** for other projects, once it has earned
  the right to be reused.

---

## What a pack can never do

- Change active local policy after adoption.
- Install enforcement — controls bind locally and are versioned locally.
- Resolve its own conflicts with another pack.
- Activate itself, or supply its own authority. Activation is attributed to a
  person.

These are enforced, not conventions:
`upstream_version_cannot_change_an_adopted_local_clause`,
`a_pack_upgrade_is_a_proposal_that_changes_nothing_by_itself`,
`conflicting_packs_require_an_explicit_rejection_before_adoption`,
`a_tailored_clause_is_flagged_so_an_upgrade_cannot_silently_undo_it`,
`adoption_materializes_a_self_contained_clause_with_source_provenance`,
`validation_rejects_digest_mismatch_and_duplicate_clause_keys`,
`activation_requires_an_attributed_authority`.
