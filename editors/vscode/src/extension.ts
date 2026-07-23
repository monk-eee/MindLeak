import * as fs from "fs";
import * as path from "path";
import * as vscode from "vscode";

import { BoardItem, BoardViewProvider } from "./boardViewProvider";
import { WorkspaceChangeDetector } from "./changeDetector";
import { GitSensor } from "./gitSensor";
import { GraphViewProvider } from "./graphViewProvider";
import { McpClient } from "./mcpClient";
import { TerminalCaptureConfig, TerminalSensor } from "./terminalSensor";
import {
  conformanceDiagnostic,
  evidenceRequestForTask,
  healthSummary,
  resolveBinaryPath,
  resolveServerPath,
  toArtifactId,
} from "./util";

let client: McpClient | undefined;
let lodestar: McpClient | undefined;
let provider: GraphViewProvider | undefined;
let board: BoardViewProvider | undefined;
let output: vscode.OutputChannel;
let configuredAgentId = "vscode";
let serverHealth = "memory starting";
let intentHealth = "intent starting";
let terminalHealth = "terminal capture starting";
let gitHealth = "Git capture starting";

export interface MindLeakExtensionApi {
  health(): {
    memory: string;
    intent: string;
    terminal: string;
    git: string;
  };
}

export async function activate(context: vscode.ExtensionContext): Promise<MindLeakExtensionApi> {
  output = vscode.window.createOutputChannel("MindLeak");
  context.subscriptions.push(output);

  const workspace = vscode.workspace.workspaceFolders?.[0]?.uri.fsPath ?? process.cwd();
  const config = vscode.workspace.getConfiguration("mindleak");
  const serverPath = resolveServerPath(
    config.get<string>("serverPath", "mindleak-mcp"),
    workspace,
    {
      exists: fs.existsSync,
      extensionPath: context.extensionPath,
    }
  );
  const dbPath =
    config.get<string>("databasePath", "") || path.join(workspace, ".mindleak", "graph.db");
  const agentId = config.get<string>("agentId", "vscode");
  configuredAgentId = agentId;

  client = new McpClient(
    serverPath,
    workspace,
    { MINDLEAK_DB: dbPath, MINDLEAK_AGENT: agentId },
    (m) => output.appendLine(m)
  );

  provider = new GraphViewProvider(context.extensionUri, {
    onReady: () => void refresh(),
    onRefresh: () => void refresh(),
    onPrune: () => void prune(),
    onExport: () => void exportSnapshot(),
  });
  context.subscriptions.push(
    vscode.window.registerWebviewViewProvider(GraphViewProvider.viewType, provider)
  );

  board = new BoardViewProvider();
  context.subscriptions.push(
    vscode.window.registerTreeDataProvider(BoardViewProvider.viewType, board)
  );
  const lodestarPath = resolveBinaryPath(
    config.get<string>("lodestarServerPath", "lodestar-mcp"),
    workspace,
    "lodestar-mcp",
    { exists: fs.existsSync, extensionPath: context.extensionPath }
  );
  const lodestarDb =
    config.get<string>("lodestarDatabasePath", "") || path.join(workspace, ".lodestar", "spec.db");
  lodestar = new McpClient(
    lodestarPath,
    workspace,
    { LODESTAR_DB: lodestarDb, LODESTAR_AGENT: agentId },
    (m) => output.appendLine(m)
  );

  try {
    await client.start();
    serverHealth = "memory connected";
    updateHealth();
    output.appendLine(`Connected to ${serverPath} (db: ${dbPath})`);
  } catch (err) {
    serverHealth = "memory unavailable";
    updateHealth();
    vscode.window.showWarningMessage(
      `MindLeak: could not start '${serverPath}'. Set 'mindleak.serverPath'. (${(err as Error).message})`
    );
  }

  try {
    await lodestar.start();
    intentHealth = "intent connected";
    updateHealth();
    output.appendLine(`Connected to ${lodestarPath} (intent plane: ${lodestarDb})`);
    void refreshBoard();
  } catch (err) {
    intentHealth = "intent unavailable";
    updateHealth();
    output.appendLine(
      `Lodestar intent plane unavailable ('${lodestarPath}'): ${(err as Error).message}`
    );
  }

  const mindleakClient = client;
  const changeDetector = new WorkspaceChangeDetector();
  const terminalSensor = new TerminalSensor(
    mindleakClient,
    workspace,
    changeDetector,
    terminalCaptureConfig,
    (message) => output.appendLine(message),
    (status) => setTerminalHealth(status)
  );
  const gitSensor = new GitSensor(
    mindleakClient,
    () => vscode.workspace.getConfiguration("mindleak").get<boolean>("captureCommits", true),
    (message) => output.appendLine(message),
    (status) => setGitHealth(status)
  );
  context.subscriptions.push(changeDetector, terminalSensor, gitSensor);
  void gitSensor.start().catch((err) => {
    setGitHealth("Git capture degraded: startup failed");
    output.appendLine(`Git capture startup error: ${(err as Error).message}`);
  });

  // Passive sensors: focus boosts a node; save ingests its symbols.
  context.subscriptions.push(
    vscode.window.onDidChangeActiveTextEditor((editor) => {
      if (editor) {
        void onFocus(editor.document);
      }
    })
  );
  context.subscriptions.push(
    vscode.workspace.onDidSaveTextDocument((doc) => {
      if (config.get<boolean>("autoIngestOnSave", true)) {
        void onSave(doc);
      }
    })
  );

  context.subscriptions.push(
    vscode.commands.registerCommand("mindleak.refresh", () => refresh()),
    vscode.commands.registerCommand("mindleak.prune", () => prune()),
    vscode.commands.registerCommand("mindleak.export", () => exportSnapshot()),
    vscode.commands.registerCommand("mindleak.backup", () => backupBoth()),
    vscode.commands.registerCommand("mindleak.resetMemory", () => resetMemory()),
    vscode.commands.registerCommand("mindleak.ingestActiveFile", () => {
      const doc = vscode.window.activeTextEditor?.document;
      if (doc) {
        void onSave(doc);
      }
    }),
    vscode.commands.registerCommand("mindleak.board.refresh", () => refreshBoard()),
    vscode.commands.registerCommand("mindleak.task.completeWithEvidence", (item?: BoardItem) => {
      void completeWithEvidence(item);
    })
  );

  // Prime the view with whatever is currently open.
  if (vscode.window.activeTextEditor) {
    void onFocus(vscode.window.activeTextEditor.document);
  }

  return {
    health: () => ({
      memory: serverHealth,
      intent: intentHealth,
      terminal: terminalHealth,
      git: gitHealth,
    }),
  };
}

export function deactivate(): void {
  client?.dispose();
  lodestar?.dispose();
}

function terminalCaptureConfig(): TerminalCaptureConfig {
  const config = vscode.workspace.getConfiguration("mindleak");
  return {
    enabled: config.get<boolean>("captureExecutions", true),
    captureOutput: config.get<boolean>("captureTerminalOutput", false),
    maxOutputChars: Math.max(0, config.get<number>("terminalOutputMaxChars", 8192)),
    maxChangedFiles: Math.max(0, config.get<number>("maxChangedFilesPerExecution", 200)),
    excludedPathPrefixes: config.get<string[]>("captureExcludePathPrefixes", []),
  };
}

function setTerminalHealth(status: string): void {
  if (terminalHealth !== status) {
    terminalHealth = status;
    output.appendLine(status);
    updateHealth();
  }
}

function setGitHealth(status: string): void {
  if (gitHealth !== status) {
    gitHealth = status;
    output.appendLine(status);
    updateHealth();
  }
}

function updateHealth(): void {
  provider?.status(healthSummary(serverHealth, intentHealth, terminalHealth, gitHealth));
}

function artifactId(doc: vscode.TextDocument): string {
  return toArtifactId(vscode.workspace.asRelativePath(doc.uri, false));
}

async function onFocus(doc: vscode.TextDocument): Promise<void> {
  if (!client?.isReady() || doc.uri.scheme !== "file") {
    return;
  }
  const id = artifactId(doc);
  try {
    await client.callTool("boost_entity", { id });
    await refresh(id);
  } catch (err) {
    output.appendLine(`focus error: ${(err as Error).message}`);
  }
}

async function onSave(doc: vscode.TextDocument): Promise<void> {
  if (!client?.isReady() || doc.uri.scheme !== "file") {
    return;
  }
  const rel = vscode.workspace.asRelativePath(doc.uri, false).replace(/\\/g, "/");
  try {
    await client.callTool("ingest_file", { path: rel, content: doc.getText() });
    await refresh(`artifact:${rel}`);
  } catch (err) {
    output.appendLine(`ingest error: ${(err as Error).message}`);
  }
}

async function refresh(seed?: string): Promise<void> {
  if (!client?.isReady() || !provider) {
    return;
  }
  const limit = vscode.workspace.getConfiguration("mindleak").get<number>("snapshotLimit", 60);
  const activeSeed =
    seed ??
    (vscode.window.activeTextEditor
      ? artifactId(vscode.window.activeTextEditor.document)
      : undefined);
  try {
    const args: Record<string, unknown> = { limit };
    if (activeSeed) {
      args.seed = activeSeed;
    }
    const subgraph = await client.callTool("graph_snapshot", args);
    const stats = await client.callTool("graph_stats", {});
    provider.update(subgraph, stats);
  } catch (err) {
    output.appendLine(`refresh error: ${(err as Error).message}`);
  }
}

async function prune(): Promise<void> {
  if (!client?.isReady()) {
    return;
  }
  try {
    const res = await client.callTool("prune_graph", {});
    vscode.window.showInformationMessage(
      `MindLeak pruned ${res.edges_removed} edges, ${res.nodes_removed} nodes.`
    );
    await refresh();
  } catch (err) {
    vscode.window.showErrorMessage(`MindLeak prune failed: ${(err as Error).message}`);
  }
}

async function exportSnapshot(): Promise<void> {
  if (!client?.isReady()) {
    return;
  }
  try {
    const graph = await client.callTool("export_graph", {});
    const target = await vscode.window.showSaveDialog({
      filters: { JSON: ["json"] },
      saveLabel: "Export MindLeak Graph",
    });
    if (target) {
      fs.writeFileSync(target.fsPath, JSON.stringify(graph, null, 2));
      vscode.window.showInformationMessage(`MindLeak graph exported to ${target.fsPath}`);
    }
  } catch (err) {
    vscode.window.showErrorMessage(`MindLeak export failed: ${(err as Error).message}`);
  }
}

async function backupBoth(): Promise<void> {
  if (!client?.isReady() || !lodestar?.isReady()) {
    vscode.window.showWarningMessage("MindLeak and Lodestar must both be connected.");
    return;
  }
  const selected = await vscode.window.showOpenDialog({
    canSelectFiles: false,
    canSelectFolders: true,
    canSelectMany: false,
    openLabel: "Back Up Both Planes",
  });
  if (!selected?.[0]) {
    return;
  }
  const timestamp = new Date().toISOString().replace(/[:.]/g, "-");
  const memoryPath = path.join(selected[0].fsPath, `mindleak-${timestamp}.db`);
  const intentPath = path.join(selected[0].fsPath, `lodestar-${timestamp}.db`);
  try {
    await client.callTool("backup_database", { path: memoryPath });
    await lodestar.callTool("backup_database", { path: intentPath });
    vscode.window.showInformationMessage(`MindLeak backups created in ${selected[0].fsPath}`);
  } catch (err) {
    vscode.window.showErrorMessage(`MindLeak backup failed: ${(err as Error).message}`);
  }
}

async function resetMemory(): Promise<void> {
  if (!client?.isReady()) {
    vscode.window.showWarningMessage("MindLeak memory plane is not connected.");
    return;
  }
  const confirmed = await vscode.window.showWarningMessage(
    "Reset all MindLeak memory for this workspace?",
    {
      modal: true,
      detail: "This clears the graph, embeddings, and telemetry. Lodestar intent is preserved.",
    },
    "Reset Memory"
  );
  if (confirmed !== "Reset Memory") {
    return;
  }
  try {
    await client.callTool("reset_database", { confirm: "RESET MINDLEAK" });
    vscode.window.showInformationMessage("MindLeak memory reset. Lodestar intent was preserved.");
    await refresh();
  } catch (err) {
    vscode.window.showErrorMessage(`MindLeak reset failed: ${(err as Error).message}`);
  }
}

async function refreshBoard(): Promise<void> {
  if (!lodestar?.isReady() || !board) {
    return;
  }
  try {
    const tasks = await lodestar.callTool("board", {});
    board.update(Array.isArray(tasks) ? tasks : []);
  } catch (err) {
    output.appendLine(`board error: ${(err as Error).message}`);
  }
}

async function completeWithEvidence(item?: BoardItem): Promise<void> {
  if (!client?.isReady() || !lodestar?.isReady()) {
    vscode.window.showWarningMessage("MindLeak and Lodestar must both be connected.");
    return;
  }
  if (!item) {
    vscode.window.showWarningMessage("Run this command from a claimed task in the Intent Board.");
    return;
  }
  try {
    const request = evidenceRequestForTask(
      item.task,
      configuredAgentId,
      Math.floor(Date.now() / 1000)
    );
    const evidence = await client.callTool("evidence_for", { ...request });
    const result = await lodestar.callTool("complete_task", {
      task_id: item.task.id,
      evidence,
    });
    const conformance = result.conformance ?? result;
    const diagnostic = conformanceDiagnostic(conformance);
    const message = diagnostic?.message ?? `MindLeak conformance: aligned — ${item.task.title}`;
    if (diagnostic?.severity === "error") {
      vscode.window.showErrorMessage(message);
    } else if (diagnostic?.severity === "warning") {
      vscode.window.showWarningMessage(message);
    } else {
      vscode.window.showInformationMessage(message);
    }
    await refreshBoard();
  } catch (err) {
    vscode.window.showErrorMessage(
      `MindLeak evidence completion failed: ${(err as Error).message}`
    );
  }
}
