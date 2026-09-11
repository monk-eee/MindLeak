- **Tool-call evidence uses the registered session:** `ingest_tool_invocation`
  now advertises and enforces the same session contract as other attributed
  ingestion tools. Registered callers no longer fail with a missing `agent`
  argument, missing or unknown sessions are refused, and caller-supplied agent
  labels cannot select the stored observer. Shell-hygiene classification and the
  result format are unchanged.
