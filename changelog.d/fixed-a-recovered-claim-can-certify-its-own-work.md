- A recovered claim can now certify the work it was recovered for (ADR-0076).
  Conformance bounds evidence by a live claim, but compared it against the most
  recent window rather than the one that authorised the work. Recovery
  necessarily happens *after* the work it exists to rescue, and
  `recover_claim` opened a fresh window, so the rescued work sat before the only
  window the task could show. Every route back to a live claim behaved the same
  way — `claim_task` and `recover_claim` both set the start to now, and
  `renew_lease` refuses a lapsed lease — so there was no ordering of calls that
  could certify a recovered claim at all.
  Reproduced on `task:36fa0badd713`, whose commit `64fb56b3` is on `main`: the
  bundle was exactly right, one commit and three changed nodes with no
  contamination, and conformance still answered "evidence interval falls outside
  the live claim". Such tasks could only be closed by a human `resolve_task`.
  The floor is now the earliest window in the audited recovery chain leading to
  the current owner, read back from the transfer history rather than stored
  again — it was already reconstructing the interrupted window from the log.
  The guarantee is unchanged: every link is a transfer that named the owner it
  took from, so an identity that never held the task inherits nothing, and
  evidence from before any claim is still refused. Committing before claiming
  still cannot be certified, and that is asserted by its own test rather than
  left as a side effect.
