- **The merged-branch audit now runs on every push to `main` instead of only
  when someone remembers.** A merged pull request whose commits never reached
  `main` fails nothing anywhere: the pull request reads merged, the branch reads
  ahead, and CI is green on both — the only signal is an ancestry check nobody
  was running. `scripts/merge-audit.mjs` is that check, and it identifies both
  known incidents correctly after the fact, naming the pull request and each
  commit that was left behind. It was reachable only through `make merge-audit`,
  a command with no reason to be typed on a good day, which is precisely when
  this failure happens. It now runs in CI on pushes to `main` — the moment when
  "did the thing that just merged leave work behind?" is a live question — and
  not on pull requests, where it is not yet a question at all.
