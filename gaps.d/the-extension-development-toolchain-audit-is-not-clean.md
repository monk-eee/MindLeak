- **The extension development-tool audit reports 17 high and one low advisory,
  while the shipped production dependency set remains clean.** — Observed on
  2026-07-31 while installing the declared `editors/vscode` dependencies for
  ADR-0079 validation. `npm audit --omit=dev` reports zero vulnerabilities, but
  the full audit reaches high-severity `brace-expansion` / `minimatch` paths
  through ESLint, `@vitest/coverage-v8`, and `@vscode/vsce`, plus the known low
  esbuild development-server issue. Most suggested fixes are major toolchain
  upgrades (ESLint 10, typescript-eslint 8, Vitest 4); forcing them inside an
  unrelated model-health change would bypass compatibility review. Development
  tooling may remain exposed to denial-of-service inputs; production extension
  dependencies are unaffected. Left open for a dedicated toolchain upgrade.
