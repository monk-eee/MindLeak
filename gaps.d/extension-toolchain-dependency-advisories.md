- **The extension development toolchain has unresolved npm advisories -- OPEN.**
  On 2026-09-11, `npm audit --json` in `editors/vscode` reported six affected
  packages (two high, four moderate), not six distinct advisories. Installed
  versions from `editors/vscode/package-lock.json`: `vitest`,
  `@vitest/coverage-v8`, and `@vitest/mocker` 4.1.10; `fast-uri` 3.1.5;
  `js-yaml` 4.3.1; `qs` 6.15.3. Reported issues include mock-server path
  traversal (GHSA-82fw-gwwq-j7x9), URI host confusion/SSRF
  (GHSA-5jgf-p345-68v8, GHSA-f65p-4m7j-42xc, GHSA-fph4-wmhf-6fwf,
  GHSA-jqff-g426-hqxp), YAML merge CPU exhaustion (GHSA-2883-xcg3-v3hh), and
  query-parser limit bypass/denial of service (GHSA-x5fp-wj9c-mxmx,
  GHSA-4mjr-xmp4-gh2g). Audit identifies fixes including Vitest 4.1.11,
  fast-uri 3.1.6, js-yaml 4.3.2, and qs 6.16.0. This observation does not
  establish exploitability of the packaged extension. Left for later: update
  compatible toolchain dependencies and verify extension tests, packaging, and
  the resulting audit; no automatic dependency fix was applied during Work UI
  implementation.
