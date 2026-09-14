- Added an explicit timed Industrial regression runner that repeats the existing
  full database/recovery/native-credential gate against a clean pinned source
  candidate. It requires disposable loopback fixtures, isolates build output,
  journals command and cycle outcomes, and counts only successful completed
  cycles toward the requested duration. Failed, changed-source or interrupted
  runs never report a passing measurement. This measures regression endurance,
  not continuous uptime or the small-team pilot.
