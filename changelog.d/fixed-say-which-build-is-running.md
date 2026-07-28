- **A server now says when it is a stale build of the checkout it serves.** Both
  servers have always reported `<version>+<git-sha>` at MCP `initialize`, and
  comparing that against the checkout was left to whoever thought to look.
  Nobody did. A binary in `target/release` built two days earlier served every
  session in one workspace: it resolved a pre-ADR-0054 forked identity, so
  `renew_lease` returned a silent `false` and two claims lapsed, and the symptom
  was blamed on the VS Code extension **four separate times** across a session.
  The extension was correct throughout; the binary being accused was never the
  one answering, and no surface said which build was.
  Startup now compares the compiled-in sha against `HEAD` and warns with both
  values when they differ. The comparison is deliberately made only when the
  binary lives inside the workspace it is serving: an installed release running
  against an arbitrary repository is *expected* to differ, and warning about
  that would be noise that teaches people to skip the line that matters.
