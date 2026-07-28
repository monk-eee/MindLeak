- **The command palette lists 8 commands instead of 34.** Typing "MindLeak"
  returned every contributed command, and 26 of them could do nothing from
  there: 20 are driven by a row in a view — from the palette they only answer
  "Run *X* from an Intent Board row" — and 6 are per-view refresh, which belongs
  on the view title where it already is. Those 26 are hidden from the palette
  and unchanged everywhere else: view title buttons and right-click menus behave
  exactly as before. What remains is the set that does something when invoked by
  name: prune, reconcile, export, back up, reset, ingest the active file, next
  task, sync ADRs. A test enforces the rule rather than the list, so a new
  row-driven command is caught instead of quietly rejoining the wall, and a
  budget assertion fails if the palette grows past ten without anyone deciding
  it should.
