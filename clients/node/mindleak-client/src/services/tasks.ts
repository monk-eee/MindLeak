import type { MindLeakClient } from "../client.js";

export interface LodestarTask {
  id: string;
  title: string;
  status: string;
  [key: string]: unknown;
}

/** Lodestar's task lifecycle: create, claim, transition, query (ADR-0015/0020/0048). */
export class TaskService {
  constructor(private readonly client: MindLeakClient) {}

  create(args: {
    goal_id: string;
    title?: string;
    acceptance?: string;
    blocked_by?: string;
    also_serves?: string[];
  }): Promise<LodestarTask> {
    return this.client.callTool("task_create", args);
  }

  claim(args: { task_id: string; lease_secs?: number; paths?: string[]; symbols?: string[] }): Promise<unknown> {
    return this.client.callTool("task_claim", { ...args, step: "claim" });
  }

  renew(args: { task_id: string; lease_secs?: number }): Promise<unknown> {
    return this.client.callTool("task_claim", { ...args, step: "renew" });
  }

  release(args: { task_id: string }): Promise<unknown> {
    return this.client.callTool("task_claim", { ...args, step: "release" });
  }

  complete(args: { task_id: string; evidence: unknown; check: unknown; learned?: string }): Promise<unknown> {
    return this.client.callTool("task_transition", { ...args, to: "complete" });
  }

  block(args: { task_id: string; reason?: string; blocked_by?: string }): Promise<unknown> {
    return this.client.callTool("task_transition", { ...args, to: "block" });
  }

  /**
   * The task board. A current server bounds this to the 200 most recently
   * touched tasks by default and answers `{count, tasks, tasks_truncated}`;
   * pass `limit: 0` for the full, unbounded history. Either response shape
   * unwraps to a plain task array here, so this method's own return type
   * never changes underneath callers.
   */
  async board(
    args: { include_terminal?: boolean; detail?: boolean; limit?: number } = {}
  ): Promise<LodestarTask[]> {
    const result = await this.client.callTool("task_query", { ...args, view: "board" });
    if (Array.isArray(result)) {
      return result as LodestarTask[];
    }
    const tasks = (result as { tasks?: unknown } | null | undefined)?.tasks;
    return Array.isArray(tasks) ? (tasks as LodestarTask[]) : [];
  }

  overlap(args: { paths?: string[]; symbols?: string[] }): Promise<unknown> {
    return this.client.callTool("task_query", { ...args, view: "overlap" });
  }

  next(): Promise<LodestarTask | null> {
    return this.client.callTool("task_query", { view: "next" });
  }
}
