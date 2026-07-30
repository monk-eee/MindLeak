- **The README layout diagram no longer states a per-server tool count.**
  It read `mindleak-mcp (23 tools)` / `lodestar-mcp (49 tools)` — both stale
  after the cluster collapses (ADR-0059) and the default/full profile split
  (ADR-0059 rule 2), where a single number is neither current nor meaningful (17
  in the default profile, more under `LODESTAR_TOOL_PROFILE=full`). A layout
  diagram is not the place a count can be kept honest, so it no longer claims
  one; `docs/TOOLS.md` and `scripts/measure-tool-surface.mjs` are where the
  surface is stated and measured.
