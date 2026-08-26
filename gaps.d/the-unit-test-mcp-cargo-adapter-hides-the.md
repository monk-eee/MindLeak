- **The Unit Test MCP cargo adapter hides the assertion, so a red test cannot be
  diagnosed — OBSERVED, still true in the tool; this repo no longer routes
  agents to it by default.** `run_tests` with `framework=custom` returns
  `status: FAILED` with `passed/failed/total` all zero and a message containing
  only cargo's stderr (`error: test failed, to rerun pass -p <crate> --test
  <target>`). The failing test's name and its assertion output go to the
  harness's stdout, which the adapter drops, and `compact_output=false` does not
  bring them back. Impact: a genuine red is indistinguishable from a compile
  error, and there is no way to tell *which* test failed or why. This is what
  turned the mtime bug above into a long hunt instead of a one-line read.
  Workaround: have the test write its result to a file under `target/tmp/` and
  read that file, then delete the write before committing. Left for later — the
  adapter needs to surface harness stdout on failure.

  **`test_pattern` is also ignored, and its apparent effect is a trap —
  MEASURED 2026-07-29.** Passing `test_pattern` does not narrow a cargo run: the
  whole lib suite executes and aborts at the first failure. Proven by a control
  experiment while red/green-proving the amendment control tests — with a
  deliberate break in `amend_constitution`, a run naming
  `an_amendment_that_changes_nothing_is_refused`, a test that cannot touch that
  code, still returned `FAILED`.
  The trap is the timing. A filtered-looking run returns in 6–7 s against ~60 s
  for a green suite, which reads exactly like a filter working. It is not: that
  duration is *time to first failure*, so it shrinks as the suite gets redder.
  Anyone using run duration to infer that a filter took effect will conclude the
  named test failed when the failure was somewhere else entirely — this note
  exists because that inference was made and acted on earlier the same day.
  To attribute a failure to one test, mark the others `#[ignore]` and run the
  full suite; that does work, and it is how the three control tests were each
  proven red for their own reason.

  **NARROWED 2026-08-26.** The line above originally read "while the repo
  instructions correctly forbid running `cargo test` in a terminal" — that
  instruction is gone. `AGENTS.md`'s "Commands" section used to prefer the
  Unit Test MCP tool for test runs generally; it now names this fragment's
  hidden-assertion and ignored-`test_pattern` defects directly (alongside its
  three siblings) and tells agents to run `cargo test`/`cargo test --all` in a
  terminal instead, which surfaces the real assertion and honours a real
  filter. This closes this repository's exposure to both defects; neither is
  fixed in the adapter itself, which remains a third-party extension.
