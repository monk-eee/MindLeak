- **The one-shot embedding test server can reset a valid Windows request —
  OBSERVED, OPEN.** GitHub Actions run `30694558233` failed the Windows
  `cargo test --all` job in
  `embed::tests::a_batch_round_trip_returns_one_vector_per_input_in_input_order`
  after 435 tests passed. The helper at
  `crates/lodestar-core/src/embed.rs::tests::stub_embedder` accepts a socket,
  performs one request read, writes its response, and drops the connection; the
  client reported Windows error 10054 because the remote host forcibly closed
  it. The failed-job rerun and the next queue-refreshed run both passed without
  a code change, isolating this to a nondeterministic test transport rather than
  product embedding behavior. Impact: an unrelated pull request can lose its
  required Windows gate, and a real transport regression has a noisier signal.
  Left for later — consume the complete HTTP headers and declared request body
  before replying and closing, then validate the helper repeatedly on Windows.
