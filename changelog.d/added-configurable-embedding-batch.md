- **The semantic index batch size is configurable, so a high-dimension embedding
  model is usable instead of merely slow.** `MINDLEAK_EMBED_BATCH` (default 64,
  unchanged) sets how many nodes are embedded per `/v1/embeddings` request. The
  batch was previously a fixed 64, which is only safe for a fixed embedding
  dimension: a response carries `batch * dimensions` floats and an optional HTTP
  response is capped at 4 MiB. Measured 2026-08-27 against LM Studio,
  768-dimension `nomic-embed-text` costs ~23 KB per vector so 64 fits in ~1.5
  MiB, while 2560-dimension `qwen3-embedding-4b` costs ~78 KB per vector so the
  same 64 lands at ~4.75 MiB and *every* index pass failed on the cap with
  `optional HTTP response exceeded 4194304 bytes`. The default is unchanged for
  the dimensions this shipped against; a larger model now sets the batch down
  rather than being unusable. An absent, unparseable, or zero value falls back to
  the default rather than failing the pass.
