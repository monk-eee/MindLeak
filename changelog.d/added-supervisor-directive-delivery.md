- **A directive issued into Ackplane now actually reaches a running supervisor,
  and its receipt comes back (ADR-0116 slice 3, closing ADR-0107's control
  loop).** Ackplane could already record a directive and could already ingest a
  receipt, but nothing carried one between them: the server emitted no
  `AgentDirective` frame at all, and `DirectiveStore` had no read path to find
  a pending directive with. A supervisor's local inbox and the server's receipt
  recorder had therefore only ever been exercised with synthetic frames. The
  server now delivers a session's outstanding directives over the live
  authenticated NodeSync connection, and `NodeSyncConnection` gains
  `next_directive` and `submit_directive_receipt` so a supervisor can process
  one and return its receipt over the same stream.
  Delivery is at-least-once and says so: Ackplane records no "delivered"
  marker, because a frame it put on a stream is not evidence a supervisor acted
  — only the returned receipt is. Redelivery after a reconnect is therefore
  normal and safe, because the receiving inbox replays the original receipt
  instead of acting twice. Directives are sent ahead of the receipt for the
  session frame that addressed them, making that receipt a delivery barrier, so
  a supervisor draining its directives can never miss one still in flight.
