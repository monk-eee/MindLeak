### Added

- CI now runs a reference-consumer compatibility gate (ADR-0104):
  `scripts/reference-consumer.mjs` spawns both `mindleak-mcp` and
  `lodestar-mcp` through the packaged Node client (ADR-0103) and exercises
  one representative read-only call per tool family, so a breaking change to
  either server's tool surface is caught in CI before merge instead of
  surfacing first as a downstream consumer's failure.
