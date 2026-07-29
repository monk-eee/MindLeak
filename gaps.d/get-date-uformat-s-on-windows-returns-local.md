- **`Get-Date -UFormat %s` on Windows returns local time as an epoch, not UTC —
  CONFIRMED, no code change.** Any evidence window built from it is hours in the
  future, and `check_conformance` then rejects it with *"evidence interval falls
  outside the live claim"* — an error that reads like a lapsed claim and is not
  one. Impact: an agent can wrongly conclude a task is stranded and escalate it
  to a human. Use `git log -1 --format=%ct`, which is true UTC. Confirm a
  suspected lapse with `renew_lease` (`renewed: false`) rather than inferring it
  from that message.
