- Added revision-bound Industrial host archives built by `make package-industrial`
  and the tagged release workflow. Each platform bundle contains the six host
  binaries and a Node-only installer, with version, revision, platform and
  per-binary checksums. Installation needs no Cargo or Git checkout and refuses
  invalid manifests, wrong platforms and changed binaries before replacement.
  Local archives remain unchanged; no credentials or deployment state are bundled.
