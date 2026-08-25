### Added

- Bridge's read-only Delegations authority view now follows bounded durable
  pages rather than stopping at the selected limit. Callers follow the paired
  `after_source_event_position` and `after_delegation_id` cursor from
  `next_after`; the UI exposes an accessible Load more authority control while
  preserving already inspected grants and revocations.
