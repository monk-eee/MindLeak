- The optional LLM API key can no longer leak through debug output. `LlmClient`
  (Lodestar) and `Consolidator` (MindLeak) derived `Debug` over a cleartext
  `api_key`, so a single stray `{:?}` — in a log line, a panic message, or an
  error chain — would have printed the bearer token, contradicting the standing
  rule that the key is never logged. Both now implement `Debug` by hand,
  rendering the key as `<unset>` or `<redacted>` while still showing the base URL
  and model. No call site printed it today; this closes the latent path before
  one could, and a regression test in each crate asserts the token never appears
  in `Debug` output.
