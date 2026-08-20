- **Changed:** CI now surfaces whether `ackplane-client`'s OS credential
  facility round-trip test actually ran for real or silently skipped (no
  Secret Service daemon on the Linux runner), instead of both reading as an
  identical "passed" (gaps.d/the-credential-facility-signer-is-unverified-outside-windows.md).
