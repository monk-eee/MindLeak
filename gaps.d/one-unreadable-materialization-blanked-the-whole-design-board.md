- **One unreadable materialization blanked the whole Design Board — FIXED.** —
  `DesignBoardController.refresh()` fanned `design_promotion` out over every
  materialized design with `Promise.all`, so a single rejection rejected the
  batch, `provider.update` never ran, and the view silently kept stale contents
  behind one error toast. — Medium impact: the board looked merely out of date
  rather than broken. — Fixed this run: `Promise.allSettled`, with each failed
  lookup logged against its design id and the remaining rows still rendered.
