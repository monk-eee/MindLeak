import { ChildProcessWithoutNullStreams, spawn } from "child_process";
import * as readline from "readline";

import { parseToolResult } from "./util";

interface Pending {
  resolve: (value: any) => void;
  reject: (err: any) => void;
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

  constructor(
    private readonly command: string,
    private readonly cwd: string,
    private readonly env: NodeJS.ProcessEnv,
    private readonly log: (message: string) => void
  ) {}

  async start(): Promise<void> {
    this.proc = spawn(this.command, [], {
      cwd: this.cwd,
      env: { ...process.env, ...this.env },
    });

    this.proc.on("error", (err) => this.log(`spawn error: ${err.message}`));
    this.proc.on("exit", (code) => {
      this.ready = false;
      this.log(`mindleak-mcp exited (code ${code ?? "null"})`);
    });

    const rl = readline.createInterface({ input: this.proc.stdout });
    rl.on("line", (line) => this.onLine(line));
    this.proc.stderr.on("data", (chunk) => this.log(`[mindleak-mcp] ${chunk.toString().trim()}`));

    await this.request("initialize", {
      protocolVersion: "2024-11-05",
      capabilities: {},
      clientInfo: { name: "mindleak-vscode", version: "0.1.0" },
    });
    this.notify("notifications/initialized", {});
    this.ready = true;
  }

  isReady(): boolean {
    return this.ready;
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
      this.pending.set(id, { resolve, reject });
      this.proc!.stdin.write(JSON.stringify(payload) + "\n");
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

  dispose(): void {
    this.pending.clear();
    this.proc?.kill();
    this.proc = undefined;
    this.ready = false;
  }
}
