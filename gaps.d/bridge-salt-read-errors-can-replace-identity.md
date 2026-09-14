- **Bridge salt read errors can replace the developer-tenant identity -- OPEN.**
  Source inspection on 2026-09-14 found that
  `crates/ackplane-bridge/src/lib.rs::load_or_generate_salt` treats every failed
  `fs::read`, not only `NotFound`, as a reason to generate and write another
  salt. An existing file that is writable but unreadable can therefore be
  replaced rather than refused. The salt determines the developer-tenant token,
  so replacement can switch the Bridge to a different tenant identity and make
  its existing records appear absent.

  This was observed while testing startup exit codes, not reproduced against a
  live identity. Left for a separate fix: distinguish a missing salt from an
  unreadable existing salt, propagate non-absence read failures, and prove that
  failure leaves the existing bytes untouched. Returning a nonzero startup code
  does not fix this loader, because a replacement write can itself succeed.
