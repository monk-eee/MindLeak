- **The Lodestar tool surface is now tiered: the default profile is the common
  path, and the specialist machinery is advertised only when asked for.**
  Every agent loads `tools/list` before its first question, so an unspent
  minute of governance authoring — the constitution, amendments, policy packs,
  waivers, ratchets, the design board, database admin — was a tax paid in every
  session of every worktree (ADR-0059 rule 2). The default profile now
  advertises the seventeen tools an agent uses to find, claim, do, prove and
  hand off work, plus the ones it reads to know what governs it: 17 tools,
  ~4,513 tokens, down from 67 tools, ~13,757 tokens. Nothing became
  unreachable — dispatch is unchanged, so a specialist tool called by name
  still runs. Set `LODESTAR_TOOL_PROFILE=full` to advertise the whole surface.
  The allowlist is deliberate: a tool added anywhere else is specialist until
  someone puts its name on the common path, so the surface an agent pays for
  every session grows by decision rather than by default.
