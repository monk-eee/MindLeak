- **An installed binary now says which build it is.** The startup notice only
  ever spoke when the running binary lived inside the workspace it served, on
  the reasoning that an installed release is not built from that checkout so a
  difference means nothing. That rules out calling it *stale* — it does not
  excuse saying nothing at all. The VS Code extension binaries are the ones the
  fleet actually runs, and they reported no identity, so three servers served a
  build predating a merged fix for most of a day, deciding conformance verdicts
  with it, while every surface read healthy. An out-of-workspace binary now logs
  the sha it was built from and explicitly does not claim staleness; a binary
  inside the workspace keeps the existing comparison. `stale_build_notice` is
  renamed to `build_notice` and returns a `BuildNotice` carrying a `stale` flag,
  so a genuine staleness warning stays a warning and identity is logged as
  information — a notice that cries wolf is how the real one gets ignored.
