- A MindLeak database that cannot be opened because another process holds it now
  says which step lost the race — enabling WAL, applying the schema, or migrating
  — and that a peer process is opening or migrating it, instead of reporting a
  bare "database is locked". The schema and migration steps also retry while the
  database is busy, which previously only the WAL step did, and all three share
  one time budget.
- That budget is now genuinely bounded. Each attempt is granted only the time
  remaining, so a single SQLite wait can no longer start just before the deadline
  and run a full timeout past it — which is how an open meant to give up after
  five seconds could take closer to ten under load.
