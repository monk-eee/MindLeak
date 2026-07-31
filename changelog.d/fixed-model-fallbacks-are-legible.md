- **Optional model failures are now bounded and visible instead of silently
  degrading.** Both planes separate a one-second connect budget from the model
  read budget and retry one read timeout exactly once, while connection refusal
  and 4xx responses still fail immediately. Model-backed results identify
  `model` versus `fallback` with a stable failure reason, and
  `storage_status(include_model_health=true)` performs an explicit one-shot
  reachability/JSON probe without adding a background poller.
