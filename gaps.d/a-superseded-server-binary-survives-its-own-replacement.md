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
