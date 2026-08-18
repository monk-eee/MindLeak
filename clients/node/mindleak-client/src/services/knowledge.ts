import type { MindLeakClient } from "../client.js";

export interface KnowledgeRecord {
  id: string;
  [key: string]: unknown;
}

/** MindLeak's learned-knowledge loop (ADR-0022): record, search, retire. */
export class KnowledgeService {
  constructor(private readonly client: MindLeakClient) {}

  record(args: { content: string; node_ids?: string[]; goal_id?: string; source_ref?: string }): Promise<KnowledgeRecord> {
    return this.client.callTool("record_knowledge", args);
  }

  active(args: { query?: string; source_ref?: string } = {}): Promise<KnowledgeRecord[]> {
    return this.client.callTool("active_knowledge", args);
  }

  retire(args: { id?: string; source_ref?: string; reason: string }): Promise<unknown> {
    return this.client.callTool("retire_knowledge", args);
  }

  reconfirm(args: { id: string }): Promise<unknown> {
    return this.client.callTool("reconfirm_knowledge", args);
  }
}
