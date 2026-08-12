- **Fixed four high-severity transitive vulnerabilities in the VS Code
  extension's dev dependencies.** `npm --prefix editors/vscode audit` reported
  `brace-expansion` (GHSA-rgw5-rvv9-x895), `fast-uri` (GHSA-7p8r-x3mc-p8w7),
  `js-yaml` (GHSA-5p4m-2wfm-xmqj), and `nanoid` (GHSA-2v37-7h3g-55p8) under
  `editors/vscode/node_modules`, pulled in transitively through `@vscode/vsce`
  and `vitest`. `npm audit fix` resolved all four with patch-level lockfile
  bumps and no `package.json` changes; `npm audit` now reports zero
  vulnerabilities.
