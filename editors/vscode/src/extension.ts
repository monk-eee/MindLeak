import * as fs from "fs";
import * as path from "path";
import * as vscode from "vscode";

import { BoardViewProvider } from "./boardViewProvider";
import { GraphViewProvider } from "./graphViewProvider";
import { McpClient } from "./mcpClient";
import { conformanceDiagnostic, resolveBinaryPath, resolveServerPath, toArtifactId } from "./util";

let client: McpClient | undefined;
let lodestar: McpClient | undefined;
let provider: GraphViewProvider | undefined;
let board: BoardViewProvider | undefined;
let diagnostics: vscode.DiagnosticCollection | undefined;
let output: vscode.OutputChannel;

export async function activate(context: vscode.ExtensionContext): Promise<void> {
  output = vscode.window.createOutputChannel("MindLeak");
  context.subscriptions.push(output);

  const workspace = vscode.workspace.workspaceFolders?.[0]?.uri.fsPath ?? process.cwd();
  const config = vscode.workspace.getConfiguration("mindleak");
  const serverPath = resolveServerPath(
    config.get<string>("serverPath", "mindleak-mcp"),
    workspace,
    {
      exists: fs.existsSync,
    }
  );
  const dbPath =
    config.get<string>("databasePath", "") || path.join(workspace, ".mindleak", "graph.db");
  const agentId = config.get<string>("agentId", "vscode");

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
  diagnostics = vscode.languages.createDiagnosticCollection("mindleak-conformance");
  context.subscriptions.push(diagnostics);

  const lodestarPath = resolveBinaryPath(
    config.get<string>("lodestarServerPath", "lodestar-mcp"),
    workspace,
    "lodestar-mcp",
    { exists: fs.existsSync }
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
    provider.status("connected");
    output.appendLine(`Connected to ${serverPath} (db: ${dbPath})`);
  } catch (err) {
    provider.status("server unavailable");
    vscode.window.showWarningMessage(
      `MindLeak: could not start '${serverPath}'. Set 'mindleak.serverPath'. (${(err as Error).message})`
    );
  }

  try {
    await lodestar.start();
    output.appendLine(`Connected to ${lodestarPath} (intent plane: ${lodestarDb})`);
    void refreshBoard();
  } catch (err) {
    output.appendLine(
      `Lodestar intent plane unavailable ('${lodestarPath}'): ${(err as Error).message}`
    );
  }

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
      if (config.get<boolean>("conformanceOnSave", true)) {
        void checkConformance(doc);
      }
    })
  );

  context.subscriptions.push(
    vscode.commands.registerCommand("mindleak.refresh", () => refresh()),
    vscode.commands.registerCommand("mindleak.prune", () => prune()),
    vscode.commands.registerCommand("mindleak.export", () => exportSnapshot()),
    vscode.commands.registerCommand("mindleak.ingestActiveFile", () => {
      const doc = vscode.window.activeTextEditor?.document;
      if (doc) {
        void onSave(doc);
      }
    }),
    vscode.commands.registerCommand("mindleak.board.refresh", () => refreshBoard()),
    vscode.commands.registerCommand("mindleak.checkConformance", () => {
      const doc = vscode.window.activeTextEditor?.document;
      if (doc) {
        void checkConformance(doc);
      }
    })
  );

  // Prime the view with whatever is currently open.
  if (vscode.window.activeTextEditor) {
    void onFocus(vscode.window.activeTextEditor.document);
  }
}

export function deactivate(): void {
  client?.dispose();
  lodestar?.dispose();
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
    const subgraph = await client.callTool("graph_snapshot", { limit: 500 });
    const target = await vscode.window.showSaveDialog({
      filters: { JSON: ["json"] },
      saveLabel: "Export MindLeak Graph",
    });
    if (target) {
      fs.writeFileSync(target.fsPath, JSON.stringify(subgraph, null, 2));
      vscode.window.showInformationMessage(`MindLeak graph exported to ${target.fsPath}`);
    }
  } catch (err) {
    vscode.window.showErrorMessage(`MindLeak export failed: ${(err as Error).message}`);
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

async function checkConformance(doc: vscode.TextDocument): Promise<void> {
  if (!lodestar?.isReady() || !diagnostics || doc.uri.scheme !== "file") {
    return;
  }
  try {
    const result = await lodestar.callTool("check_conformance", {
      change_node_ids: [artifactId(doc)],
    });
    const diag = conformanceDiagnostic(result);
    if (!diag) {
      diagnostics.delete(doc.uri);
      return;
    }
    const severity =
      diag.severity === "error"
        ? vscode.DiagnosticSeverity.Error
        : diag.severity === "warning"
          ? vscode.DiagnosticSeverity.Warning
          : vscode.DiagnosticSeverity.Information;
    const entry = new vscode.Diagnostic(new vscode.Range(0, 0, 0, 1), diag.message, severity);
    entry.source = "MindLeak";
    diagnostics.set(doc.uri, [entry]);
  } catch (err) {
    output.appendLine(`conformance error: ${(err as Error).message}`);
  }
}
