- **Ackplane now persists human decision (escalation) requests as immutable
  events with a checked current projection (ADR-0115 item 5).** A request
  names the proposed action, target, reason, context packet and evidence
  digests, alternatives, safe behavior while pending, and expiry, and can only
  be resolved by a verified principal distinct from the one that proposed it
  (ADR-0115 item 8: separation of duties). This slice is server-internal: it
  does not yet add a Bridge queue, listing/pagination, or policy automation.
