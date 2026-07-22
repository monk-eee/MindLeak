# RATIONALE — why this repo is shaped the way it is

Every other doc says *what* the convention is; this one defends *why it earns its
place*. If a file here looks like ceremony, read the matching section before
deleting it.

> **Optimise for the reader who arrives cold** — a new contributor, an AI agent,
> you-in-six-months. Every file exists to shrink the gap between landing in the
> repo and taking the correct next action. A convention that doesn't shrink that
> gap is ceremony; cut it.

## Why split docs instead of one big README?

Different readers arrive at different moments with different urgency. The README
is a **router, not a warehouse**: it answers "where do I go?" in one screen and
hands off. [DEVELOPERS.md](DEVELOPERS.md) answers "how do I run it"; [AGENTS.md](AGENTS.md)
answers "what must I not break"; [docs/SPEC.md](docs/SPEC.md) answers "what is the
design contract". Recognisable names (`README`, `CONTRIBUTING`, `SECURITY`,
`CODEOWNERS`, `AGENTS.md`) are a zero-cost index for humans, GitHub, and agents.

## Why a separate AGENTS.md?

The README *orients*; [AGENTS.md](AGENTS.md) *constrains*. An agent needs the
load-bearing rules stated imperatively — "zero-token write path", "effective
weight is derived, never stored", "decay is the point, tune half-lives not the
mechanism". Mixing those into welcoming prose makes both worse, and agents read
`AGENTS.md` by convention, so the constraints land in context automatically.

## Why pre-commit hooks? CI catches it anyway.

CI catches it *eventually* — after a 10-minute round-trip and a `fix lint` commit
that pollutes history forever. A hook catches it in two seconds, locally, before
it exists in the record. Hooks convert the floor from *goodwill* into *physics*:
"please run `cargo fmt`" depends on memory; a hook that blocks the commit does
not. We never bypass with `--no-verify`.

## Why `.gitattributes`?

Without it, line endings are a function of each contributor's laptop —
nondeterministic and decided in the wrong place. A CRLF checkout can make a
one-line change diff as "whole file modified" and can break the shell hook
scripts outright (`bad interpreter: /bin/sh^M`). [.gitattributes](.gitattributes)
moves the decision into the repo, once, for everyone: `* text=auto eol=lf`.

## Why ADRs? Why not a code comment?

Code shows *what*; an [ADR](docs/adr/) shows *why*, and the *why* is the
expensive, perishable knowledge. [ADR-0002](docs/adr/0002-sqlite-decay-over-vector-llm.md)
exists precisely so the next contributor doesn't "simplify" the decay engine away
and reintroduce graph rot. A comment gets deleted in the refactor that needs it
most; an ADR is dated, durable, and survives the code it describes.
