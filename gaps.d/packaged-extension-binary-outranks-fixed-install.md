- **A packaged extension binary outranks every other install, so a stale VSIX
  silently serves stale servers — PARTIALLY FIXED, OPEN.** Observed while fixing
  `constitution_define`'s array schema: the fix was committed, merged, rebuilt
  and deployed to `~/.mindleak/bin`, yet clients kept failing, because
  `resolveBinaryPath` (`editors/vscode/src/util.ts`) prefers
  `<extension>/bin/<server>` ahead of the shared install, and the installed
  0.1.5 extension carried binaries packaged before the fix. Impact: the
  documented remedy for a stale server — rebuild and rerun
  `scripts/install-servers.mjs` — cannot reach the binary VS Code actually
  launches, and nothing reported the mismatch; the packaged copy has no version
  or provenance marker to compare against. ADR-0073's reasoning for ranking the
  shared install above a worktree `target/` (avoid stale per-worktree builds)
  applies verbatim to the packaged copy, which is staler still and is ranked
  higher.

  **Fixed:** `resolveBinaryPathDetailed` (`editors/vscode/src/util.ts`) now
  names which of the six candidates resolved (`explicit-config`, `packaged`,
  `shared-install`, `workspace-release`, `workspace-debug`, `fallback`)
  alongside the path; `resolveBinaryPath` is unchanged, so every existing
  caller keeps its old behavior. The extension's own "Connected to X" output
  lines for both planes now include the resolved source, so a stale packaged
  binary silently outranking a rebuilt one is visible in the output channel
  instead of invisible.

  **Still open:** the packaged copy still carries no version or provenance
  marker of its own, so nothing can yet warn *before* connecting that a
  packaged binary predates the extension's own fix — a user has to read the
  new source label and already know to be suspicious of `packaged`. Recording
  the source commit alongside a staged binary at package time, and warning
  when it predates the extension's own commit, remains unimplemented.
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
