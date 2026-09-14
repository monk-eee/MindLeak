- **Supervisor restart identity:** durable queues now store the full original
  session atomically with their identity binding. Restart restores that session
  instead of sending a new start time under the same ID; recovery requires both
  queues to match the original run. Permanent registration, session and heartbeat
  refusals stop the daemon instead of causing endless reconnects. Older queues
  without the original declaration are explicitly refused with evidence retained;
  preserve and reconcile them before configuring a new supervisor and state directory.
