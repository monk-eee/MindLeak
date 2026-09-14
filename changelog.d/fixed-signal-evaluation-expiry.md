- The deterministic signal evaluation now measures retained expired evidence
  through the existing signal and decay APIs, and separately verifies that
  active traversal hides it. It no longer panics while looking for an expired
  spam edge. The evaluator's full contract now runs in the normal Cargo test
  suite. Decay thresholds, reinforcement rules, and its JSON output are unchanged.
