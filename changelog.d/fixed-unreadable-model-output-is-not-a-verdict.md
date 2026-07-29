- **An unreadable model answer no longer masquerades as a semantic verdict.**
  `LlmClient::judge` used to turn a missing `verdict` into `needs_human` and a
  missing `rationale` into empty text, so a protocol failure reached the durable
  receipt as `semantic check needs human review: ` and sent a human to review
  nothing. Missing fields now follow the existing `semantic check unavailable`
  path, an unsupported verdict names the value the model returned, and a real
  `needs_human` answer with a blank rationale says `judge gave no reason`.
