import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import process from "node:process";
import { fileURLToPath } from "node:url";

import { runTests } from "@vscode/test-electron";

const extensionDevelopmentPath = path.resolve(
  path.dirname(fileURLToPath(import.meta.url)),
  "..",
  ".."
);
const extensionTestsPath = path.join(
  extensionDevelopmentPath,
  "scripts",
  "extension-host",
  "suite.cjs"
);
const root = fs.mkdtempSync(path.join(os.tmpdir(), "mindleak-extension-host-"));
const workspace = path.join(root, "workspace");
const userData = path.join(root, "user-data");
const extensions = path.join(root, "extensions");
fs.mkdirSync(workspace, { recursive: true });
fs.writeFileSync(
  path.join(workspace, "smoke.ts"),
  "export function extensionHostSmoke(): boolean { return true; }\n"
);

try {
  await runTests({
    version: "1.93.1",
    extensionDevelopmentPath,
    extensionTestsPath,
    extensionTestsEnv: {
      MINDLEAK_EXTENSION_SMOKE_WORKSPACE: workspace,
    },
    launchArgs: [
      workspace,
      `--user-data-dir=${userData}`,
      `--extensions-dir=${extensions}`,
      "--disable-extensions",
      "--disable-workspace-trust",
      "--skip-welcome",
      "--skip-release-notes",
    ],
  });
} catch (error) {
  console.error(`Extension Host smoke failed: ${error.message}`);
  process.exitCode = 1;
} finally {
  fs.rmSync(root, { recursive: true, force: true });
}
