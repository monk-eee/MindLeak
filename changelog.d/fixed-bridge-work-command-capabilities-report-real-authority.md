- Fixed the Bridge Work page reporting every command as
  `authorization_unavailable` while the command routes underneath it executed
  all ten. ADR-0142 gave the hardened loopback profile a real verified
  principal, but the read surface kept a second, hand-written capability list
  that still described the pre-ADR-0142 world. The list is now derived from the
  same `verified_principal` grant the `submit`/`confirm` routes authorize
  against, so the two cannot drift apart again: an operation the principal
  allows reports `available_without_policy`, and one it does not still reports
  `authorization_unavailable` with its own reason. The page distinguishes the
  two, and says plainly that an available command's control is disabled because
  the read-only Work page has no submit wiring — not because authorization is
  missing.
