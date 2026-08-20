### Added

- The `rust` CI job's `ubuntu-latest` run now installs `gnome-keyring` and
  attempts to start a real D-Bus Secret Service session before the
  credential-facility regression test, so that test can exercise a genuine
  store/load round trip on Linux instead of always hitting its own skip
  branch. Best-effort: a failed install or an unavailable provider falls back
  to the previous behavior, so this cannot turn the check's existing soft
  skip into a hard failure.
