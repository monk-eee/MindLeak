### Fixed

- **Work command vocabulary is single-sourced (ADR-0125 decision 3).** The
  Bridge's read-only `/api/v1/repositories/:id/work` command-capability list
  used to hardcode its own copy of the ten Work command operation names,
  duplicating `ackplane-server`'s `WorkCommandKind` enum. A new public
  `ackplane_server::work_command_vocabulary::WORK_COMMAND_OPERATIONS`
  constant is now the one canonical declaration both `WorkCommandKind` (via
  its `operation_name` method) and the Bridge derive from, so the two
  vocabularies can no longer independently drift. No response shape or
  behavior changes; the Bridge still reports every command as
  `authorization_unavailable` until a real authorization path exists.
