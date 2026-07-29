- **Ingest tools are unauthenticated (by design).** — Any client with stdio
  access to `mindleak-mcp` can write nodes/edges. — Acceptable for local
  single-user use; the server has no network listener. Do not expose it over a
  network without an auth layer (see [docs/SPEC.md § 8](docs/SPEC.md)).
