import * as vscode from "vscode";

import { PathChangeDetector } from "./changeDetector";
import { filterChangedPaths, redactTerminalOutput, shouldCaptureCommand } from "./util";

export interface SensorClient {
  isReady(): boolean;
  callTool(name: string, args: Record<string, unknown>): Promise<unknown>;
}

export interface TerminalCaptureConfig {
  enabled: boolean;
  captureOutput: boolean;
  maxOutputChars: number;
  maxChangedFiles: number;
  excludedPathPrefixes: string[];
}

interface RunningExecution {
  command: string;
  cwd?: string;
  timestamp: number;
  changedPaths: Set<string>;
  output: Promise<string>;
  config: TerminalCaptureConfig;
}

/** Captures shell-integrated terminal executions without intercepting the shell. */
export class TerminalSensor implements vscode.Disposable {
  private readonly subscriptions: vscode.Disposable[] = [];
  private readonly running = new Map<vscode.TerminalShellExecution, RunningExecution>();

  constructor(
    private readonly client: SensorClient,
    private readonly workspace: string,
    changeDetector: PathChangeDetector,
    private readonly getConfig: () => TerminalCaptureConfig,
    private readonly log: (message: string) => void,
    private readonly health: (status: string) => void
  ) {
    this.subscriptions.push(
      vscode.window.onDidStartTerminalShellExecution((event) => this.onStart(event)),
      vscode.window.onDidEndTerminalShellExecution((event) => void this.onEnd(event)),
      vscode.window.onDidChangeTerminalShellIntegration(() => this.updateHealth()),
      vscode.window.onDidOpenTerminal(() => this.updateHealth()),
      vscode.window.onDidCloseTerminal(() => this.updateHealth()),
      vscode.workspace.onDidChangeConfiguration(() => this.updateHealth()),
      changeDetector.onDidChange((path) => this.recordChangedPath(path))
    );
    this.updateHealth();
  }

  dispose(): void {
    for (const subscription of this.subscriptions) {
      subscription.dispose();
    }
    this.running.clear();
  }

  private onStart(event: vscode.TerminalShellExecutionStartEvent): void {
    const config = this.getConfig();
    const commandLine = event.execution.commandLine;
    if (!config.enabled || !shouldCaptureCommand(commandLine.value, commandLine.confidence)) {
      return;
    }
    this.health("terminal capture active");
    this.running.set(event.execution, {
      command: commandLine.value,
      cwd: event.execution.cwd?.scheme === "file" ? event.execution.cwd.fsPath : undefined,
      timestamp: Math.floor(Date.now() / 1000),
      changedPaths: new Set<string>(),
      output:
        config.captureOutput && config.maxOutputChars > 0
          ? collectOutput(event.execution.read(), config.maxOutputChars).catch((error) => {
              this.health("terminal capture degraded: output stream failed");
              this.log(`terminal output capture error: ${(error as Error).message}`);
              return "";
            })
          : Promise.resolve(""),
      config,
    });
  }

  private async onEnd(event: vscode.TerminalShellExecutionEndEvent): Promise<void> {
    const running = this.running.get(event.execution);
    if (!running) {
      return;
    }
    this.running.delete(event.execution);
    const commandLine = event.execution.commandLine;
    if (!shouldCaptureCommand(commandLine.value, commandLine.confidence)) {
      this.log("terminal capture suppressed a command after confidence/privacy recheck");
      return;
    }
    if (event.exitCode === undefined) {
      this.health("terminal capture degraded: exit code unavailable");
      this.log("terminal capture skipped an execution with no exit code");
      return;
    }

    const rawOutput = await running.output;
    if (!this.client.isReady()) {
      return;
    }
    try {
      await this.client.callTool("ingest_execution", {
        command: commandLine.value || running.command,
        exit_code: event.exitCode,
        output: running.config.captureOutput
          ? redactTerminalOutput(rawOutput, running.config.maxOutputChars)
          : "",
        cwd: running.cwd ?? this.workspace,
        changed_files: filterChangedPaths(
          running.changedPaths,
          running.config.excludedPathPrefixes,
          running.config.maxChangedFiles
        ),
        timestamp: running.timestamp,
      });
    } catch (error) {
      this.health("terminal capture degraded: ingestion failed");
      this.log(`terminal capture error: ${(error as Error).message}`);
    }
  }

  private recordChangedPath(path: string): void {
    for (const execution of this.running.values()) {
      if (
        execution.changedPaths.size < execution.config.maxChangedFiles &&
        filterChangedPaths([path], execution.config.excludedPathPrefixes, 1).length === 1
      ) {
        execution.changedPaths.add(path);
      }
    }
  }

  private updateHealth(): void {
    if (!this.getConfig().enabled) {
      this.health("terminal capture disabled");
      return;
    }
    if (vscode.window.terminals.length === 0) {
      this.health("terminal capture waiting for a terminal");
      return;
    }
    this.health(
      vscode.window.terminals.some((terminal) => terminal.shellIntegration)
        ? "terminal capture active"
        : "terminal capture degraded: shell integration unavailable"
    );
  }
}

async function collectOutput(stream: AsyncIterable<string>, maxChars: number): Promise<string> {
  let output = "";
  for await (const chunk of stream) {
    if (output.length < maxChars) {
      output += chunk.slice(0, maxChars - output.length);
    }
  }
  return output;
}
