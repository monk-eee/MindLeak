- **Work task detail stays consistent during concurrent commands:** Bridge and
  MCP reads now fetch the task, event history, and waits from one read-only
  database snapshot. A command committed during a read appears together on the
  next request, rather than mixing an older task version with newer history or
  answers. Missing tasks and tenant/repository boundaries remain unchanged.
