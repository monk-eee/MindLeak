- **Bridge's Fleet, Agents, and Readiness sections now auto-refresh while the
  tab is visible.** Each section polls its own endpoint every 15 seconds, but
  only while the page is in the foreground: switching tabs or minimising the
  window pauses polling entirely, and returning to the tab triggers an
  immediate refresh before polling resumes. Existing search, sort, and
  pagination state carries over unchanged since a refresh re-runs the same
  load path a manual reload would use.
