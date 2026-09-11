const outcomes = new Set([
  "pending_confirmation", "pending_delivery", "accepted", "applied",
  "failed", "expired", "conflicted", "refused",
]);

export class WorkCommandClient {
  #url;
  #requestBody;
  #payloadBody;
  #fetch;
  #busy = false;
  #response;

  constructor({ repositoryId, command, payload, fetchImpl = globalThis.fetch }) {
    if (typeof repositoryId !== "string" || !repositoryId.trim()) {
      throw new Error("A repository is required.");
    }
    this.#url = `/api/v1/repositories/${encodeURIComponent(repositoryId)}/work/commands`;
    this.#requestBody = JSON.stringify({ ...command, ...payload });
    this.#payloadBody = JSON.stringify(payload);
    this.#fetch = fetchImpl;
  }

  async preview() {
    if (this.#busy) throw new Error("A command request is already in progress.");
    if (this.#response) return structuredClone(this.#response);
    this.#response = await this.#post(this.#url, this.#requestBody);
    return structuredClone(this.#response);
  }

  async confirm() {
    if (this.#response?.status !== "pending_confirmation") {
      throw new Error("This command is not awaiting confirmation.");
    }
    const commandId = this.#response.command_id;
    const response = await this.#post(
      `${this.#url}/${encodeURIComponent(commandId)}/confirm`,
      this.#payloadBody,
    );
    if (response.command_id && response.command_id !== commandId) {
      throw new Error("Command response does not match the confirmed command.");
    }
    this.#response = response;
    return structuredClone(response);
  }

  async #post(url, body) {
    if (this.#busy) throw new Error("A command request is already in progress.");
    this.#busy = true;
    try {
      const response = await this.#fetch(url, {
        method: "POST",
        credentials: "same-origin",
        redirect: "error",
        headers: { "Content-Type": "application/json", Accept: "application/json" },
        body,
      });
      if (!response.ok) {
        throw new Error(`Command request failed (HTTP ${response.status}).`);
      }
      const result = await response.json();
      if (!result || typeof result !== "object" || Array.isArray(result)) {
        throw new Error("Invalid command response.");
      }
      switch (result.status) {
        case "pending_confirmation":
        case "executed":
          if (
            typeof result.command_id !== "string" || !result.command_id.trim() ||
            typeof result.receipt_id !== "string" || !result.receipt_id.trim() ||
            typeof result.idempotent_replay !== "boolean" ||
            !outcomes.has(result.outcome) ||
            (result.status === "pending_confirmation" && result.outcome !== "pending_confirmation") ||
            (result.status === "executed" && typeof result.reason !== "string")
          ) {
            throw new Error("Invalid command response receipt.");
          }
          break;
        case "refused":
        case "authorization_unavailable":
          if (typeof result.reason !== "string") {
            throw new Error("Invalid command response refusal.");
          }
          break;
        case "command_not_found":
          break;
        default:
          throw new Error("Unknown command response status.");
      }
      return result;
    } finally {
      this.#busy = false;
    }
  }
}
