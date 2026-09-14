- Added revision-bound Industrial host archives built by `make package-industrial`
  and the tagged release workflow. Each platform bundle contains the six host
  binaries and a Node-only installer, with version, revision, platform and
  per-binary checksums. Installation needs no Cargo or Git checkout and refuses
  invalid manifests, wrong platforms and changed binaries before replacement.
  Extracted installers and the bundle CLI also run correctly through directory
  links instead of silently returning success without doing any work.
  Local archives remain unchanged; no credentials or deployment state are bundled.
