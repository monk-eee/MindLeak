- The MCP servers no longer die on a single non-UTF-8 byte. Both stdio serve
  loops read each line with `read_line`, which returns `InvalidData` on invalid
  UTF-8 and propagated straight out of the loop — terminating the server and its
  in-memory session state, even though a malformed *JSON* line is already
  recovered with a `-32700` parse error. A stray byte from a mis-encoding pipe
  was enough. Both loops now decode each line with `String::from_utf8_lossy`, so
  a malformed line becomes the same recoverable parse error and the server keeps
  serving; only a genuine I/O error still ends the loop.
