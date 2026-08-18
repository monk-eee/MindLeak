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

  board(args: { include_terminal?: boolean } = {}): Promise<LodestarTask[]> {
    return this.client.callTool("task_query", { ...args, view: "board" });
  }

  overlap(args: { paths?: string[]; symbols?: string[] }): Promise<unknown> {
    return this.client.callTool("task_query", { ...args, view: "overlap" });
  }

  next(): Promise<LodestarTask | null> {
    return this.client.callTool("task_query", { view: "next" });
  }
}
