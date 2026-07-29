- **A task's evidence-window continuity is now derivable from the log.**
  `claim_window()` replays the recorded transitions to compute the lapses and
  unleased seconds that `tasks.claim_lapses` and `tasks.unleased_seconds`
  currently carry as running totals (ADR-0064 decisions 5 and 6). Nothing is
  removed yet: the derivation is asserted **against** those columns across
  every shape they take — never claimed, clean claim with renewals, one lapse,
  two lapses, handover to a new owner, and park/resume — because that agreement
  can only be proved while both still exist. After the columns go there is
  nothing left to disagree with.
  The genesis event now carries the counters it imported. Deriving continuity
  from in-log transitions alone would report zero lapses for any window that
  opened before the log did, and under ADR-0048 a window with no lapses may
  certify itself as `aligned` — so a migration would have quietly laundered a
  discontinuous window clean and handed out a receipt for work with holes in
  it. Derivation is therefore genesis seed plus in-log transitions, with a test
  that a pre-log window keeps the three lapses it had and accumulates a fourth
  on top.
