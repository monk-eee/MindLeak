- Optional HTTP circuit breakers now count failures that happen during endpoint
  resolution or response decoding, so a broken embedding or consolidation
  endpoint fast-fails after the configured threshold instead of repeatedly
  consuming its timeout. Embedding responses now reject non-numeric,
  non-finite, and inconsistent-dimension vectors before any index rows are
  written, preventing malformed model output from silently disappearing from
  semantic recall.
