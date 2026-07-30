- The editor no longer sends the server a path it cannot place. `asRelativePath`
  returns its input *unchanged* when a file sits outside every workspace folder,
  and agents routinely edit a sibling worktree from a window rooted elsewhere, so
  an absolute path went on the wire and became a second identity for a file the
  graph already tracked — measured on 2026-07-30 as 34 absolute artifact nodes,
  with one file holding 117 structural edges under its absolute id and 43 under
  its relative one.
  The server has refused such a path since the ingest guard landed, so these
  calls were already failing loudly rather than corrupting; what remained was the
  editor generating a doomed request on every save of an out-of-workspace file.
  A single pure helper now decides whether a raw path is repository-relative,
  mirroring the server's rule so the two agree on what "relative" means, and the
  save, focus, delete and commit paths skip what they cannot place instead of
  asking. A placeable path is sent exactly as before — the rule rejects the id
  shape, not the file.
