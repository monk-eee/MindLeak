- A decider label that is one edit from one already in the ledger is now flagged
  at the moment it is recorded. Attribution labels are free text and
  deliberately unverified — ADR-0071 is explicit that they are attributed, not
  authenticated — so a typo cannot be detected by checking it against anything.
  It can only be compared with what is already there.
  This matters because the moment of writing is the *only* moment a slip is
  fixable. Afterwards every verb that could correct one refuses by design:
  `attribute` answers "a recorded human act is not rewritten here" and `reopen`
  answers "a recorded decision is not undone here". Both refusals are right —
  an agent that could rewrite who decided something would make attribution
  worthless — but together they mean a mistyped name is permanent.
  Measured on the live ledger: 73 rows carried 70 decisions by `monk-eee`, one
  by `Lyndon Swan`, and one by `monk-ee`, which is a typo for the first and can
  never be corrected. This is what would have caught it.
  The check is advisory and never refuses. Two people can legitimately have
  similar names, and rejecting a genuine new reviewer to catch a typo is the
  worse failure; the response carries the recorded label, what it resembles, and
  what to do about it, and the decision itself proceeds untouched.
