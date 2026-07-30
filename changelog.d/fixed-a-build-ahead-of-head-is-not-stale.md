- A binary built *ahead* of the checkout is no longer reported as a stale build.
  The notice compared build sha against `HEAD` with a plain string inequality and
  no ancestry check, so any difference in either direction produced the same
  advice: "Rebuild and restart". Measured on 2026-07-30, the checkout the fleet's
  servers are compared against sat 599 commits behind `main`, so a binary built
  from `main`'s tip was reported stale on every `open_session` — and following
  that advice would have rebuilt from the older checkout and reverted an ingest
  guard merged minutes earlier. A warning whose remedy undoes the fix is worse
  than silence, because it gets followed.
  Staleness now requires evidence that the build is actually behind: when the
  build has `HEAD` in its history, the notice says the checkout is behind the
  binary and to update the checkout instead. A build genuinely behind `HEAD`
  still warns exactly as before, and an unanswerable lineage — git unavailable,
  or a commit this checkout does not have — is treated as ignorance rather than
  as proof the build is fine, so it keeps warning. Both cases still name the
  build sha, because "which build is answering" is the question the notice
  exists to answer.
