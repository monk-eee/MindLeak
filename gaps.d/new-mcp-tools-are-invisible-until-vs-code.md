- **A new MCP tool is invisible until VS Code reloads, and refreshing the
  installed binary does not change what is running — NARROWED 2026-08-14, still
  OPEN.** Two of this fragment's claims are contradicted by the tree and are
  corrected here rather than left to mislead a reader into rebuilding work that
  exists. The residual is smaller than the heading above it used to claim, and
  it has moved.

  **No longer true: the running servers lock the binaries.** The original text
  said `cargo build --release` fails with `Access is denied (os error 5)`
  because the servers hold the files open. Measured 2026-08-14 with eight server
  processes live: `target/release/lodestar-mcp.exe`,
  `target/release/mindleak-mcp.exe` and `~/.mindleak/bin/lodestar-mcp.exe` each
  opened `r+` without complaint. Two things removed the lock — the fleet
  executes sha-suffixed copies (`lodestar-mcp-1551270.exe`) rather than the
  build output, and `scripts/install-servers.mjs` renames the destination aside
  before copying, for the reason its own comment gives: "Windows refuses to
  overwrite a live executable but does allow renaming one".

  **No longer true: there is no in-band signal.** `stale_build` is threaded from
  `main.rs` through `server.rs` into `tools/mod.rs` on both planes and returned
  from `open_session`, the one call every agent makes first, with
  `build_identity.rs` producing "running a stale build of this checkout: binary
  was built from `<sha>`, HEAD is `<sha>`". A second notice, `replaced_binary`,
  covers the swap that `stale_build` structurally cannot see.
  Both are tested. It fired during the session that wrote this: a binary built
  from `ecac179523e5` answering for a checkout at `ee94dc73`.

  **Still true, and why this fragment stays:** a tool added during a session
  cannot be exercised in that session. VS Code caches the advertised tool list,
  so a new verb is absent from `tools/list` until the window reloads, and
  reaching it by name still lands in a server process that predates it.

  **New, and the sharper half: installing a fresh build silently changes nothing
  that is running.** `install-servers.mjs` writes the *unsuffixed*
  `executableName(name)` into the shared install directory, while the live
  processes execute sha-suffixed copies sitting beside it. On 2026-08-14
  `~/.mindleak/bin/lodestar-mcp.exe` was almost four hours newer than the
  `lodestar-mcp-1551270.exe` every server was actually running. The install
  succeeds, reports success, and the fleet goes on serving the older build —
  which is precisely what `stale_build` then reports, one layer too late to have
  prevented it.

  **No longer true: nothing collects the copies left beside them.** This
  fragment reported roughly 70 MB of `.old` and `.superseded` binaries that
  nothing reclaimed; measured on the same day at eight copies and 68.2 MiB. The
  cause was narrower than "nothing collects it": `pruneSupersededInstalls`
  already existed, but it matched only `.old` — the name `installOne` writes —
  while a hand deploy renames the live file to `.superseded` for the same lock
  reason, and it ran only as a side effect of a full install, which a deploy
  that copies a build in by hand never performs. It now takes both suffixes and
  is reachable on its own as `node scripts/install-servers.mjs --prune`.

  Left open because the remaining fix is a decision, not a patch: the
  sha-suffixed copy is an operator workaround for a lock the installer already
  solves by renaming, so what needs settling is which name the registration
  should spell, and who is allowed to change it.
