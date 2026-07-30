- **Mixed-type embedding vectors are silently shortened and stored as valid --
  REPRODUCED, OPEN.** `Embedder::embed_batch` converts an embedding with
  `.filter_map(|value| value.as_f64())`, so a non-number is dropped instead of
  rejecting the model response. The remaining values are accepted as a vector,
  `index` reports success, and the shortened dimension is persisted.

  Reproduced through the public MCP surface on 2026-07-30 from main
  `1f82ef811a16f9948a758beb24d690bb4f98c4d1`, and verified unchanged on main
  `fd176c196164196c6e3e02381fa5a510e2957d83`. A local OpenAI-compatible mock
  returned `{"data":[{"index":0,"embedding":[0.1,"not-a-number",0.2]}]}`.
  Two `index(limit=1)` calls each returned `{"indexed":1}`; SQLite then held
  both indexed nodes with `dim = 2` and an eight-byte vector, silently losing
  the middle element of the three-element response.

  Impact: one malformed model response can create vectors whose dimensions no
  longer match later query vectors. `cosine` returns zero for unequal lengths,
  so affected nodes disappear from semantic recall without an error explaining
  why. Left open: reject every non-numeric or non-finite embedding component,
  and require one consistent, non-zero dimension across the whole batch before
  writing any vector.
