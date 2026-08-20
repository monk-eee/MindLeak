import * as fs from "fs";
import * as path from "path";
import { randomBytes } from "crypto";
import * as vscode from "vscode";

import { BoardItem, BoardViewProvider } from "./boardViewProvider";
import { WorkspaceChangeDetector } from "./changeDetector";
import { DesignBoardController } from "./designBoardController";
import { DesignBoardItem, DesignBoardViewProvider } from "./designBoardViewProvider";
import { DoctorViewProvider } from "./doctorViewProvider";
import { EvidenceBoardViewProvider, EvidenceNode } from "./evidenceBoardViewProvider";
import { doctorGroups } from "./fleet";
import { FleetController } from "./fleetController";
import { FleetViewProvider } from "./fleetViewProvider";
import { GitSensor } from "./gitSensor";
import { GraphViewProvider } from "./graphViewProvider";
import { McpClient } from "./mcpClient";
import { ReadinessSnapshot, sessionAgentIdentity } from "./readiness";
import { ReadinessController, RuntimeHealth } from "./readinessController";
import { ReadinessViewProvider } from "./readinessViewProvider";
import { RepositoryWorktrees } from "./repositoryWorktrees";
import { TaskAllocationController } from "./taskAllocationController";
import { TelemetryViewProvider } from "./telemetryViewProvider";
import { TerminalCaptureConfig, TerminalSensor } from "./terminalSensor";
import {
  canRetireTask,
  configuredPathEnvironment,
  conformanceDiagnostic,
  ConformanceRecord,
  evidenceGroups,
  evidenceRequestForTask,
  formatTaskEvidence,
  GoverningClause,
  healthSummary,
  leaseActionFor,
  LodestarTask,
  logLines,
  pendingQuestion,
  planMcpServers,
  repoRelativePath,
  resolveBinaryPathDetailed,
  serverFilePath,
  shouldPollTelemetry,
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
let boardTree: vscode.TreeView<BoardItem> | undefined;
let allocationController: TaskAllocationController | undefined;
let fleetView: FleetViewProvider | undefined;
let fleetController: FleetController | undefined;
let designBoard: DesignBoardViewProvider | undefined;
let designController: DesignBoardController | undefined;
let evidenceBoard: EvidenceBoardViewProvider | undefined;
let doctorBoard: DoctorViewProvider | undefined;
let doctorTree: vscode.TreeView<import("./doctorViewProvider").DoctorNode> | undefined;
let readinessView: ReadinessViewProvider | undefined;
let readinessController: ReadinessController | undefined;
let repositoryWorktrees: RepositoryWorktrees | undefined;
let output: vscode.OutputChannel;
let configuredAgentId = "vscode";
const health: RuntimeHealth = {
  memory: "memory starting",
  intent: "intent starting",
  terminal: "terminal capture starting",
  git: "Git capture starting",
};

export interface MindLeakExtensionApi {
  health(): {
    memory: string;
    intent: string;
    terminal: string;
    git: string;
  };
  readiness(): ReadinessSnapshot;
}

export async function activate(context: vscode.ExtensionContext): Promise<MindLeakExtensionApi> {
  output = vscode.window.createOutputChannel("MindLeak");
  context.subscriptions.push(output);

  const workspace = vscode.workspace.workspaceFolders?.[0]?.uri.fsPath ?? process.cwd();
  repositoryWorktrees = new RepositoryWorktrees(workspace);
  const config = vscode.workspace.getConfiguration("mindleak");
  const resolvedServer = resolveBinaryPathDetailed(
    config.get<string>("serverPath", "mindleak-mcp"),
    workspace,
    "mindleak-mcp",
    {
      exists: fs.existsSync,
      extensionPath: context.extensionPath,
    }
  );
  const serverPath = resolvedServer.path;
  const databasePathOverride = config.get<string>("databasePath", "");
  // Passed to both servers as their display name only. Since ADR-0054 it is no
  // part of the agent id, so the identity below is derived from the session
  // token alone and does not change with how the process was launched.
  const agentId = config.get<string>("agentId", "vscode");
  // A single MCP request must outlast the bounded model sequence: one DNS/connect
  // budget, two connect+read attempts, and retry backoff. MindLeak's generous
  // 120s model-read default makes that about 243s; five minutes leaves headroom
  // for the success or typed fallback to reach the caller.
  const requestTimeoutMs = config.get<number>("requestTimeoutMs", 300000);
  const sessionId = randomBytes(16).toString("hex");
  configuredAgentId = sessionAgentIdentity(sessionId);

  // Contribute both planes to the editor so chat agents reach the same servers
  // this window already talks to, resolved by the same rule. Before this, a
  // committed `.vscode/mcp.json` named the binaries independently, so the
  // extension and the agent could disagree about which build was in force.
  // Rooting each server at this window's workspace folder is what preserves
  // ADR-0073 across sibling worktrees.
  const mcpServersChanged = new vscode.EventEmitter<void>();
  context.subscriptions.push(mcpServersChanged);
  context.subscriptions.push(
    vscode.lm.registerMcpServerDefinitionProvider("mindleak.servers", {
      onDidChangeMcpServerDefinitions: mcpServersChanged.event,
      provideMcpServerDefinitions: () =>
        planMcpServers(
          workspace,
          agentId,
          {
            memory: config.get<string>("serverPath", "mindleak-mcp"),
            intent: config.get<string>("lodestarServerPath", "lodestar-mcp"),
            memoryDatabase: config.get<string>("databasePath", ""),
            intentDatabase: config.get<string>("lodestarDatabasePath", ""),
          },
          {
            exists: fs.existsSync,
            extensionPath: context.extensionPath,
            version: (candidate) => {
              const stat = fs.statSync(candidate, { throwIfNoEntry: false });
              return stat?.isFile() ? `${stat.size}:${stat.mtimeMs}` : undefined;
            },
          }
        ).map((plan) => {
          const definition = new vscode.McpStdioServerDefinition(
            plan.label,
            plan.command,
            [],
            plan.env
          );
          definition.cwd = vscode.Uri.file(plan.cwd);
          definition.version = plan.version;
          return definition;
        }),
    })
  );
  // A server path or database override changes where a server should run, so the
  // editor has to be told to re-read the definitions rather than keep a stale one.
  context.subscriptions.push(
    vscode.workspace.onDidChangeConfiguration((event) => {
      if (
        [
          "serverPath",
          "lodestarServerPath",
          "databasePath",
          "lodestarDatabasePath",
          "agentId",
        ].some((key) => event.affectsConfiguration(`mindleak.${key}`))
      ) {
        mcpServersChanged.fire();
      }
    })
  );

  client = new McpClient(
    serverPath,
    workspace,
    {
      ...configuredPathEnvironment("MINDLEAK_DB", databasePathOverride),
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
    sessionId,
    (m) => output.appendLine(m),
    requestTimeoutMs
  );
  followConnection("memory", client);

  readinessView = new ReadinessViewProvider();
  context.subscriptions.push(
    vscode.window.registerTreeDataProvider(ReadinessViewProvider.viewType, readinessView)
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
  const telemetryRefreshMs = Math.max(1, config.get<number>("telemetryRefreshSecs", 30)) * 1000;
  const telemetryTimer = setInterval(() => {
    if (telemetry && shouldPollTelemetry(telemetry.isVisible(), telemetry.isLive())) {
      void refreshTelemetry();
    }
  }, telemetryRefreshMs);
  context.subscriptions.push({ dispose: () => clearInterval(telemetryTimer) });

  fleetView = new FleetViewProvider(context.extensionUri, {
    onReady: () => void fleetController?.refresh(),
    onRefresh: () => void fleetController?.refresh(),
    onAct: (verb, taskId, agentId) => void fleetController?.act(verb, taskId, agentId),
    onOpenTask: (taskId) => void revealTask(taskId),
  });
  context.subscriptions.push(
    vscode.window.registerWebviewViewProvider(FleetViewProvider.viewType, fleetView)
  );

  board = new BoardViewProvider(configuredAgentId);
  boardTree = vscode.window.createTreeView(BoardViewProvider.viewType, {
    treeDataProvider: board,
  });
  context.subscriptions.push(boardTree);
  designBoard = new DesignBoardViewProvider();
  context.subscriptions.push(
    vscode.window.registerTreeDataProvider(DesignBoardViewProvider.viewType, designBoard)
  );
  evidenceBoard = new EvidenceBoardViewProvider();
  context.subscriptions.push(
    vscode.window.registerTreeDataProvider(EvidenceBoardViewProvider.viewType, evidenceBoard)
  );
  doctorBoard = new DoctorViewProvider();
  doctorTree = vscode.window.createTreeView(DoctorViewProvider.viewType, {
    treeDataProvider: doctorBoard,
  });
  context.subscriptions.push(doctorTree);
  const lodestarResolved = resolveBinaryPathDetailed(
    config.get<string>("lodestarServerPath", "lodestar-mcp"),
    workspace,
    "lodestar-mcp",
    { exists: fs.existsSync, extensionPath: context.extensionPath }
  );
  const lodestarPath = lodestarResolved.path;
  const lodestarDatabasePathOverride = config.get<string>("lodestarDatabasePath", "");
  lodestar = new McpClient(
    lodestarPath,
    workspace,
    {
      ...configuredPathEnvironment("LODESTAR_DB", lodestarDatabasePathOverride),
      LODESTAR_AGENT: agentId,
      MINDLEAK_WORKSPACE: workspace,
    },
    sessionId,
    (m) => output.appendLine(m),
    requestTimeoutMs
  );
  followConnection("intent", lodestar);
  readinessController = new ReadinessController(
    client,
    lodestar,
    readinessView,
    configuredAgentId,
    () => Boolean(vscode.window.activeTextEditor),
    currentHealth(),
    (message) => output.appendLine(message)
  );
  allocationController = new TaskAllocationController(
    lodestar,
    client,
    board,
    boardTree,
    configuredAgentId,
    refreshBoard,
    (message) => output.appendLine(message)
  );
  designController = new DesignBoardController(
    lodestar,
    designBoard,
    configuredAgentId,
    (message) => output.appendLine(message),
    refreshBoard
  );
  fleetController = new FleetController(
    lodestar,
    client,
    fleetView,
    configuredAgentId,
    (message) => output.appendLine(message),
    refreshBoard
  );
  // The pane polls only while visible: a fleet readout nobody is looking at is
  // the request traffic that starves the idle maintenance worker.
  const fleetTimer = setInterval(
    () => {
      if (fleetView?.isVisible()) {
        void fleetController?.refresh();
      }
    },
    Math.max(2, config.get<number>("fleetRefreshSecs", 30)) * 1000
  );
  context.subscriptions.push({ dispose: () => clearInterval(fleetTimer) });
  const adrWatcher = vscode.workspace.createFileSystemWatcher("**/docs/adr/*.md");
  context.subscriptions.push(
    adrWatcher,
    adrWatcher.onDidCreate(() => void designController?.sync()),
    adrWatcher.onDidChange(() => void designController?.sync()),
    adrWatcher.onDidDelete(() => void designController?.sync())
  );

  try {
    await client.start();
    if (client.agentIdentity() !== configuredAgentId) {
      throw new Error("MindLeak session identity does not match the client contract");
    }
    setHealth("memory", "memory connected");
    output.appendLine(
      `Connected to ${serverPath} (source: ${resolvedServer.source}; db: ${databasePathOverride.trim() || "shared repository state"})`
    );
    if (config.get<boolean>("autoIngestOnSave", true)) {
      void reconcileWorkspace();
    }
  } catch (err) {
    setHealth("memory", `memory unavailable: ${(err as Error).message}`);
    vscode.window.showWarningMessage(
      `MindLeak: could not start '${serverPath}'. Set 'mindleak.serverPath'. (${(err as Error).message})`
    );
  }

  try {
    await lodestar.start();
    if (lodestar.agentIdentity() !== configuredAgentId) {
      throw new Error("Lodestar session identity does not match the memory plane");
    }
    setHealth("intent", "intent connected");
    output.appendLine(
      `Connected to ${lodestarPath} (source: ${lodestarResolved.source}; intent plane: ${lodestarDatabasePathOverride.trim() || "shared repository state"})`
    );
    void refreshBoard();
    void refreshEvidence();
    void designController.sync();
  } catch (err) {
    setHealth("intent", `intent unavailable: ${(err as Error).message}`);
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
    (status) => setHealth("terminal", status)
  );
  const gitSensor = new GitSensor(
    mindleakClient,
    () => vscode.workspace.getConfiguration("mindleak").get<boolean>("captureCommits", true),
    (message) => output.appendLine(message),
    (status) => setHealth("git", status)
  );
  context.subscriptions.push(changeDetector, terminalSensor, gitSensor);
  void gitSensor.start().catch((err) => {
    setHealth("git", "Git capture degraded: startup failed");
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

  // Forget a file's structure when it is deleted or renamed in the editor, so the
  // graph stops carrying symbols for a path that no longer exists instead of
  // waiting for the edges to decay. Editor-mediated events only (not a raw file
  // watcher) so a file briefly absent during a git operation is not reaped.
  context.subscriptions.push(
    vscode.workspace.onDidDeleteFiles((event) => {
      if (!config.get<boolean>("autoIngestOnSave", true)) {
        return;
      }
      for (const uri of event.files) {
        void onDelete(uri);
      }
    }),
    vscode.workspace.onDidRenameFiles((event) => {
      if (!config.get<boolean>("autoIngestOnSave", true)) {
        return;
      }
      for (const file of event.files) {
        void onDelete(file.oldUri);
      }
    })
  );

  context.subscriptions.push(
    vscode.commands.registerCommand("mindleak.readiness.refresh", () =>
      readinessController?.refresh()
    ),
    vscode.commands.registerCommand("mindleak.refresh", () => refresh()),
    vscode.commands.registerCommand("mindleak.fleet.refresh", () => fleetController?.refresh()),
    vscode.commands.registerCommand("mindleak.prune", () => prune()),
    vscode.commands.registerCommand("mindleak.reconcile", () => reconcileWorkspace()),
    vscode.commands.registerCommand("mindleak.export", () => exportSnapshot()),
    vscode.commands.registerCommand("mindleak.backup", () => backupBoth()),
    vscode.commands.registerCommand("mindleak.resetMemory", () => resetMemory()),
    vscode.commands.registerCommand("mindleak.ingestActiveFile", async () => {
      const doc = vscode.window.activeTextEditor?.document;
      if (!doc) {
        vscode.window.showWarningMessage("Open a source file before ingesting workspace context.");
        return;
      }
      await onSave(doc);
    }),
    vscode.commands.registerCommand("mindleak.board.refresh", () => refreshBoard()),
    vscode.commands.registerCommand("mindleak.doctor.refresh", () => refreshDoctor()),
    vscode.commands.registerCommand("mindleak.evidence.refresh", () => refreshEvidence()),
    vscode.commands.registerCommand("mindleak.evidence.inspect", (node?: EvidenceNode) => {
      void inspectEvidenceNode(node);
    }),
    vscode.commands.registerCommand("mindleak.evidence.export", (node?: EvidenceNode) => {
      void exportEvidenceNode(node);
    }),
    vscode.commands.registerCommand("mindleak.task.next", () => allocationController?.revealNext()),
    vscode.commands.registerCommand("mindleak.task.claimForMe", (item?: BoardItem) => {
      void allocationController?.claimForMe(item);
    }),
    vscode.commands.registerCommand("mindleak.task.renew", (item?: BoardItem) => {
      void allocationController?.renew(item);
    }),
    vscode.commands.registerCommand("mindleak.task.release", (item?: BoardItem) => {
      void allocationController?.release(item);
    }),
    vscode.commands.registerCommand("mindleak.task.recover", (item?: BoardItem) => {
      void allocationController?.recover(item);
    }),
    vscode.commands.registerCommand("mindleak.task.acceptReview", (item?: BoardItem) => {
      void allocationController?.accept(item);
    }),
    vscode.commands.registerCommand("mindleak.task.retryReview", (item?: BoardItem) => {
      void allocationController?.retry(item);
    }),
    vscode.commands.registerCommand("mindleak.design.refresh", () => designController?.refresh()),
    vscode.commands.registerCommand("mindleak.design.sync", () => designController?.sync()),
    vscode.commands.registerCommand("mindleak.design.expand", () => designController?.expand()),
    vscode.commands.registerCommand("mindleak.design.toggleArchive", async () => {
      await designController?.toggleArchive();
      await vscode.commands.executeCommand(
        "setContext",
        "mindleak.design.showArchive",
        designBoard?.includeArchive ?? false
      );
    }),
    vscode.commands.registerCommand("mindleak.design.toggleDeferred", async () => {
      await designController?.toggleDeferred();
      await vscode.commands.executeCommand(
        "setContext",
        "mindleak.design.showDeferred",
        designBoard?.includeDeferred ?? false
      );
    }),
    vscode.commands.registerCommand("mindleak.design.defer", (item?: DesignBoardItem) => {
      void designController?.defer(item);
    }),
    vscode.commands.registerCommand("mindleak.design.resume", (item?: DesignBoardItem) => {
      void designController?.resume(item);
    }),
    vscode.commands.registerCommand("mindleak.design.accept", (item?: DesignBoardItem) => {
      void designController?.accept(item);
    }),
    vscode.commands.registerCommand("mindleak.design.reject", (item?: DesignBoardItem) => {
      void designController?.reject(item);
    }),
    vscode.commands.registerCommand("mindleak.design.promote", (item?: DesignBoardItem) => {
      void designController?.promote(item);
    }),
    vscode.commands.registerCommand("mindleak.design.revisePromotion", (item?: DesignBoardItem) => {
      void designController?.revisePromotion(item);
    }),
    vscode.commands.registerCommand("mindleak.design.openAdr", (item?: DesignBoardItem) => {
      void designController?.openAdr(item);
    }),
    vscode.commands.registerCommand(
      "mindleak.design.inspectPromotion",
      (item?: DesignBoardItem) => {
        void designController?.inspectPromotion(item);
      }
    ),
    vscode.commands.registerCommand("mindleak.telemetry.refresh", () => refreshTelemetry()),
    vscode.commands.registerCommand("mindleak.task.completeWithEvidence", (item?: BoardItem) => {
      void completeWithEvidence(item);
    }),
    vscode.commands.registerCommand("mindleak.task.inspectEvidence", (item?: BoardItem) => {
      void inspectTaskEvidence(item);
    }),
    vscode.commands.registerCommand("mindleak.task.answer", (item?: BoardItem) => {
      void answerTaskQuestion(item);
    }),
    vscode.commands.registerCommand("mindleak.task.retire", (item?: BoardItem) => {
      void retireTask(item);
    }),
    vscode.commands.registerCommand("mindleak.task.pause", (item?: BoardItem) => {
      void changeTaskLease("pause", item);
    }),
    vscode.commands.registerCommand("mindleak.task.resume", (item?: BoardItem) => {
      void changeTaskLease("resume", item);
    })
  );

  // Prime the view with whatever is currently open.
  if (vscode.window.activeTextEditor) {
    void onFocus(vscode.window.activeTextEditor.document);
  }

  const readiness = await readinessController.refresh();
  const readinessSeenKey = "mindleak.readiness.seen.v1";
  if (
    !context.workspaceState.get<boolean>(readinessSeenKey) &&
    ["disconnected", "ready_empty"].includes(readiness.state)
  ) {
    await context.workspaceState.update(readinessSeenKey, true);
    void vscode.commands.executeCommand("mindleak.readinessView.focus");
  }

  return {
    health: currentHealth,
    readiness: () => readinessController!.snapshot(),
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

function setHealth(plane: keyof RuntimeHealth, status: string): void {
  if (health[plane] === status) {
    return;
  }
  health[plane] = status;
  output.appendLine(status);
  updateHealth();
}

/**
 * Keep a plane's health following its server. Without this the line recorded
 * at activation outlives the connection, so a dead server still reads
 * "connected" while every pane sits empty.
 */
function followConnection(plane: "memory" | "intent", server: McpClient): void {
  server.onStateChange((state, detail) => {
    switch (state) {
      case "connected":
        setHealth(plane, `${plane} connected`);
        break;
      case "reconnecting":
        setHealth(plane, `${plane} reconnecting (${detail})`);
        break;
      case "disconnected":
        setHealth(plane, `${plane} unavailable: ${detail}`);
        break;
    }
  });
}

function updateHealth(): void {
  provider?.status(healthSummary(health.memory, health.intent, health.terminal, health.git));
  readinessController?.setHealth(currentHealth());
}

function currentHealth(): RuntimeHealth {
  return { ...health };
}

function artifactId(doc: vscode.TextDocument): string | null {
  const rel = repoRelativePath(vscode.workspace.asRelativePath(doc.uri, false));
  return rel === null ? null : toArtifactId(rel);
}

async function onFocus(doc: vscode.TextDocument): Promise<void> {
  if (!client?.isReady() || doc.uri.scheme !== "file") {
    return;
  }
  const id = artifactId(doc);
  if (id === null) {
    // No repo-relative id, so there is no node in this graph to boost.
    return;
  }
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
  if (!(await repositoryWorktrees?.contains(doc.uri.fsPath))) {
    return;
  }
  const sourcePath = serverFilePath(
    vscode.workspace.asRelativePath(doc.uri, false),
    doc.uri.fsPath
  );
  if (sourcePath === null) {
    return;
  }
  try {
    const outcome = await client.callTool("ingest_file", {
      path: sourcePath,
      content: doc.getText(),
    });
    const artifact = outcome?.node_ids?.find((id: string) => id.startsWith("artifact:"));
    if (artifact) {
      await refresh(artifact);
    }
  } catch (err) {
    output.appendLine(`ingest error: ${(err as Error).message}`);
  }
}

async function onDelete(uri: vscode.Uri): Promise<void> {
  if (!client?.isReady() || uri.scheme !== "file") {
    return;
  }
  if (!(await repositoryWorktrees?.contains(uri.fsPath))) {
    return;
  }
  const sourcePath = serverFilePath(vscode.workspace.asRelativePath(uri, false), uri.fsPath);
  if (sourcePath === null) {
    return;
  }
  try {
    await client.callTool("forget_file", { path: sourcePath });
    await refresh();
  } catch (err) {
    output.appendLine(`forget error: ${(err as Error).message}`);
  }
}

// Reconcile the graph against the workspace's real file set: forget artifacts for
// files that no longer exist (deleted/moved outside the editor, or ingested
// before the junk filter existed). The file list is the authoritative truth; the
// server forgets anything not in it. Runs once on activation to clear accumulated
// stale structure, and on demand via the command.
async function reconcileWorkspace(): Promise<void> {
  if (!client?.isReady()) {
    return;
  }
  try {
    const uris = await vscode.workspace.findFiles(
      "**/*",
      "**/{node_modules,target,dist,coverage,.git,.mindleak,.lodestar,.vscode-test,out}/**"
    );
    const paths = uris.map((u) => vscode.workspace.asRelativePath(u, false).replace(/\\/g, "/"));
    const outcome = await client.callTool("reconcile_workspace", { paths });
    if (outcome?.files_forgotten > 0) {
      output.appendLine(
        `Reconciled workspace: forgot ${outcome.files_forgotten} stale file(s) ` +
          `(${outcome.nodes_removed} nodes, ${outcome.edges_removed} edges).`
      );
      await refresh();
    }
  } catch (err) {
    output.appendLine(`reconcile error: ${(err as Error).message}`);
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
    readinessController?.setGraph(stats);
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

/** Focus the board on one task, so a Fleet row can hand off to the fuller view. */
async function revealTask(taskId: string): Promise<void> {
  await refreshBoard();
  const item = board?.find(taskId);
  if (!item || !boardTree) {
    return;
  }
  try {
    await boardTree.reveal(item, { select: true, focus: true });
  } catch {
    // Revealing is a convenience; a tree that cannot reveal must not raise.
  }
}

async function refreshBoard(): Promise<void> {
  if (!lodestar?.isReady() || !board) {
    return;
  }
  try {
    const tasks = await lodestar.callTool("task_query", {
      view: "board",
      include_terminal: false,
    });
    const list: LodestarTask[] = Array.isArray(tasks) ? tasks : [];
    // Enrich claimed tasks with the clauses governing their scope so the board
    // shows what governs the work an agent picked up (ADR-0029). Best-effort:
    // a failed enrichment must never break the board.
    await Promise.all(
      list
        .filter((task) => task.status === "claimed")
        .map(async (task) => {
          try {
            const governing = await lodestar!.callTool("governing_for_task", { task_id: task.id });
            if (Array.isArray(governing)) {
              task.governing = governing as GoverningClause[];
            }
          } catch {
            // Leave the task without governing rather than failing the refresh.
          }
        })
    );
    board.update(list);
    readinessController?.setActionableTasks(list.length);
  } catch (err) {
    output.appendLine(`board error: ${(err as Error).message}`);
  }
  // Doctor findings are cheap relative to the board read above and every
  // trigger that changes the board (claim, complete, block...) can change
  // them too, so this rides the same refresh rather than needing its own.
  await refreshDoctor();
}

async function refreshDoctor(): Promise<void> {
  if (!lodestar?.isReady() || !doctorBoard) {
    return;
  }
  try {
    const findings = await lodestar.callTool("task_query", { view: "doctor" });
    const groups = doctorGroups(Array.isArray(findings) ? findings : []);
    doctorBoard.update(groups);
    if (doctorTree) {
      doctorTree.badge = doctorBoard.findingCount
        ? { value: doctorBoard.findingCount, tooltip: "Board Doctor findings" }
        : undefined;
    }
  } catch (err) {
    output.appendLine(`board doctor error: ${(err as Error).message}`);
  }
}

async function refreshEvidence(): Promise<void> {
  if (!lodestar?.isReady() || !evidenceBoard) {
    return;
  }
  try {
    const tasks = await lodestar.callTool("task_query", {
      view: "board",
      include_terminal: true,
    });
    const list: LodestarTask[] = Array.isArray(tasks) ? tasks : [];
    // Only completed/reviewed/blocked work carries conformance proof; skip the
    // rest so the board is one bounded pass, not a lookup per open task.
    const evidenced = list.filter((task) =>
      ["done", "in_review", "blocked", "abandoned"].includes(task.status)
    );
    const historyByTask: Record<string, ConformanceRecord[]> = {};
    await Promise.all(
      evidenced.map(async (task) => {
        try {
          const records = await lodestar!.callTool("conformance_history", { task_id: task.id });
          if (Array.isArray(records) && records.length) {
            historyByTask[task.id] = records as ConformanceRecord[];
          }
        } catch {
          // A failed lookup must not break the board.
        }
      })
    );
    evidenceBoard.update(evidenceGroups(list, historyByTask));
  } catch (err) {
    output.appendLine(`evidence board error: ${(err as Error).message}`);
  }
}

/** Open a task's conformance chain as readable markdown from the Evidence Board. */
async function inspectEvidenceNode(node?: EvidenceNode): Promise<void> {
  const group = node?.group;
  if (!group) {
    vscode.window.showWarningMessage("Run this from a task on the Evidence Board.");
    return;
  }
  const markdown = formatTaskEvidence(group.records, group.title);
  if (!markdown) {
    vscode.window.showInformationMessage(`No conformance evidence for ${group.title}.`);
    return;
  }
  const doc = await vscode.workspace.openTextDocument({ content: markdown, language: "markdown" });
  await vscode.window.showTextDocument(doc, { preview: true });
}

/** Export a task's proof-of-work to a committed artifact via `export_evidence` (ADR-0031). */
async function exportEvidenceNode(node?: EvidenceNode): Promise<void> {
  if (!lodestar?.isReady()) {
    vscode.window.showWarningMessage("Lodestar must be connected to export evidence.");
    return;
  }
  const group = node?.group;
  if (!group) {
    vscode.window.showWarningMessage("Run this from a task on the Evidence Board.");
    return;
  }
  const safe = group.taskId.replace(/[^a-zA-Z0-9._-]/g, "-");
  const relative = path.join(".lodestar", "evidence", `${safe}.md`);
  try {
    await lodestar.callTool("export_evidence", { task_id: group.taskId, path: relative });
    const root = vscode.workspace.workspaceFolders?.[0]?.uri ?? vscode.Uri.file(".");
    const doc = await vscode.workspace.openTextDocument(vscode.Uri.joinPath(root, relative));
    await vscode.window.showTextDocument(doc, { preview: true });
    vscode.window.showInformationMessage(`Exported evidence to ${relative}`);
  } catch (err) {
    vscode.window.showErrorMessage(`Evidence export failed: ${(err as Error).message}`);
  }
}

async function refreshTelemetry(): Promise<void> {
  if (!client?.isReady() || !telemetry) {
    return;
  }
  const live = telemetry.isLive();
  try {
    const snapshot = (await client.callTool("telemetry_snapshot", {
      limit: live ? 200 : 20,
    })) as TelemetrySnapshot;
    telemetry.update(telemetryDashboard(snapshot), logLines(snapshot, live), live);
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
    const result = await lodestar.callTool("task_transition", {
      task_id: item.task.id,
      to: "complete",
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
    const thread = (await lodestar.callTool("task_query", {
      task_id: item.task.id,
      view: "thread",
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
    await lodestar.callTool("task_transition", {
      task_id: item.task.id,
      to: "answer",
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

async function retireTask(item?: BoardItem): Promise<void> {
  if (!lodestar?.isReady()) {
    vscode.window.showWarningMessage("Lodestar must be connected to retire a task.");
    return;
  }
  if (!item || !canRetireTask(item.task, Math.floor(Date.now() / 1000))) {
    vscode.window.showWarningMessage(
      "Only open, review, blocked, or expired-claim tasks can be retired."
    );
    return;
  }
  const confirmed = await vscode.window.showWarningMessage(
    `Retire "${item.task.title}" from the active board?`,
    {
      modal: true,
      detail: "The task and its conformance history remain durable as abandoned work.",
    },
    "Retire Task"
  );
  if (confirmed !== "Retire Task") {
    return;
  }
  try {
    const result = await lodestar.callTool("task_transition", {
      task_id: item.task.id,
      to: "abandon",
    });
    if (!result?.abandoned) {
      throw new Error("task state changed before it could be retired");
    }
    vscode.window.showInformationMessage(`Retired ${item.task.title}.`);
    await refreshBoard();
  } catch (err) {
    vscode.window.showErrorMessage(`MindLeak task retirement failed: ${(err as Error).message}`);
  }
}

async function changeTaskLease(action: "pause" | "resume", item?: BoardItem): Promise<void> {
  if (!lodestar?.isReady()) {
    vscode.window.showWarningMessage(`Lodestar must be connected to ${action} a task.`);
    return;
  }
  if (!item) {
    vscode.window.showWarningMessage("Run this command from a task in the Intent Board.");
    return;
  }
  if (leaseActionFor(item.task) !== action) {
    vscode.window.showWarningMessage(
      `Task ${item.task.title} cannot be ${action}d (it is ${item.task.status}).`
    );
    return;
  }
  try {
    await lodestar.callTool("task_transition", {
      task_id: item.task.id,
      to: action,
      ...(item.task.owner ? { agent: item.task.owner } : {}),
    });
    vscode.window.showInformationMessage(
      action === "pause"
        ? `MindLeak: paused — ${item.task.title} suspended for its owner.`
        : `MindLeak: resumed — ${item.task.title} is claimed again.`
    );
    await refreshBoard();
  } catch (err) {
    vscode.window.showErrorMessage(`MindLeak ${action} failed: ${(err as Error).message}`);
  }
}
