import * as vscode from "vscode";

import { BoardItem, BoardViewProvider } from "./boardViewProvider";
import { McpClient } from "./mcpClient";
import {
  canClaimTask,
  claimTaskRequest,
  overlapWarningDetail,
  parseTaskScope,
  releaseTaskRequest,
  renewTaskRequest,
  TaskScope,
} from "./util";

const LEASE_OPTIONS = [
  { label: "5 minutes", seconds: 300 },
  { label: "15 minutes", seconds: 900 },
  { label: "30 minutes", seconds: 1800 },
  { label: "1 hour", seconds: 3600 },
  { label: "2 hours", seconds: 7200 },
  { label: "8 hours", seconds: 28_800 },
];

export class TaskAllocationController {
  constructor(
    private readonly client: McpClient,
    private readonly memoryClient: McpClient,
    private readonly provider: BoardViewProvider,
    private readonly tree: vscode.TreeView<BoardItem>,
    private readonly configuredAgentId: string,
    private readonly refresh: () => Promise<void>,
    private readonly log: (message: string) => void
  ) {}

  async allocate(item?: BoardItem): Promise<void> {
    if (!this.requireItem(item, "Allocate Task")) {
      return;
    }
    const agent = await vscode.window.showInputBox({
      title: `Allocate: ${item.task.title}`,
      prompt: "Stable agent identity",
      value: item.task.owner?.trim() || this.configuredAgentId,
      ignoreFocusOut: true,
      validateInput: (value) => (value.trim() ? undefined : "An agent identity is required."),
    });
    if (agent === undefined) {
      return;
    }
    await this.claim(item, agent);
  }

  async claimForMe(item?: BoardItem): Promise<void> {
    if (!this.requireItem(item, "Claim for Me")) {
      return;
    }
    await this.claim(item, this.configuredAgentId);
  }

  async renew(item?: BoardItem): Promise<void> {
    if (!this.requireItem(item, "Renew Lease")) {
      return;
    }
    const leaseSeconds = await this.promptLease(`Renew: ${item.task.title}`);
    if (!leaseSeconds) {
      return;
    }
    try {
      const request = renewTaskRequest(item.task, leaseSeconds, nowUnix());
      const result = await this.client.callTool("renew_lease", { ...request });
      if (!result?.renewed) {
        throw new Error("the lease changed or expired before renewal");
      }
      vscode.window.showInformationMessage(`Renewed ${item.task.title} for ${request.agent}.`);
      await this.refresh();
    } catch (error) {
      this.reportError("Lease renewal", error);
    }
  }

  async release(item?: BoardItem): Promise<void> {
    if (!this.requireItem(item, "Release Task")) {
      return;
    }
    let request: ReturnType<typeof releaseTaskRequest>;
    try {
      request = releaseTaskRequest(item.task, nowUnix());
    } catch (error) {
      this.reportError("Task release", error);
      return;
    }
    const confirmed = await vscode.window.showWarningMessage(
      `Release "${item.task.title}" from ${request.agent}?`,
      {
        modal: true,
        detail: "The task returns to the claimable pool. Existing task history is preserved.",
      },
      "Release Task"
    );
    if (confirmed !== "Release Task") {
      return;
    }
    try {
      const result = await this.client.callTool("release_task", { ...request });
      if (!result?.released) {
        throw new Error("the owner or task state changed before release");
      }
      vscode.window.showInformationMessage(`Released ${item.task.title}.`);
      await this.refresh();
    } catch (error) {
      this.reportError("Task release", error);
    }
  }

  async revealNext(): Promise<void> {
    if (!this.client.isReady()) {
      vscode.window.showWarningMessage("Lodestar must be connected to select the next task.");
      return;
    }
    try {
      const next = await this.client.callTool("next_task", {});
      if (!next || typeof next === "string" || typeof next.id !== "string") {
        vscode.window.showInformationMessage("No claimable Lodestar task is available.");
        return;
      }
      await this.refresh();
      const item = this.provider.find(next.id);
      if (!item) {
        throw new Error(`task ${next.id} is not visible on the active board`);
      }
      await this.tree.reveal(item, { select: true, focus: true });
      vscode.window.showInformationMessage(`Next task: ${item.task.title}`);
    } catch (error) {
      this.reportError("Next task selection", error);
    }
  }

  private async claim(item: BoardItem, agent: string): Promise<void> {
    const scope = await this.promptScope(item.task.title);
    if (!scope) {
      return;
    }
    const leaseSeconds = await this.promptLease(`Lease: ${item.task.title}`);
    if (!leaseSeconds) {
      return;
    }
    try {
      if (!(await this.confirmPreflight(item, agent, scope))) {
        return;
      }
      const request = claimTaskRequest(item.task, agent, leaseSeconds, nowUnix(), scope);
      const result = await this.client.callTool("claim_task", { ...request });
      if (!result?.won) {
        vscode.window.showWarningMessage(
          `Allocation lost: ${item.task.title} was claimed or changed by another agent.`
        );
        await this.refresh();
        return;
      }
      vscode.window.showInformationMessage(`Allocated ${item.task.title} to ${request.agent}.`);
      await this.refresh();
    } catch (error) {
      this.reportError("Task allocation", error);
    }
  }

  private async confirmPreflight(
    item: BoardItem,
    agent: string,
    scope: TaskScope
  ): Promise<boolean> {
    if (scope.paths.length === 0 && scope.symbols.length === 0) {
      return true;
    }
    try {
      if (!this.memoryClient.isReady()) {
        throw new Error("MindLeak memory plane is unavailable");
      }
      const [claimResult, footprintResult] = await Promise.all([
        this.client.callTool("check_overlap", {
          ...scope,
          exclude_task_id: item.task.id,
        }),
        this.memoryClient.callTool("check_overlap", {
          ...scope,
          exclude_agent: agent,
        }),
      ]);
      const detail = overlapWarningDetail({
        claims: Array.isArray(claimResult?.claims) ? claimResult.claims : [],
        footprints: Array.isArray(footprintResult?.footprints) ? footprintResult.footprints : [],
      });
      if (!detail) {
        return true;
      }
      const choice = await vscode.window.showWarningMessage(
        `Overlap detected before claiming "${item.task.title}".`,
        {
          modal: true,
          detail: `${detail}\n\nCoordinate, choose different work, or claim anyway.`,
        },
        "Claim Anyway"
      );
      return choice === "Claim Anyway";
    } catch (error) {
      const choice = await vscode.window.showWarningMessage(
        `Overlap pre-flight failed for "${item.task.title}".`,
        {
          modal: true,
          detail: `${(error as Error).message}\n\nNo lock was acquired; continue only if you accept the missing overlap evidence.`,
        },
        "Claim Without Check"
      );
      return choice === "Claim Without Check";
    }
  }

  private async promptScope(title: string): Promise<TaskScope | undefined> {
    const paths = await vscode.window.showInputBox({
      title: `Scope: ${title}`,
      prompt: "Concrete workspace-relative paths (comma- or newline-separated, optional)",
      placeHolder: "src/auth.ts, src/session.ts",
      ignoreFocusOut: true,
    });
    if (paths === undefined) {
      return undefined;
    }
    const symbols = await vscode.window.showInputBox({
      title: `Scope: ${title}`,
      prompt: "MindLeak symbol ids (comma- or newline-separated, optional)",
      placeHolder: "symbol:src/auth.ts:validateSession",
      ignoreFocusOut: true,
    });
    return symbols === undefined ? undefined : parseTaskScope(paths, symbols);
  }

  private async promptLease(title: string): Promise<number | undefined> {
    const selected = await vscode.window.showQuickPick(LEASE_OPTIONS, {
      title,
      placeHolder: "Choose a bounded lease duration",
      ignoreFocusOut: true,
    });
    return selected?.seconds;
  }

  private requireItem(item: BoardItem | undefined, action: string): item is BoardItem {
    if (!this.client.isReady()) {
      vscode.window.showWarningMessage("Lodestar must be connected to allocate tasks.");
      return false;
    }
    if (!item) {
      vscode.window.showWarningMessage(`Run ${action} from an Intent Board row.`);
      return false;
    }
    if (
      (action === "Allocate Task" || action === "Claim for Me") &&
      !canClaimTask(item.task, nowUnix())
    ) {
      vscode.window.showWarningMessage(`Task ${item.task.title} is not currently claimable.`);
      return false;
    }
    return true;
  }

  private reportError(action: string, error: unknown): void {
    const message = `${action} failed: ${(error as Error).message}`;
    this.log(message);
    vscode.window.showErrorMessage(`MindLeak ${message}`);
  }
}

function nowUnix(): number {
  return Math.floor(Date.now() / 1000);
}
