- Added `make install-industrial` and the explicit `--profile industrial`
  installer option for the six Industrial host binaries, with federation-capable
  local planes and stable user-local executable paths. The default Local profile
  is unchanged. Industrial installs require complete release builds, honor
  `CARGO_TARGET_DIR`, and stage replacements before moving working commands;
  missing builds and failed copies no longer partially remove an installation.
  No services are started and no credentials are copied or replaced.
