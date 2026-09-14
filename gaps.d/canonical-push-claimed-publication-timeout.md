- **The claimed-publication fixture can exceed Vitest's default timeout -- OPEN.**
  CI run `34794061852` on `6a7c684133ef6e6e4e84a9af05c439beb4331875`
  failed the offline extension job at
  `editors/vscode/scripts/canonical-push.test.mjs` /
  `provides both MCP planes for claimed publication`: `Test timed out in 5000ms`.
  The test constructs the Git sandbox but omits the explicit `TIMEOUT_MS` used
  by neighboring sandbox tests. This can reject otherwise passing publications;
  the new Linux and Windows native Industrial archive checks passed in that run.
  A focused rerun with the same refused HTTP/HTTPS proxy settings passed locally
  in 264ms. Left for a focused fixture/timing repair; no assertion was removed
  and the offline CI gate remains required.
