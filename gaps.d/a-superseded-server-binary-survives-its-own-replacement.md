- **A superseded server binary survives its own replacement while the process is
  running, and keeps serving the defect.** `install-servers.mjs` renames the
  running binary to `<name>.exe.<timestamp>.old` and writes the new one in its
  place, which succeeds on Windows even with the file open — but the live
  process keeps executing the old image, so a fixed binary on disk changes
  nothing until every server process is restarted. Measured here: four servers
  started ~13 minutes before the fix went on disk continued to advertise the
  broken schema afterwards, and their `.old` files could not be deleted
  (`Access to the path ... is denied`) until those processes were stopped. The
  installer's closing advice to "restart the MCP servers" is therefore load
  bearing rather than housekeeping, and the leftover `.old` files are a reliable
  signal that a pre-fix process is still live. Fixed this run only in the sense
  that the processes were stopped and the residue collected.
- **NARROWED 2026-08-29: that signal is now reported instead of discarded, but
  the underlying survival is unchanged and OPEN.** `pruneSupersededInstalls`
  already learned exactly what this fragment describes every time it ran — a
  set-aside binary that will not delete is a process still holding it — and threw
  it away in a bare `catch`. It now returns `{ pruned, held }`, and both the
  install and `--prune` paths say that a held binary means those servers are
  still running the code the install replaced, so the change is not live until
  they restart. The unconditional "restart the MCP servers" advice is therefore
  no longer the only thing standing between a shipped fix and a process quietly
  serving the defect. Two limits worth stating plainly: this is evidence only on
  Windows, because Unix unlinks a running binary happily and leaves no residue,
  so a quiet result there means "no evidence", never "nothing is running"; and
  nothing here stops or restarts anything, so the operator still has to act. The
  root — a live process keeps executing the old image — is inherent to replacing
  a running binary and is not fixable in the installer.
