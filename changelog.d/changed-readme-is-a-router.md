- **The README is a router again, and the tool reference has its own page.** The
  front door carried 90 rows of tool tables and put architecture and build
  instructions ahead of "how do I try this", so the fastest path for a new
  reader was to scroll past the design of the system to reach the install. The
  tables move to `docs/TOOLS.md` — a reference is for looking things up in, not
  for reading — and the getting-started sections now come before the ones that
  explain how it works. README drops from 436 lines to 316. Every pointer moved
  with it: `AGENTS.md`, `DEVELOPERS.md`, `docs/ARCHITECTURE.md`, `docs/USAGE.md`
  and the pull-request template all named the README table as the thing to
  update when a tool is added, and would otherwise have sent the next
  contributor to a table that is no longer there.
- **Both "adding an MCP tool" worked paths pointed at a file that does not
  exist.** `crates/mindleak-mcp/src/tools.rs` became a directory when the tools
  were split into modules, and the instruction to add a `CHANGELOG.md` line
  predates fragments (ADR-0056). A worked path that names the wrong file is
  worse than none: it is followed confidently.
