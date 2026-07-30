- **A green local test run meant half of what it said.** `script-tests.mjs` runs
  `scripts/*.test.mjs` under `node:test`. A second suite —
  `editors/vscode/scripts/*.test.mjs`, run by vitest from the extension job —
  covers the same scripts, and the runner neither ran it nor mentioned it. A
  full green run therefore reported success over 18 of 33 test files while
  naming none of the gap.
  It was acted on: the claim-gate and completion-offer guidance fix passed every
  local assertion, then failed CI on the mirrored ones — twice, across two pull
  requests, on work that was correct. The mirrors asserted the retired verbs in
  the message text, and one of them builds a fake MCP server that answers by
  tool name, so a collapsed verb reaches a fixture that replies to nothing.
  The runner now names what it does not run and the command that does run it.
  Failing instead was considered and rejected: driving vitest from here would
  make a pre-push hook depend on the extension's `node_modules`, which is not
  always installed. The defect was never the missing execution — it was a green
  result that quietly meant "half", and one honest line repairs that.
  The check moved into `scripts/script-suites.mjs` so it can be tested at all:
  importing the runner executes it, which is why this had no test and why it
  went unnoticed. A test also asserts the mirror is still discoverable, so if it
  moves the notice cannot silently stop appearing — the same rot it exists to
  report.
