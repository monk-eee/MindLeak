- **`link-check` crashes with a raw stack when a tracked doc is deleted but not
  yet staged — OBSERVED 2026-07-30, OPEN, low severity.**
  `scripts/link-check.mjs` takes its file list from Git's tracked set and then
  reads each path from disk. A doc deleted in the working tree and not yet
  staged is still tracked, so `readFileSync` throws `ENOENT` and the script dies
  with a Node stack trace naming `checkRepo` — not a message naming the file or
  saying it has been deleted. Observed while removing four gap fragments: the
  crash also took `node scripts/script-tests.mjs` red (`fail 1`), and both went
  green the moment the deletions were staged, which makes the failure look
  intermittent and unrelated to what was actually done.

  Impact is small and entirely diagnostic — the pre-commit hook stages before it
  runs, so this is reachable mainly when the script is run by hand mid-edit. But
  a tool whose failure mode is a stack trace pointing at its own internals sends
  the reader looking in the wrong place, which is the same cost as any silent
  guard. Skipping a tracked path that no longer exists, or reporting it as
  deleted, would both be honest; crashing is not.
