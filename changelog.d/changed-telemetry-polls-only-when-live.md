- **The Telemetry pane now polls periodically only after the user enables
  Live.** The three-second timer previously ran whenever the pane was visible,
  even with Live off. Lifetime telemetry measured 16,522 `graph_stats` calls
  and 12,567 `telemetry_snapshot` calls against 66 reads that could change a
  decision; `graph_stats` alone spent 57 cumulative minutes answering the
  dashboard. That wasted compute and made the telemetry record mostly describe
  its own observer.
  Opening the pane, clicking Refresh, and toggling Live still refresh
  immediately. The configured cadence is unchanged and applies only to the
  opt-in live stream. A pure four-state regression test pins hidden,
  visible-non-live, and visible-live behavior.
