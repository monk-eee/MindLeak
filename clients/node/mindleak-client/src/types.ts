export interface ServerIdentity {
  name: string;
  version: string;
}

export interface SessionContext {
  branch?: string;
  head_sha?: string;
  base?: string;
  dirty?: boolean;
  behind?: number;
}

export interface SessionIdentity {
  agent_id: string;
  context?: SessionContext;
  stale_build?: string;
  waiting_on_you?: unknown;
  paused_by_you?: unknown;
  awaiting_a_human?: unknown;
  rescue_work?: unknown;
}

export interface ToolDescriptor {
  name: string;
  description?: string;
  inputSchema?: unknown;
}

export interface ToolContentBlock {
  type: string;
  text?: string;
}

export interface ToolCallResult {
  isError?: boolean;
  content?: ToolContentBlock[];
  structuredContent?: unknown;
}
