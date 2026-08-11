- **The model-policy test flakes under a full parallel test run.** —
  `llm::tests::model_policy_does_not_retry_a_4xx_rejection`
  (`crates/lodestar-core/src/llm.rs`) failed once during
  `cargo test -p lodestar-core -p lodestar-mcp`, asserting `Some(Timeout)`
  against the expected `Some(Misconfigured)`; the stub server thread panicked at
  the same moment with `write test model body: Os { code: 10053,
  ConnectionAborted }`. The test passes on its own, so the 4xx response never
  reached the client and the policy classified the aborted socket as a timeout
  rather than the rejection it was. — Medium impact: it is a false red on a real
  assertion, and re-running to green is exactly how a genuine regression here
  would be dismissed. — Left for later; observed while implementing ADR-0090's
  certification status, which does not touch the model path.
