- **`scripts/mcp-build-probe.mjs` asks each MCP build what it writes, instead of
  guessing from dates or git distance.** A stale server that still writes
  absolute node ids is not detectable from either. Measured across ten
  worktrees: two that were only **five commits behind `main`** wrote absolute
  ids, while others 17 and 38 behind wrote correct ones — and every worktree was
  behind on `crates/`, so any threshold-based warning would have fired on all
  ten. A warning that always fires is one people learn to skip, which is how the
  original defect survived three days; that design was measured, rejected, and
  is recorded here so it is not proposed again.
  What separates them is behaviour. Node ids are repo-relative by contract
  (ADR-0038), so the probe hands each binary one file by absolute path against a
  throwaway database and reads the id it produces. No heuristics, no false
  positives, and no live data touched. On first run it found **6 of 15 builds**
  still writing absolute ids — the same six a manual sweep had found, in one
  command. `--check` exits non-zero for CI or a pre-flight before trusting a
  fleet-wide result.
