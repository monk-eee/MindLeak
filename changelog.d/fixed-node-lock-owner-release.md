- Node process ownership now explicitly unlocks when its guard is dropped, so
  a duplicated Unix descriptor cannot keep a gracefully stopped provider locked
  during restart. The stable lock file remains in place. Live-owner exclusion,
  independent repositories, crash release and protection of the next owner are
  preserved and tested; no credential reset or retry delay is introduced.
