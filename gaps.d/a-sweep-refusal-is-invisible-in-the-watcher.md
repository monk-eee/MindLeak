- **The delivery watcher discards every reason the sweep declined, so a sweep
  that has stopped running looks identical to one that is up to date.**
  `sweepNow` in
  [`scripts/delivery-queue.mjs`](../scripts/delivery-queue.mjs) logs only when
  `outcome.ran` is true; every `{ ran: false, reason }` is dropped on the
  floor. That was defensible while the only refusals were "not due" (true on
  all but one call in several hours) and lock contention. It is not defensible
  now that `sweepIfDue` can refuse for `stale checkout: …`, because that
  refusal is persistent — a refused run never records `lastRunAt`, so a stale
  watcher declines on every 60-second tick and says nothing on any of them.
  The observable symptom is disk growing without bound again, with a watcher
  that appears healthy; the diagnosis is one field that was computed, returned,
  and never printed. Impact is worst on exactly the path that has no operator
  watching it, which is the path the freshness guard exists to protect.
  The fix is not simply to print it — at one tick per minute that is 60
  identical lines an hour, which is how a log becomes something nobody reads.
  It needs announce-on-change (remember the last reason, print only when it
  differs, clear it on a successful run), and that belongs with whoever next
  holds `delivery-queue.mjs`. Left for later; the refusal itself is correct and
  the standalone CLI does print it.
