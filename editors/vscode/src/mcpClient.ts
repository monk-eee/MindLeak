import { ChildProcessWithoutNullStreams, spawn } from "child_process";
import * as readline from "readline";

import { parseToolResult } from "./util";

interface Pending {
  resolve: (value: any) => void;
  reject: (err: any) => void;
  timer?: ReturnType<typeof setTimeout>;
}

export interface McpServerIdentity {
  name: string;
  version: string;
}

/**
 * A minimal MCP client speaking newline-delimited JSON-RPC 2.0 to the
 * mindleak-mcp server over stdio.
 */
export class McpClient {
  private proc?: ChildProcessWithoutNullStreams;
  private nextId = 1;
  private pending = new Map<number, Pending>();
  private ready = false;
  private identity?: McpServerIdentity;

  constructor(
    private readonly command: string,
    private readonly cwd: string,
    private readonly env: NodeJS.ProcessEnv,
    private readonly log: (message: string) => void,
    private readonly requestTimeoutMs = 30_000
  ) {}

  async start(): Promise<void> {
    this.proc = spawn(this.command, [], {
      cwd: this.cwd,
      env: { ...process.env, ...this.env },
    });

    this.proc.on("error", (err) => {
      this.log(`spawn error: ${err.message}`);
      this.rejectPending(new Error(`MCP server spawn error: ${err.message}`));
    });
    this.proc.on("exit", (code) => {
      this.ready = false;
      this.identity = undefined;
      this.rejectPending(new Error(`MCP server exited (code ${code ?? "null"})`));
      this.log(`mindleak-mcp exited (code ${code ?? "null"})`);
    });

    const rl = readline.createInterface({ input: this.proc.stdout });
    rl.on("line", (line) => this.onLine(line));
    this.proc.stderr.on("data", (chunk) => this.log(`[mindleak-mcp] ${chunk.toString().trim()}`));
    this.proc.stdin.on("error", (err) => this.log(`stdin error: ${err.message}`));

    const initialized = await this.request("initialize", {
      protocolVersion: "2024-11-05",
      capabilities: {},
      clientInfo: { name: "mindleak-vscode", version: "0.1.0" },
    });
    const info = initialized?.serverInfo;
    this.identity =
      typeof info?.name === "string" && typeof info?.version === "string"
        ? { name: info.name, version: info.version }
        : undefined;
    this.notify("notifications/initialized", {});
    this.ready = true;
  }

  isReady(): boolean {
    return this.ready;
  }

  serverIdentity(): McpServerIdentity | undefined {
    return this.identity ? { ...this.identity } : undefined;
  }

  private onLine(line: string): void {
    const trimmed = line.trim();
    if (!trimmed) {
      return;
    }
    let msg: any;
    try {
      msg = JSON.parse(trimmed);
    } catch {
      this.log(`unparseable line: ${trimmed.slice(0, 200)}`);
      return;
    }
    if (typeof msg.id !== "number") {
      return;
    }
    const pending = this.pending.get(msg.id);
    if (!pending) {
      return;
    }
    this.pending.delete(msg.id);
    if (pending.timer) {
      clearTimeout(pending.timer);
    }
    if (msg.error) {
      pending.reject(new Error(msg.error.message ?? "MCP error"));
    } else {
      pending.resolve(msg.result);
    }
  }

  private request(method: string, params: unknown): Promise<any> {
    if (!this.proc) {
      return Promise.reject(new Error("MCP server not started"));
    }
    const id = this.nextId++;
    const payload = { jsonrpc: "2.0", id, method, params };
    return new Promise((resolve, reject) => {
      const timer = setTimeout(() => {
        if (this.pending.delete(id)) {
          reject(new Error(`MCP request "${method}" timed out after ${this.requestTimeoutMs}ms`));
        }
      }, this.requestTimeoutMs);
      this.pending.set(id, { resolve, reject, timer });
      try {
        this.proc!.stdin.write(JSON.stringify(payload) + "\n");
      } catch (err) {
        clearTimeout(timer);
        this.pending.delete(id);
        reject(err instanceof Error ? err : new Error(String(err)));
      }
    });
  }

  private notify(method: string, params: unknown): void {
    if (!this.proc) {
      return;
    }
    this.proc.stdin.write(JSON.stringify({ jsonrpc: "2.0", method, params }) + "\n");
  }

  /** Call an MCP tool and parse its first text-content block as JSON. */
  async callTool(name: string, args: Record<string, unknown>): Promise<any> {
    const result = await this.request("tools/call", { name, arguments: args });
    if (result?.isError) {
      const text = result?.content?.[0]?.text ?? "tool error";
      throw new Error(text);
    }
    return parseToolResult(result);
  }

  async dispose(graceMilliseconds = 2000, forceMilliseconds = 1000): Promise<void> {
    const proc = this.proc;
    this.proc = undefined;
    this.ready = false;
    this.identity = undefined;
    this.rejectPending(new Error("MCP client disposed"));
    if (!proc || proc.exitCode !== null) {
      return;
    }
    await new Promise<void>((resolve) => {
      let completed = false;
      let forceTimer: NodeJS.Timeout | undefined;
      const finish = () => {
        if (!completed) {
          completed = true;
          clearTimeout(timer);
          if (forceTimer) {
            clearTimeout(forceTimer);
          }
          resolve();
        }
      };
      proc.once("exit", finish);
      const timer = setTimeout(
        () => {
          if (!proc.kill()) {
            finish();
            return;
          }
          forceTimer = setTimeout(
            () => {
              proc.kill("SIGKILL");
              finish();
            },
            Math.max(0, forceMilliseconds)
          );
        },
        Math.max(0, graceMilliseconds)
      );
      proc.stdin.end();
    });
  }

  private rejectPending(error: Error): void {
    for (const pending of this.pending.values()) {
      if (pending.timer) {
        clearTimeout(pending.timer);
      }
      pending.reject(error);
    }
    this.pending.clear();
  }
}
