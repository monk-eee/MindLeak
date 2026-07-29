- **A guard was watching two names the server had stopped answering to under
  their own contract.** The test that proves the session envelope is tolerated
  rather than validated as a tool argument — the regression that once made the
  VS Code extension report `disconnected` instead of `ready_empty`, and which
  only the Extension Host smoke noticed — asserted over `board` and
  `design_board`. ADR-0059 retired both. The server no longer advertises them
  and answers them only through the deprecation table, so the guard was
  exercising the *aliases* while the advertised readiness path (`task_query`,
  `design_query`) went unwatched: the same regression could have returned
  through the new names without failing anything, and when the aliases are
  removed the guard would have vanished with them.
  Its own comment had predicted this — *"a whitelist entry is easy to drop in a
  refactor, and the unit that catches it must be the call the client actually
  makes"* — and the whitelist had quietly stopped being that call. It now
  asserts each name it mentions is a tool the server actually advertises, so
  the next rename fails here instead of hollowing the guard out in silence.
  This is the same defect class as `requires_session`: a list keyed by tool
  name that a rename left pointing at nothing. Finding it twice by hand is what
  the fence is for.

- **Two more retired names were still in live guidance and a second guard.**
  Reconnecting with paused work advised the owner to *"Call resume_task"* — a
  verb the server no longer advertises, offered to an agent at exactly the
  moment it is trying to get back to work. It now names `task_transition` with
  `to="resume"`, and says which argument answers a `needs_input` task instead.
  The guard proving an offered session sharpens the overlap read while its
  absence never refuses (ADR-0024) was driven through `check_overlap`; it now
  goes through `task_query` with `view="overlap"`, because the alias resolves
  to the same handler and so proved only that the deprecation table works.
