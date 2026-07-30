- **The link checker no longer trips over illustrative links in code.**
  A `[text](target)` written inside inline backticks or a fenced code block is
  documentation about a link — an example, a former filename, a shape — not a
  live link, so its target need not exist. The checker (added in the previous
  change) treated them as real and flagged them, which blocked any PR whose
  changed docs carried such an example. It now skips links inside code spans and
  fenced blocks, while still catching a real link elsewhere on the same line.
