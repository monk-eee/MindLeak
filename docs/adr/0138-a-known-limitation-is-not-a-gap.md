# ADR-0138: A known limitation is not a gap

- Status: Accepted
- Date: 2026-08-29
- Deciders: MindLeak maintainers
- Related: [ADR-0056](0056-the-changelog-is-assembled-not-edited.md)
  (fragments rather than a shared append-only file — the mechanism `gaps.d/`
  borrowed, and the one this decision partly reverses),
  [ADR-0058](0058-work-that-shipped-must-leave-the-board.md) (a catalogue that
  keeps entries nothing can close stops describing outstanding work)

## Context

[`gaps.d/`](../../gaps.d/) is this repository's defect catalogue: one Markdown
fragment per known gap, so two branches recording two unrelated observations
never write the same path. That mechanism works and is not in question here.

What it accumulated is. At 36 fragments, roughly half described something no
commit could ever close:

- **Deliberate design positions.** `active_knowledge` decays on a timer rather
  than on repository evidence *because that is what a decaying memory plane is*.
  Evidence is bounded by the claim window *because ADR-0009 says so*. The recall
  floor cannot rank, and raising it was measured making recall worse. These are
  trades that were made on purpose, with the measurement that justified them.
- **Defects in tools we do not own.** Four fragments recorded ways the Unit Test
  MCP extension misreports results; three recorded VS Code client behaviour.
  None is reachable from this codebase at all.

Both kinds were filed correctly under the standing instruction to never silently
drop a finding, and the evidence in them is good. The problem is what a reader
does with the file. `gaps.d/` is read — by agents and humans — as a list of
outstanding jobs. An entry with no job in it does not merely sit there inertly:

1. `scripts/gaps.mjs --triage` exists to report ageing, untracked fragments as an
   honest measure of neglect. A limitation is untracked and ages forever, because
   there is no repair for it to be waiting on. Half the backlog was permanent
   noise, which is precisely what stops anyone reading the number.
2. An agent told to "clear the gaps" reaches for the only two moves available:
   delete entries that are still true, or build something to satisfy an entry
   that was never asking for anything. Both are worse than leaving it alone.
3. The half of the catalogue that *does* name fixable defects is devalued by
   being mixed in with the half that does not.

The failure mode here is the same one this repository already recognises in
`merge-audit` and in conformance verdicts: a signal that cannot distinguish two
different states will be believed to mean the more alarming one, and then
discounted entirely once readers learn it over-reports.

## Decision

**Split the catalogue by whether a fix is possible here, not by subject matter.**

- [`gaps.d/`](../../gaps.d/) holds only observations where something in this
  repository could be changed to make the observation stop being true. A
  fragment is an assertion that a fix exists to be written. It is closed by
  deleting it in the commit that fixes it.
- [`docs/KNOWN-LIMITATIONS.md`](../KNOWN-LIMITATIONS.md) holds everything else,
  in two sections: *Design positions* (working as intended, recording what the
  alternative would cost) and *External tools* (recording the version measured,
  which is what makes the entry falsifiable later).

The filing test is one question, stated in both `AGENTS.md` and `DEVELOPERS.md`:
**is there anything in this repository I could change to make this stop being
true?** Yes is a gap; no is a limitation.

**Movement in both directions is expected and cheap.** A dependency ships a fix,
or an ADR reverses a design position — the entry moves to `gaps.d/` and becomes
a defect. A gap turns out to be inherent — it moves the other way. Neither
direction needs an ADR of its own.

**The evidence standard does not change.** A limitation entry carries the same
measurement, location, and impact that a gap fragment does. Only the implied
call to action differs. Nothing is deleted by this split: all 18 moved entries
keep their text verbatim.

## Consequences

- `gaps.d/` becomes readable as what it claims to be. It went from 36 fragments
  to 18, and every remaining one names something that could be fixed.
- `--triage`'s backlog numbers describe real neglect rather than a floor of
  permanent entries, which is the only condition under which the number is worth
  printing.
- **One shared append-only file comes back, deliberately.** `KNOWN-LIMITATIONS.md`
  is exactly the shape ADR-0056 removed, and it will conflict when two branches
  add an entry at once. That is accepted rather than overlooked: limitations are
  added rarely (18 accumulated over the project's life, against a changelog
  fragment on nearly every pull request), and the value of reading them as one
  ordered document outweighs an occasional conflict. If the rate rises, this
  decision should be revisited and the file split into fragments — the same
  reasoning ADR-0056 applied, with a different input.
- A new judgement call enters the filing path, and it can be got wrong. The cost
  of a wrong call is low and self-correcting: a misfiled limitation shows up in
  `--triage` as an ageing untracked fragment, and a misfiled gap shows up as an
  entry in a "cannot be fixed" document that somebody then fixes.
- The catalogues remain non-authoritative. Neither is re-evaluated by anything,
  so both can go stale silently; verify against the tree before acting on either.
  Silence in both is not an all-clear — it means nobody has looked.
