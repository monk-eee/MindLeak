import type { ToolCallResult } from "./types.js";

/**
 * Parse an MCP `tools/call` result the same way every MindLeak consumer
 * must: prefer machine-readable `structuredContent`, fall back to
 * JSON-parsing the first text content block, and fall back further to the
 * raw text when it is not JSON. Throws when the tool reported an error.
 *
 * A pure function so it is unit-testable without a live connection.
 */
export function parseToolResult<T = unknown>(result: ToolCallResult): T {
  if (result?.isError) {
    throw new Error(result?.content?.[0]?.text ?? "tool error");
  }
  if (result?.structuredContent !== undefined) {
    return result.structuredContent as T;
  }
  const text = result?.content?.[0]?.text;
  if (typeof text !== "string") {
    return result as unknown as T;
  }
  try {
    return JSON.parse(text) as T;
  } catch {
    return text as unknown as T;
  }
}
