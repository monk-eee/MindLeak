- **`fleet_view` now surfaces a stale one-way wait, not only a wait cycle.** A
  task parked on `ask_question` addressed to a peer who never answers was flagged
  by nothing — only a mutual wait cycle was — so it could sit until the seven-day
  parking grace released it. `fleet_view` now reports a one-way wait whose
  addressee has gone quiet (no live claim and no session declared since the
  question was asked, past a grace) as a distinct, weaker signal than a cycle:
  the remedy is not to break a deadlock but to redirect the question to someone
  who is here. Advisory only, like the rest of the view.
