// Newline-delimited JSON-RPC 2.0 framing over an arbitrary duplex pair.
// Contains no process-spawning logic, so it is unit-testable with plain
// in-memory streams -- see client.ts's spawnMindLeakServer for the
// process-backed factory this is used from.
import { createInterface } from "node:readline";
import type { Readable, Writable } from "node:stream";

export interface JsonRpcError {
  code: number;
  message: string;
  data?: unknown;
}

interface Pending {
  resolve: (value: unknown) => void;
  reject: (err: Error) => void;
  timer: ReturnType<typeof setTimeout>;
}

export interface McpConnectionOptions {
  requestTimeoutMs?: number;
}

export class McpConnection {
  #nextId = 1;
  #pending = new Map<number, Pending>();
  #closed = false;
  #closeListeners = new Set<(err?: Error) => void>();
  readonly #requestTimeoutMs: number;

  constructor(
    private readonly stdin: Writable,
    stdout: Readable,
    options: McpConnectionOptions = {}
  ) {
    this.#requestTimeoutMs = options.requestTimeoutMs ?? 30_000;
    const rl = createInterface({ input: stdout });
    rl.on("line", (line) => this.#onLine(line));
    stdout.on("close", () => this.#onClose());
    stdout.on("error", (err) => this.#onClose(err));
  }

  /** Notified once, when the connection closes for any reason (EOF, error, explicit close()). */
  onClose(listener: (err?: Error) => void): void {
    this.#closeListeners.add(listener);
  }

  request<T = unknown>(method: string, params: unknown): Promise<T> {
    if (this.#closed) {
      return Promise.reject(new Error("MCP connection is closed"));
    }
    const id = this.#nextId++;
    const payload = { jsonrpc: "2.0", id, method, params };
    return new Promise<T>((resolve, reject) => {
      const timer = setTimeout(() => {
        if (this.#pending.delete(id)) {
          reject(new Error(`MCP request "${method}" timed out after ${this.#requestTimeoutMs}ms`));
        }
      }, this.#requestTimeoutMs);
      this.#pending.set(id, { resolve: resolve as (value: unknown) => void, reject, timer });
      this.stdin.write(JSON.stringify(payload) + "\n", (err) => {
        if (err) {
          clearTimeout(timer);
          this.#pending.delete(id);
          reject(err);
        }
      });
    });
  }

  notify(method: string, params: unknown): void {
    if (this.#closed) return;
    this.stdin.write(JSON.stringify({ jsonrpc: "2.0", method, params }) + "\n");
  }

  close(): void {
    this.#onClose();
  }

  #onLine(line: string): void {
    const trimmed = line.trim();
    if (!trimmed) return;
    let msg: { id?: number; result?: unknown; error?: JsonRpcError };
    try {
      msg = JSON.parse(trimmed);
    } catch {
      return; // Not a JSON-RPC message on this line -- ignore rather than crash the connection.
    }
    if (typeof msg.id !== "number") return;
    const pending = this.#pending.get(msg.id);
    if (!pending) return;
    this.#pending.delete(msg.id);
    clearTimeout(pending.timer);
    if (msg.error) {
      pending.reject(new Error(msg.error.message ?? "MCP error"));
    } else {
      pending.resolve(msg.result);
    }
  }

  #onClose(err?: Error): void {
    if (this.#closed) return;
    this.#closed = true;
    const closeErr = err ?? new Error("MCP connection closed");
    for (const pending of this.#pending.values()) {
      clearTimeout(pending.timer);
      pending.reject(closeErr);
    }
    this.#pending.clear();
    for (const listener of this.#closeListeners) listener(err);
  }
}
