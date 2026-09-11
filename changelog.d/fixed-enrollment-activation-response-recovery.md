- `register-me activate` now saves the bound public challenge nonce before
  submitting proof, allowing a restarted CLI to recover the original activation
  result after a lost response. A pending approval may refresh its challenge;
  completed activation clears the retry nonce only after saving its receipt.
  Server replay resolves the key bound to the original receipt rather than a
  newer key for the same node. No private key or signature is persisted.
