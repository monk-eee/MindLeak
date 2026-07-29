- **A control can now be stood down through the tool surface.**
  `register_control` and `register_ratchet` were exposed and `retire_control`
  was not, so a control registered under the wrong id was permanent: its
  version can never move backwards, which means re-registering the id is
  refused, and there was no supported way to withdraw it. Dead and duplicate
  mechanisms accumulated against live clauses and went on reporting.
  `retire_control` is now an MCP tool. Retirement is deliberately not deletion —
  the control keeps recording what it once enforced, so an observation naming it
  resolves as `unknown` rather than quietly disappearing, which is the honest
  answer to "this measurement came from a mechanism we have since stood down".
