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

export interface McpSessionIdentity {
  agent_id: string;
}

/**
 * Working context an `open_session` call may declare (ADR-0035): branch,
 * head commit, dirty tree, and — only when a tracked upstream makes them
 * knowable rather than guessed — base and how far behind it. Any field left
 * out stays `unknown` to the server rather than being invented here.
 */
export interface SessionContext {
  branch?: string;
  head_sha?: string;
  base?: string;
  dirty?: boolean;
  behind?: number;
}

/**
 * Supplies the session context to declare, called fresh at every declaration
 * (initial `open_session` and each {@link McpClient.refreshContext} call) so
 * it always reports the current state rather than a snapshot from
 * construction time. Returns `undefined` when no context is available (no
 * workspace Git repository, extension disabled, etc.) — the client then
 * declares none, which the server already treats as `unknown`.
 */
export type SessionContextProvider = () =>
  SessionContext | undefined | Promise<SessionContext | undefined>;

/** Whether the server is answering, coming back, or gone until a reload. */
export type McpConnectionState = "connected" | "reconnecting" | "disconnected";

/**
 * Consecutive automatic relaunches allowed after an unexpected server exit.
 * Not reset on a successful relaunch, so a crash loop stops instead of
 * respawning forever; a window reload starts a fresh budget.
 */
const MAX_RESTARTS = 3;

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
  private sessionIdentity?: McpSessionIdentity;
  private disposing = false;
  private restarts = 0;
  private stateListener?: (state: McpConnectionState, detail: string) => void;

  constructor(
    private readonly command: string,
    private readonly cwd: string,
    private readonly env: NodeJS.ProcessEnv,
    private readonly sessionId: string,
    private readonly log: (message: string) => void,
    private readonly requestTimeoutMs = 30_000,
    private readonly contextProvider: SessionContextProvider = () => undefined
  ) {}

  /**
   * Observe connection state. Register before {@link start} so a caller's
   * health surface follows the server rather than reporting whatever was true
   * at activation.
   */
  onStateChange(listener: (state: McpConnectionState, detail: string) => void): void {
    this.stateListener = listener;
  }

  async start(): Promise<void> {
    this.disposing = false;
    this.restarts = 0;
    await this.launch();
  }

  private async launch(): Promise<void> {
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
      this.sessionIdentity = undefined;
      this.rejectPending(new Error(`MCP server exited (code ${code ?? "null"})`));
      if (this.disposing) {
        // The output channel is already gone during extension teardown.
        return;
      }
      this.log(`${this.command} exited (code ${code ?? "null"})`);
      this.restart();
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
    const sessionResult = await this.request("tools/call", {
      name: "open_session",
      arguments: { session_id: this.sessionId, ...(await this.declaredContext()) },
    });
    const session = parseToolResult(sessionResult) as McpSessionIdentity;
    if (typeof session?.agent_id !== "string" || !session.agent_id) {
      throw new Error("MCP server did not return a session agent identity");
    }
    this.sessionIdentity = session;
    this.ready = true;
    this.stateListener?.("connected", this.command);
  }

  /**
   * Relaunch the server after it exited on its own (a crash, or an external
   * `taskkill` while the binary is rebuilt). Without this the panes stay dead
   * until the window is reloaded.
   */
  private restart(): void {
    if (this.restarts >= MAX_RESTARTS) {
      const detail = `${this.command} stayed down after ${MAX_RESTARTS} restarts; reload the window`;
      this.log(detail);
      this.stateListener?.("disconnected", detail);
      return;
    }
    this.restarts += 1;
    this.log(`restarting ${this.command} (attempt ${this.restarts}/${MAX_RESTARTS})`);
    this.stateListener?.("reconnecting", `restarting ${this.restarts}/${MAX_RESTARTS}`);
    void this.launch().then(
      () => {
        if (this.disposing) {
          void this.dispose(0, 0);
          return;
        }
        this.log(`reconnected to ${this.command}`);
      },
      (err) => {
        const detail = `restart failed: ${(err as Error).message}`;
        this.log(detail);
        this.stateListener?.("disconnected", detail);
      }
    );
  }

  isReady(): boolean {
    return this.ready;
  }

  serverIdentity(): McpServerIdentity | undefined {
    return this.identity ? { ...this.identity } : undefined;
  }

  agentIdentity(): string | undefined {
    return this.sessionIdentity?.agent_id;
  }

  /**
   * Re-declare this session's Git context after it changes (a branch switch,
   * a new commit, the working tree turning dirty or clean). `open_session`
   * upserts by session id (ADR-0035), so calling it again refreshes the
   * fleet's declared context in place rather than minting a new session. A
   * no-op before the session is ready, or when the context provider currently
   * has nothing to declare (no workspace repository, Git extension disabled).
   */
  async refreshContext(): Promise<void> {
    if (!this.ready) {
      return;
    }
    const context = await this.declaredContext();
    if (Object.keys(context).length === 0) {
      return;
    }
    try {
      await this.callTool("open_session", context);
    } catch (err) {
      this.log(`context refresh error: ${(err as Error).message}`);
    }
  }

  /**
   * Build the `open_session` arguments for whatever context is currently
   * available, dropping any field the provider left undeclared rather than
   * guessing a value for it (ADR-0044).
   */
  private async declaredContext(): Promise<Record<string, unknown>> {
    let context: SessionContext | undefined;
    try {
      context = await this.contextProvider();
    } catch (err) {
      this.log(`session context unavailable: ${(err as Error).message}`);
      return {};
    }
    if (!context) {
      return {};
    }
    const args: Record<string, unknown> = {};
    if (context.branch !== undefined) {
      args.branch = context.branch;
    }
    if (context.head_sha !== undefined) {
      args.head_sha = context.head_sha;
    }
    if (context.base !== undefined) {
      args.base = context.base;
    }
    if (context.dirty !== undefined) {
      args.dirty = context.dirty;
    }
    if (context.behind !== undefined) {
      args.behind = context.behind;
    }
    return args;
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
    const result = await this.request("tools/call", {
      name,
      arguments: { ...args, session_id: this.sessionId },
    });
    if (result?.isError) {
      const text = result?.content?.[0]?.text ?? "tool error";
      throw new Error(text);
    }
    return parseToolResult(result);
  }

  async dispose(graceMilliseconds = 2000, forceMilliseconds = 1000): Promise<void> {
    const proc = this.proc;
    this.disposing = true;
    this.proc = undefined;
    this.ready = false;
    this.identity = undefined;
    this.sessionIdentity = undefined;
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
