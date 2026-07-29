- **The delivery queue now names the open work it is not managing.** Arming a
  pull request is what puts it in the queue (ADR-0045), so an unarmed one is not
  last in line — it is not in the line at all. The tick reported only the armed
  entries, which made "nothing is waiting" and "three pull requests are waiting
  and nobody armed them" print identically. Measured on the day this landed:
  three of five open pull requests were invisible to the queue, and no change to
  the ordering could have reached them, because ordering only ever applies to
  what is in the queue. Each unmanaged pull request is now listed with its merge
  state and the reason — `not queued: nobody armed it`. This is reporting, not
  policy: arming still decides membership and the first-in-first-out order by
  arming time is unchanged.
