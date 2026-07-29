- **A control can be created through the MCP surface but never stood down —
  FIXED.** `register_control` and `register_ratchet` were exposed;
  `retire_control` existed on the core facade and was not. An operator who
  registered a control under the wrong id, or whose control had been superseded
  by a better one, had no way to retire it without linking against the library,
  and re-registering the id is refused because a control version never moves
  backwards — so the id was spent and the dead control kept reporting against a
  live clause. — Status: fixed. `retire_control` is on the tool surface,
  session-bound and attributed: the store records who stood a control down and
  when, resolved from the calling session rather than supplied by the caller,
  for the same reason a waiver names an author. Standing a mechanism down is the
  one act that reduces what a clause can enforce without changing a word of the
  clause. Retirement is deliberately not deletion: the control keeps recording
  what it enforced, so observations naming it resolve as `unknown` rather than
  disappearing. Controls retired before this was recorded carry no author —
  those retirements cannot be reconstructed, and inventing one would be worse
  than admitting the gap.
