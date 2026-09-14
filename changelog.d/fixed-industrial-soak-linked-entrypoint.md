- Fixed the Industrial endurance runner's startup through directory links and
  junctions: it now resolves both entry-point paths before deciding whether to
  run, instead of silently exiting zero without starting. Canonical invocation
  and side-effect-free imports are preserved and tested.
