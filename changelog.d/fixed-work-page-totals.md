- **Work pagination preserves totals on empty pages:** Bridge and MCP Work lists
  keep the actual matching task count when a requested page is beyond the last
  result. Counts and rows still come from one SQL snapshot, with state filters,
  repository boundaries, and ordering preserved. Very large positive page
  numbers return an empty page instead of overflowing the offset calculation.
