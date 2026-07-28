- **The 18 extension settings are grouped instead of one flat list.** "Where is
  the server binary" sat beside "how many characters of terminal output to
  retain" with nothing to say which one a first-time user needs to touch. They
  are now four titled sections — Servers, Capture, Consolidation, Views — which
  VS Code renders as separate blocks in the settings UI. Setting ids are
  unchanged, so no existing configuration moves and nothing about behaviour
  changes; this is presentation only. A test asserts each setting belongs to
  exactly one non-empty titled group, and — the failure that actually costs
  someone an afternoon — that every setting the code reads is declared in the
  manifest. An undeclared setting silently returns its inline fallback, so it
  cannot be found in the settings UI and appears to do nothing when set by hand.
