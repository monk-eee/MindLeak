import * as fs from "fs";
import * as path from "path";
import * as vscode from "vscode";

import { BoardItem, BoardViewProvider } from "./boardViewProvider";
import { WorkspaceChangeDetector } from "./changeDetector";
import { GitSensor } from "./gitSensor";
import { GraphViewProvider } from "./graphViewProvider";
import { McpClient } from "./mcpClient";
import { TelemetryViewProvider } from "./telemetryViewProvider";
import { TerminalCaptureConfig, TerminalSensor } from "./terminalSensor";
import {
  conformanceDiagnostic,
  ConformanceRecord,
  evidenceRequestForTask,
  formatTaskEvidence,
  GraphCounts,
  healthSummary,
  logLines,
  pendingQuestion,
  resolveBinaryPath,
  resolveServerPath,
  TaskQaEntry,
  telemetryDashboard,
  TelemetrySnapshot,
  toArtifactId,
} from "./util";

let client: McpClient | undefined;
let lodestar: McpClient | undefined;
let provider: GraphViewProvider | undefined;
let telemetry: TelemetryViewProvider | undefined;
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
    {
      MINDLEAK_DB: dbPath,
      MINDLEAK_AGENT: agentId,
      MINDLEAK_WORKSPACE: workspace,
      MINDLEAK_AUTONOMOUS_CONSOLIDATION: String(
        config.get<boolean>("autonomousConsolidation", false)
      ),
      MINDLEAK_CONSOLIDATE_IDLE_SECS: String(config.get<number>("consolidateIdleSecs", 300)),
      MINDLEAK_CONSOLIDATE_MIN_INTERVAL_SECS: String(
        config.get<number>("consolidateMinIntervalSecs", 3600)
      ),
      MINDLEAK_CONSOLIDATE_MAX_NODES: String(config.get<number>("consolidateMaxNodes", 20)),
    },
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

  telemetry = new TelemetryViewProvider(context.extensionUri, {
    onReady: () => void refreshTelemetry(),
    onRefresh: () => void refreshTelemetry(),
    onToggleLive: () => void refreshTelemetry(),
  });
  context.subscriptions.push(
    vscode.window.registerWebviewViewProvider(TelemetryViewProvider.viewType, telemetry)
  );
  const telemetryRefreshMs = Math.max(1, config.get<number>("telemetryRefreshSecs", 3)) * 1000;
  const telemetryTimer = setInterval(() => {
    if (telemetry?.isVisible()) {
      void refreshTelemetry();
    }
  }, telemetryRefreshMs);
  context.subscriptions.push({ dispose: () => clearInterval(telemetryTimer) });

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
    vscode.commands.registerCommand("mindleak.telemetry.refresh", () => refreshTelemetry()),
    vscode.commands.registerCommand("mindleak.task.completeWithEvidence", (item?: BoardItem) => {
      void completeWithEvidence(item);
    }),
    vscode.commands.registerCommand("mindleak.task.inspectEvidence", (item?: BoardItem) => {
      void inspectTaskEvidence(item);
    }),
    vscode.commands.registerCommand("mindleak.task.answer", (item?: BoardItem) => {
      void answerTaskQuestion(item);
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

export async function deactivate(): Promise<void> {
  await Promise.all([client?.dispose(), lodestar?.dispose()]);
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
    const tasks = await lodestar.callTool("board", { include_terminal: false });
    board.update(Array.isArray(tasks) ? tasks : []);
  } catch (err) {
    output.appendLine(`board error: ${(err as Error).message}`);
  }
}

async function refreshTelemetry(): Promise<void> {
  if (!client?.isReady() || !telemetry) {
    return;
  }
  const live = telemetry.isLive();
  try {
    const counts = (await client.callTool("graph_stats", {})) as GraphCounts;
    const snapshot = (await client.callTool("telemetry_snapshot", {
      limit: live ? 200 : 20,
    })) as TelemetrySnapshot;
    telemetry.update(telemetryDashboard(snapshot, counts), logLines(snapshot, live), live);
  } catch (err) {
    output.appendLine(`telemetry error: ${(err as Error).message}`);
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

async function inspectTaskEvidence(item?: BoardItem): Promise<void> {
  if (!lodestar?.isReady()) {
    vscode.window.showWarningMessage("Lodestar must be connected to inspect task evidence.");
    return;
  }
  if (!item) {
    vscode.window.showWarningMessage("Run this command from a task in the Intent Board.");
    return;
  }
  try {
    const records = (await lodestar.callTool("conformance_history", {
      task_id: item.task.id,
    })) as ConformanceRecord[];
    const markdown = formatTaskEvidence(records, item.task.title);
    if (!markdown) {
      vscode.window.showInformationMessage(
        `No conformance evidence recorded for ${item.task.title}.`
      );
      return;
    }
    const doc = await vscode.workspace.openTextDocument({
      content: markdown,
      language: "markdown",
    });
    await vscode.window.showTextDocument(doc, { preview: true });
  } catch (err) {
    vscode.window.showErrorMessage(
      `MindLeak evidence inspection failed: ${(err as Error).message}`
    );
  }
}

async function answerTaskQuestion(item?: BoardItem): Promise<void> {
  if (!lodestar?.isReady()) {
    vscode.window.showWarningMessage("Lodestar must be connected to answer a task question.");
    return;
  }
  if (!item) {
    vscode.window.showWarningMessage(
      "Run this command from a task awaiting input in the Intent Board."
    );
    return;
  }
  if (item.task.status !== "needs_input") {
    vscode.window.showWarningMessage(`Task ${item.task.title} is not awaiting input.`);
    return;
  }
  try {
    const thread = (await lodestar.callTool("task_qa", {
      task_id: item.task.id,
    })) as TaskQaEntry[];
    const question = pendingQuestion(Array.isArray(thread) ? thread : []);
    const answer = await vscode.window.showInputBox({
      title: `Answer: ${item.task.title}`,
      prompt: question ?? "Provide the answer for this task.",
      ignoreFocusOut: true,
      validateInput: (value) => (value.trim() ? undefined : "An answer is required."),
    });
    if (answer === undefined) {
      return; // cancelled
    }
    await lodestar.callTool("answer", {
      task_id: item.task.id,
      answer,
      author: "human",
    });
    vscode.window.showInformationMessage(
      `MindLeak: answered — ${item.task.title} resumed for its owner.`
    );
    await refreshBoard();
  } catch (err) {
    vscode.window.showErrorMessage(`MindLeak answer failed: ${(err as Error).message}`);
  }
}
