### Added

- `stage-native.mjs` now detects a staged MCP server binary that was not
  rebuilt at the commit it is being packaged alongside: it reads the
  binary's own reported build sha, compares it against the current
  checkout's `git rev-parse HEAD`, and warns loudly at packaging time rather
  than shipping the mismatch silently. A `bin/build-info.json` sidecar
  records each staged server's identity. The extension's own connection log
  lines for both planes now also show the connected server's live-reported
  build version alongside its already-visible resolution source.
