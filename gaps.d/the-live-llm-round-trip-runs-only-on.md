- **The live LLM round-trip runs only on demand, not in CI.** — Ignored tests
  (`cargo test -- --ignored`) exercise the real `/v1/chat/completions` call for
  both planes (MindLeak `consolidate`, Lodestar `decompose`/`judge`) against a
  running model; CI can't run them without one. — Low impact. — Running them
  surfaced (and fixed) that `glm4:9b` wraps its JSON in prose even with
  `response_format: json_object`; both clients now extract the JSON object
  robustly.
