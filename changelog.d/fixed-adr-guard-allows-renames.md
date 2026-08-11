- **The ADR number guard can tell a rename from a collision.** Correcting the
  filename of a decision that had already landed was impossible: the guard's
  retitle allowance requires the old slug to live only on refs the branch
  answers for, which a landed ADR can never satisfy — its old name is on `main`
  and on every branch precisely *because* it landed. The guard therefore read
  the decision as its own rival and advised renumbering it, which for an
  accepted ADR means rewriting its identity, its cross-links, and every commit
  message citing it in order to fix a typo. A slug that is on the integration
  branch and has been replaced by this branch is now recognised as this
  decision's former name. The allowance stays narrow: an unlanded ADR on a
  sibling branch is still a rival, and a renumber onto a landed decision is
  still refused, because that branch has replaced nothing. `git mv` now counts
  as replacing the old slug too — git reports it as a rename rather than a
  deletion, so the previous check, which counted only deletions, missed the most
  natural way to rename a file.
