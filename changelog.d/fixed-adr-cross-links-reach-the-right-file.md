- **The resolvable ADR cross-links reach the right file again.**
  Renaming an ADR orphaned every inbound `[ADR-00NN](00NN-old-slug.md)` — the
  link kept the target's title as it was when written, and 404'd once that title
  changed. Twelve such links across nine ADRs now point at the real `00NN-*.md`
  file for their number (href only; the decisions and the parenthetical
  descriptions are untouched), and ADR-0031's `ARCHITECTURE.md` link becomes the
  correct `../ARCHITECTURE.md`. Three references to a phantom `ADR-0045 "armed
  means finished"` are left as-is and flagged for a maintainer: no ADR ever bore
  that name (0045 is "a fleet is a distributed system"), so pointing them
  anywhere would fabricate a target — that needs the author's intent, not a
  mechanical rewrite.
