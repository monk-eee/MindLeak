import type { MindLeakClient } from "../client.js";

/** Bounded, provenance-bearing evidence and conformance (ADR-0009/0025). */
export class EvidenceService {
  constructor(private readonly client: MindLeakClient) {}

  evidenceFor(args: { started_at: number; ended_at: number; task_id?: string }): Promise<unknown> {
    return this.client.callTool("evidence_for", args);
  }

  checkConformance(args: { evidence: unknown; task_id?: string }): Promise<unknown> {
    return this.client.callTool("check_conformance", args);
  }

  history(args: { task_id: string }): Promise<unknown> {
    return this.client.callTool("conformance_history", args);
  }
}
