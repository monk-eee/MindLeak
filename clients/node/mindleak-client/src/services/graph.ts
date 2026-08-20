import type { MindLeakClient } from "../client.js";

/** MindLeak's temporal context graph: recall, impact, overlap, working set. */
export class GraphService {
  constructor(private readonly client: MindLeakClient) {}

  recall(args: { query: string; limit?: number }): Promise<unknown> {
    return this.client.callTool("recall", args);
  }

  multiHop(args: { seed: string; depth?: number }): Promise<unknown> {
    return this.client.callTool("graph_multi_hop_query", args);
  }

  impactRadius(args: { node_id: string }): Promise<unknown> {
    return this.client.callTool("get_impact_radius", args);
  }

  checkOverlap(args: { paths?: string[]; symbols?: string[] }): Promise<unknown> {
    return this.client.callTool("check_overlap", args);
  }

  workingSet(): Promise<unknown> {
    return this.client.callTool("working_set", {});
  }

  stats(): Promise<unknown> {
    return this.client.callTool("graph_stats", {});
  }
}
