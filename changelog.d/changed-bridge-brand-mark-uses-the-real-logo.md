- **Bridge's shared brand mark is the real MindLeak mascot, not a placeholder.**
  The header brand mark rendered by `static/shared/chrome.js`/`chrome.css`
  (ADR-0124) previously stood in a text glyph for the product's own logo. It
  now renders the actual `mindleak_128x128.png` mascot artwork (already used
  as the VS Code extension's icon) via a dedicated `/static/shared/mark.png`
  route, so every Bridge page carries the same real brand mark the rest of
  the product already ships.
