- **Three reports that could not be run now run.** `board-health`,
  `stranded-report` and `design-audit` each resolved the server binary with
  their own release-only path instead of the shared `resolveServer`, which
  honours `LODESTAR_MCP_BIN`, accepts a debug build, and returns nothing rather
  than a path that is not there. On a debug build — the normal state for a
  developer — `board-health` died with an unhandled `spawn ENOENT` and
  `stranded-report` with it. So the two reports written to explain the board's
  stranded claims could not be executed by most of the people they were written
  for, which is why the board's state kept being reconstructed by hand. They now
  resolve like every other script and say plainly when no binary exists. Running
  them immediately separated four lapsed claims into two whose shipping commit
  can be named and two that look genuinely unfinished — a distinction that had
  been made by guesswork.
