/* eslint-disable @typescript-eslint/no-var-requires */
// Extension Host product smoke: runs inside VS Code, not a unit-test process.
const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");

const vscode = require("vscode");

async function run() {
  const workspace = process.env.MINDLEAK_EXTENSION_SMOKE_WORKSPACE;
  assert.ok(workspace, "smoke workspace environment is required");

  const extension = vscode.extensions.getExtension("monk-eee.mindleak");
  assert.ok(extension, "MindLeak extension was not discovered");
  const api = await extension.activate();
  assert.equal(api.health().memory, "memory connected");
  assert.equal(api.health().intent, "intent connected");

  const commands = await vscode.commands.getCommands(true);
  for (const command of [
    "mindleak.refresh",
    "mindleak.board.refresh",
    "mindleak.task.next",
    "mindleak.task.allocate",
    "mindleak.task.claimForMe",
    "mindleak.task.renew",
    "mindleak.task.release",
    "mindleak.design.refresh",
    "mindleak.design.sync",
    "mindleak.design.accept",
    "mindleak.design.reject",
    "mindleak.design.promote",
    "mindleak.design.openAdr",
    "mindleak.design.inspectPromotion",
    "mindleak.ingestActiveFile",
    "mindleak.export",
    "mindleak.backup",
    "mindleak.resetMemory",
  ]) {
    assert.ok(commands.includes(command), `missing contributed command: ${command}`);
  }

  const document = await vscode.workspace.openTextDocument(path.join(workspace, "smoke.ts"));
  await vscode.window.showTextDocument(document);
  await vscode.commands.executeCommand("mindleak.ingestActiveFile");
  await vscode.commands.executeCommand("mindleak.refresh");
  await vscode.commands.executeCommand("mindleak.board.refresh");
  await vscode.commands.executeCommand("mindleak.task.next");

  const adrDirectory = path.join(workspace, "docs", "adr");
  fs.mkdirSync(adrDirectory, { recursive: true });
  fs.writeFileSync(
    path.join(adrDirectory, "0099-smoke-design.md"),
    "# Smoke design\n\n- Status: Proposed\n"
  );
  await vscode.commands.executeCommand("mindleak.design.sync");
  await vscode.commands.executeCommand("mindleak.design.refresh");

  await waitForFile(path.join(workspace, ".mindleak", "graph.db"));
  await waitForFile(path.join(workspace, ".lodestar", "spec.db"));
  assert.ok(fs.statSync(path.join(workspace, ".mindleak", "graph.db")).size > 0);
  assert.ok(fs.statSync(path.join(workspace, ".lodestar", "spec.db")).size > 0);
}

async function waitForFile(file) {
  const deadline = Date.now() + 10_000;
  while (!fs.existsSync(file)) {
    if (Date.now() >= deadline) {
      throw new Error(`timed out waiting for ${file}`);
    }
    await new Promise((resolve) => setTimeout(resolve, 50));
  }
}

module.exports = { run };
