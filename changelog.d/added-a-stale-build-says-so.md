- **A server binary that predates its own source now says so.** A running MCP
  server is a *build*, not the code in front of you, and the gap between them is
  invisible: the tool answers, the answer is wrong in exactly the way the old
  code was wrong, and the obvious next move is to doubt the fix rather than the
  binary. It cost three separate diagnoses in a single day — a conformance
  verdict read as a product defect, a knowledge record that stayed silent after
  the change that should have made it speak, and the whole different-session-id
  incident behind ADR-0054. `canonical-push` and the ratchet reporter now warn
  when the resolved binary is older than the newest Rust source, naming the
  binary, how far behind it is, and the command that fixes it. Only local builds
  under `target/` are judged: a binary supplied through `LODESTAR_MCP_BIN` may be
  a released artifact that was never built from this tree, and warning that a
  shipped release is older than `crates/` would fire on every run — a warning
  that is always on is a warning nobody reads.
