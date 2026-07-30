// Which test suites cover the repository's scripts, and which a given runner
// does not execute.
//
// Extracted from `script-tests.mjs` so it can be tested: that module spawns the
// suite at import time, so a test importing it would run the tests recursively.
import { readdirSync } from "node:fs";

/// The suites that exist alongside `scripts/*.test.mjs` and are run by something
/// else — today, `editors/vscode/scripts/*.test.mjs` under vitest, from the
/// extension job.
export function siblingSuiteCount(repoRoot, read = readdirSync) {
  try {
    return read(`${repoRoot}/editors/vscode/scripts`).filter((name) =>
      name.endsWith(".test.mjs"),
    ).length;
  } catch {
    return 0;
  }
}

/// A green run must not imply coverage it does not have.
///
/// `script-tests.mjs` runs `scripts/*.test.mjs`. A second set of tests covers
/// the same scripts under vitest, and a full green run here was acted on as
/// though it were the whole story: the claim-gate and completion-offer guidance
/// fix passed every local assertion and failed CI on the mirrored ones, twice,
/// on work that was correct.
///
/// Naming the gap rather than failing on it is deliberate. Running vitest from
/// here would make a pre-push hook depend on the extension's `node_modules`,
/// which is not always installed. The defect is a green result that quietly
/// means "half"; one honest line repairs that.
export function siblingSuiteNotice(count) {
  if (count === 0) {
    return null;
  }
  return (
    `script-tests: NOT running ${count} sibling test files under editors/vscode/scripts — ` +
    "they cover the same scripts under vitest. Run them with " +
    "`npm --prefix editors/vscode test` before trusting a green result here."
  );
}
