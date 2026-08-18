import { spawn, type ChildProcessWithoutNullStreams } from "node:child_process";
import { McpConnection } from "./protocol.js";
import { parseToolResult } from "./util.js";
import { EvidenceService } from "./services/evidence.js";
import { GraphService } from "./services/graph.js";
import { KnowledgeService } from "./services/knowledge.js";
import { TaskService } from "./services/tasks.js";
import type { ServerIdentity, SessionContext, SessionIdentity, ToolCallResult, ToolDescriptor } from "./types.js";

const PROTOCOL_VERSION = "2024-11-05";

export interface MindLeakClientOptions {
  clientName?: string;
  clientVersion?: string;
  requestTimeoutMs?: number;
  cwd?: string;
  env?: NodeJS.ProcessEnv;
}

/**
 * A typed client for one MindLeak-family MCP server (`mindleak-mcp` or
 * `lodestar-mcp` -- both speak the identical stdio JSON-RPC contract).
 * Wraps the wire protocol exactly as documented (ADR-0103); adds no new
 * tool surface and never bundles or spawns a specific server binary beyond
 * the command it is given.
 */
export class MindLeakClient {
  #connection?: McpConnection;
  #process?: ChildProcessWithoutNullStreams;
  #sessionId?: string;
  #identity?: ServerIdentity;
  #session?: SessionIdentity;

  readonly knowledge = new KnowledgeService(this);
  readonly tasks = new TaskService(this);
  readonly evidence = new EvidenceService(this);
  readonly graph = new GraphService(this);

  constructor(
    private readonly command: string,
    private readonly args: string[] = []
  ) {}

  /** Spawn the server and complete the `initialize` handshake. Does not open a session. */
  async connect(options: MindLeakClientOptions = {}): Promise<ServerIdentity> {
    this.#process = spawn(this.command, this.args, {
      cwd: options.cwd,
      env: options.env ? { ...process.env, ...options.env } : process.env,
    });
    const proc = this.#process;
    this.#connection = new McpConnection(proc.stdin, proc.stdout, {
      requestTimeoutMs: options.requestTimeoutMs,
    });
    proc.on("error", () => this.#connection?.close());
    proc.on("exit", () => this.#connection?.close());

    const result = await this.#connection.request<{ serverInfo?: ServerIdentity }>("initialize", {
      protocolVersion: PROTOCOL_VERSION,
      capabilities: {},
      clientInfo: { name: options.clientName ?? "mindleak-client", version: options.clientVersion ?? "0.1.0" },
    });
    this.#connection.notify("notifications/initialized", {});
    if (!result?.serverInfo) {
      throw new Error("initialize did not return serverInfo");
    }
    this.#identity = result.serverInfo;
    return this.#identity;
  }

  /** Register a session id (ADR-0030) and declare optional working context. */
  async openSession(sessionId: string, context: SessionContext = {}): Promise<SessionIdentity> {
    this.#sessionId = sessionId;
    const session = await this.callTool<SessionIdentity>("open_session", { session_id: sessionId, ...context });
    this.#session = session;
    return session;
  }

  serverIdentity(): ServerIdentity | undefined {
    return this.#identity;
  }

  sessionIdentity(): SessionIdentity | undefined {
    return this.#session;
  }

  async listTools(): Promise<ToolDescriptor[]> {
    const result = await this.#requireConnection().request<{ tools?: ToolDescriptor[] }>("tools/list", {});
    return result?.tools ?? [];
  }

  /** Call any tool by name. Every typed service method is a thin wrapper over this. */
  async callTool<T = unknown>(name: string, args: Record<string, unknown> = {}): Promise<T> {
    const params = this.#sessionId
      ? { name, arguments: { ...args, session_id: this.#sessionId } }
      : { name, arguments: args };
    const result = await this.#requireConnection().request<ToolCallResult>("tools/call", params);
    return parseToolResult<T>(result);
  }

  close(): void {
    this.#connection?.close();
    this.#process?.kill();
    this.#process = undefined;
    this.#connection = undefined;
  }

  #requireConnection(): McpConnection {
    if (!this.#connection) {
      throw new Error("not connected -- call connect() first");
    }
    return this.#connection;
  }
}
