- **The ADR number guard can tell a rename from a collision.** Correcting the
  filename of a decision that had already landed was impossible: the guard's
  retitle allowance requires the old slug to live only on refs the branch
  answers for, which a landed ADR can never satisfy — its old name is on `main`
  and on every branch precisely *because* it landed. The guard therefore read
  the decision as its own rival and advised renumbering it, which for an
  accepted ADR means rewriting its identity, its cross-links, and every commit
  message citing it in order to fix a typo. A staged rename that keeps the ADR
  number is now recognised as one decision being renamed rather than two
  contesting a number. A rename that *changes* the number is still refused: that
  is a renumber onto somebody else's claim, which is the collision the guard
  exists for.
