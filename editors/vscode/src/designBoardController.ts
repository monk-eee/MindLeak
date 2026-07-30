import * as path from "path";
import * as vscode from "vscode";

import {
  chooseAdrTarget,
  DesignGoal,
  DesignItem,
  DesignMaterializationPlan,
  DesignMaterializationRecord,
  DesignMetadata,
  DesignPromotion,
  DesignTask,
  formatDesignPromotion,
  formatMaterializationPlan,
  parseAdrMetadata,
  rawAdrStatus,
  replaceAdrStatus,
} from "./designBoard";
import { DesignBoardItem, DesignBoardViewProvider } from "./designBoardViewProvider";
import { McpClient } from "./mcpClient";

interface ConstitutionGoal extends DesignGoal {
  status: string;
}

interface PlanSelection {
  plan: DesignMaterializationPlan;
  linkedTasks: DesignTask[];
}

export class DesignBoardController {
  constructor(
    private readonly client: McpClient,
    private readonly provider: DesignBoardViewProvider,
    private readonly agentId: string,
    private readonly log: (message: string) => void,
    private readonly refreshIntentBoard: () => Promise<void>
  ) {}

  async sync(): Promise<void> {
    if (!this.client.isReady()) {
      return;
    }
    try {
      const { metadata, skipped } = await readWorkspaceAdrMetadata(this.agentId);
      await this.client.callTool("design_register", { designs: metadata });
      await this.refresh();
      this.log(`Design Board synchronized ${metadata.length} repository ADRs.`);
      // An ADR the parser cannot read is an ADR the board can never show. That
      // has to be noisy: silence is what let two accepted decisions sit
      // unregistered while the sync kept reporting success.
      for (const { adrPath, reason } of skipped) {
        this.log(`Design Board skipped ${adrPath}: ${reason}`);
      }
      if (skipped.length) {
        vscode.window.showWarningMessage(
          `MindLeak registered ${metadata.length} ADRs and skipped ${skipped.length}. See the MindLeak output channel.`
        );
      }
    } catch (error) {
      this.reportError("ADR synchronization", error);
    }
  }

  async refresh(): Promise<void> {
    if (!this.client.isReady()) {
      return;
    }
    try {
      const designs = (await this.client.callTool("design_query", {
        view: "ledger",
      })) as DesignItem[];
      const materialized = designs.filter((design) => design.promotion_status === "materialized");
      // One unreadable promotion must not blank the whole board. `Promise.all`
      // rejected the entire batch, so a single bad row left the view showing
      // stale contents with only an error toast to explain it.
      const settled = await Promise.allSettled(
        materialized.map(async (design) => {
          const promotion = (await this.client.callTool("design_query", {
            id: design.id,
            view: "promotion",
          })) as DesignPromotion | null;
          return [design.id, promotion] as const;
        })
      );
      const promotions = new Map<string, DesignPromotion>();
      settled.forEach((outcome, index) => {
        if (outcome.status === "rejected") {
          this.log(
            `Design Board could not read materialization for ${materialized[index].id}: ${describeError(outcome.reason)}`
          );
          return;
        }
        const [id, promotion] = outcome.value;
        if (promotion) {
          promotions.set(id, promotion);
        }
      });
      this.provider.update(designs, promotions);
    } catch (error) {
      this.reportError("Design Board refresh", error);
    }
  }

  async accept(item?: DesignBoardItem): Promise<void> {
    if (!this.requireItem(item, "Accept Design")) {
      return;
    }
    const human = await this.promptHuman("Accept Design", item.design);
    if (!human) {
      return;
    }
    try {
      await this.client.callTool("design_decide", {
        id: item.design.id,
        decision: "accept",
        human,
      });
      await this.alignAdrStatus(item.design, "accepted");
      vscode.window.showInformationMessage(`Accepted design: ${item.design.title}`);
      await this.refresh();
    } catch (error) {
      this.reportError("Design acceptance", error);
    }
  }

  async reject(item?: DesignBoardItem): Promise<void> {
    if (!this.requireItem(item, "Reject Design")) {
      return;
    }
    const human = await this.promptHuman("Reject Design", item.design);
    if (!human) {
      return;
    }
    const reason = await vscode.window.showInputBox({
      title: `Reject: ${item.design.title}`,
      prompt: "Rejection rationale",
      ignoreFocusOut: true,
      validateInput: (value) => (value.trim() ? undefined : "A rationale is required."),
    });
    if (reason === undefined) {
      return;
    }
    try {
      await this.client.callTool("design_decide", {
        id: item.design.id,
        decision: "reject",
        human,
        reason: reason.trim(),
      });
      await this.alignAdrStatus(item.design, "rejected");
      vscode.window.showInformationMessage(`Rejected design: ${item.design.title}`);
      await this.refresh();
    } catch (error) {
      this.reportError("Design rejection", error);
    }
  }

  async promote(item?: DesignBoardItem): Promise<void> {
    if (!this.requireItem(item, "Promote Design")) {
      return;
    }
    try {
      const selection = await this.choosePlan(item.design, false);
      if (!selection || !(await this.confirmPlan(item.design, selection, false))) {
        this.reportCancelled("Materialization", item.design);
        return;
      }
      const promotion = (await this.client.callTool("design_promote", {
        id: item.design.id,
        step: "materialize",
        plan: selection.plan,
      })) as DesignPromotion;
      vscode.window.showInformationMessage(
        `Materialized ${promotion.tasks.length} task(s) for ${item.design.title}.`
      );
      await Promise.all([this.refresh(), this.refreshIntentBoard()]);
    } catch (error) {
      this.reportError("Design promotion", error);
      await this.refresh();
    }
  }

  async revisePromotion(item?: DesignBoardItem): Promise<void> {
    if (!this.requireItem(item, "Repair Design Materialization")) {
      return;
    }
    const human = await this.promptHuman("Repair Materialization", item.design);
    if (!human) {
      this.reportCancelled("Materialization repair", item.design);
      return;
    }
    try {
      const selection = await this.choosePlan(item.design, true);
      if (!selection || !(await this.confirmPlan(item.design, selection, true))) {
        this.reportCancelled("Materialization repair", item.design);
        return;
      }
      const promotion = (await this.client.callTool("design_promote", {
        id: item.design.id,
        step: "revise",
        human,
        plan: selection.plan,
      })) as DesignPromotion;
      vscode.window.showInformationMessage(
        `Revised ${item.design.title} to materialization r${promotion.revision}.`
      );
      await Promise.all([this.refresh(), this.refreshIntentBoard()]);
    } catch (error) {
      this.reportError("Design materialization repair", error);
      await this.refresh();
    }
  }

  async openAdr(item?: DesignBoardItem): Promise<void> {
    if (!this.requireItem(item, "Open ADR")) {
      return;
    }
    try {
      const uri = await this.resolveAdrUri(item.design);
      const document = await vscode.workspace.openTextDocument(uri);
      await vscode.window.showTextDocument(document, { preview: true });
    } catch (error) {
      this.reportError("Open ADR", error);
    }
  }

  async inspectPromotion(item?: DesignBoardItem): Promise<void> {
    if (!this.requireItem(item, "Inspect Materialization")) {
      return;
    }
    try {
      const promotion =
        item.promotion ??
        ((await this.client.callTool("design_query", {
          id: item.design.id,
          view: "promotion",
        })) as DesignPromotion | null);
      if (!promotion) {
        vscode.window.showInformationMessage(
          `No materialized implementation exists for ${item.design.title}.`
        );
        return;
      }
      const history = (await this.client.callTool("design_query", {
        id: item.design.id,
        view: "history",
      })) as DesignMaterializationRecord[];
      const document = await vscode.workspace.openTextDocument({
        content: formatDesignPromotion(promotion, history),
        language: "markdown",
      });
      await vscode.window.showTextDocument(document, { preview: true });
    } catch (error) {
      this.reportError("Materialization inspection", error);
    }
  }

  private async choosePlan(
    design: DesignItem,
    repair: boolean
  ): Promise<PlanSelection | undefined> {
    const choice = await vscode.window.showQuickPick(
      [
        {
          label: "$(add) Create new tasks",
          description: "Review suggested task drafts before creation",
          mode: "create" as const,
        },
        {
          label: "$(link) Link existing tasks",
          description: "Reuse authoritative scheduled work",
          mode: "link" as const,
        },
        {
          label: "$(check) No new work",
          description: "Record that implementation is already complete or unnecessary",
          mode: "no_work" as const,
        },
      ],
      {
        title: `${repair ? "Repair" : "Materialize"}: ${design.title}`,
        placeHolder: "Choose how this accepted design maps to executive work",
        ignoreFocusOut: true,
      }
    );
    if (!choice) {
      return undefined;
    }

    if (choice.mode === "create") {
      const objectives = await this.pickObjectives(design);
      if (!objectives.length) {
        return undefined;
      }
      const suggestions = await Promise.all(
        objectives.map(
          async (objective) =>
            (await this.client.callTool("design_promote", {
              id: design.id,
              step: "plan",
              objective_goal_id: objective.id,
            })) as DesignMaterializationPlan
        )
      );
      const plan: DesignMaterializationPlan = {
        mode: "create",
        tasks: suggestions.flatMap((suggestion) => suggestion.tasks ?? []),
        constraints: suggestions.flatMap((suggestion) => suggestion.constraints ?? []),
      };
      if (repair) {
        const rationale = await this.promptRationale(design, "Why replace the current plan?");
        if (!rationale) {
          return undefined;
        }
        plan.rationale = rationale;
      }
      return { plan, linkedTasks: [] };
    }

    const rationale = await this.promptRationale(
      design,
      choice.mode === "link"
        ? "Why does the selected work implement this design?"
        : "Why is no new implementation work required?"
    );
    if (!rationale) {
      return undefined;
    }
    if (choice.mode === "no_work") {
      return { plan: { mode: "no_work", rationale }, linkedTasks: [] };
    }

    const tasks = (
      (await this.client.callTool("task_query", {
        view: "board",
        include_terminal: true,
      })) as DesignTask[]
    ).filter((task) => task.status !== "abandoned");
    const selected = await vscode.window.showQuickPick(
      tasks.map((task) => ({
        label: task.title,
        description: `${task.status} · ${task.id}`,
        detail: task.goal_id,
        task,
      })),
      {
        title: `Link existing work: ${design.title}`,
        placeHolder: "Select one or more existing tasks",
        canPickMany: true,
        ignoreFocusOut: true,
      }
    );
    if (!selected?.length) {
      return undefined;
    }
    return {
      plan: { mode: "link", task_ids: selected.map((entry) => entry.task.id), rationale },
      linkedTasks: selected.map((entry) => entry.task),
    };
  }

  private async pickObjectives(design: DesignItem): Promise<ConstitutionGoal[]> {
    const goals = (await this.client.callTool("get_constitution", {})) as ConstitutionGoal[];
    const objectives = goals.filter(
      (goal) => goal.kind === "objective" && goal.status === "active"
    );
    if (!objectives.length) {
      vscode.window.showWarningMessage(
        "No active objective goal can hold new tasks. Define an objective first, or choose Link existing tasks or No new work."
      );
      return [];
    }
    const selected = await vscode.window.showQuickPick(
      objectives.map((goal) => ({ label: goal.title, description: goal.id, goal })),
      {
        title: `Create work: ${design.title}`,
        placeHolder: "Select one or more objectives for separate task drafts",
        canPickMany: true,
        ignoreFocusOut: true,
      }
    );
    return selected?.map((entry) => entry.goal) ?? [];
  }

  private async promptRationale(design: DesignItem, prompt: string): Promise<string | undefined> {
    const rationale = await vscode.window.showInputBox({
      title: `Materialize: ${design.title}`,
      prompt,
      ignoreFocusOut: true,
      validateInput: (value) => (value.trim() ? undefined : "A rationale is required."),
    });
    return rationale?.trim();
  }

  private async confirmPlan(
    design: DesignItem,
    selection: PlanSelection,
    repair: boolean
  ): Promise<boolean> {
    const action = repair ? "Repair" : "Materialize";
    const retention = repair
      ? "\n\nPrior tasks remain durable and must be retired separately if they are obsolete."
      : "";
    const confirmed = await vscode.window.showWarningMessage(
      `${action} ${design.title}?\n\n${formatMaterializationPlan(selection.plan, selection.linkedTasks)}${retention}`,
      { modal: true },
      action
    );
    return confirmed === action;
  }

  private async alignAdrStatus(design: DesignItem, status: "accepted" | "rejected"): Promise<void> {
    const uri = await this.resolveAdrUri(design);
    const content = Buffer.from(await vscode.workspace.fs.readFile(uri)).toString("utf8");
    const updated = replaceAdrStatus(content, status);
    if (!updated) {
      throw new Error(`${design.adr_path} has no structured Status field`);
    }
    await vscode.workspace.fs.writeFile(uri, Buffer.from(updated, "utf8"));
  }

  private async resolveAdrUri(design: DesignItem): Promise<vscode.Uri> {
    const candidates: vscode.Uri[] = [];
    for (const folder of vscode.workspace.workspaceFolders ?? []) {
      const uri = vscode.Uri.joinPath(folder.uri, ...design.adr_path.split("/"));
      try {
        await vscode.workspace.fs.stat(uri);
        candidates.push(uri);
      } catch (error) {
        if ((error as vscode.FileSystemError).code !== "FileNotFound") {
          throw error;
        }
      }
    }

    const choice = chooseAdrTarget(candidates.map((uri) => uri.fsPath));
    switch (choice.kind) {
      case "none":
        throw new Error(`cannot find ${design.adr_path} in the open workspace`);
      case "one":
        return candidates[0];
      case "ambiguous": {
        // Several checkouts of one repository are open, so which of them should
        // carry this decision is a question only the reviewer can answer. They
        // are already in the accept/reject prompt; one more is cheaper than
        // recording a decision on the wrong branch.
        const picked = await vscode.window.showQuickPick(choice.candidates, {
          title: `Which checkout records this decision for ${design.adr_path}?`,
          placeHolder: "Several open worktrees contain this ADR",
          ignoreFocusOut: true,
        });
        if (!picked) {
          throw new Error(
            `cancelled: ${design.adr_path} exists in ${choice.candidates.length} open worktrees and none was chosen`
          );
        }
        const target = candidates.find((uri) => uri.fsPath === picked);
        if (!target) {
          throw new Error(`cannot resolve the chosen checkout for ${design.adr_path}`);
        }
        return target;
      }
    }
  }

  private async promptHuman(title: string, design: DesignItem): Promise<string | undefined> {
    return vscode.window.showInputBox({
      title: `${title}: ${design.title}`,
      prompt: "Human reviewer identity",
      ignoreFocusOut: true,
      validateInput: (value) => {
        const identity = value.trim();
        if (!identity) {
          return "A human reviewer identity is required.";
        }
        if (identity === design.proposed_by || identity === this.agentId) {
          return "The proposing agent may not decide its own design.";
        }
        return undefined;
      },
    });
  }

  private requireItem(item: DesignBoardItem | undefined, action: string): item is DesignBoardItem {
    if (!this.client.isReady()) {
      vscode.window.showWarningMessage("Lodestar must be connected to use the Design Board.");
      return false;
    }
    if (!item) {
      vscode.window.showWarningMessage(`Run ${action} from a Design Board row.`);
      return false;
    }
    return true;
  }

  private reportError(action: string, error: unknown): void {
    const message = `${action} failed: ${describeError(error)}`;
    this.log(message);
    vscode.window.showErrorMessage(`MindLeak ${message}`);
  }

  /**
   * A dismissed quick pick or input box used to return silently, which is
   * indistinguishable from a failed materialization: the design simply stayed
   * pending with no message and no log entry.
   */
  private reportCancelled(action: string, design: DesignItem): void {
    const message = `${action} cancelled - ${design.title} is unchanged.`;
    this.log(message);
    vscode.window.showInformationMessage(`MindLeak ${message}`);
  }
}

export interface SkippedAdr {
  adrPath: string;
  reason: string;
}

export interface WorkspaceAdrScan {
  metadata: DesignMetadata[];
  skipped: SkippedAdr[];
}

/** A thrown value is not always an `Error`; say something useful regardless. */
export function describeError(error: unknown): string {
  if (error instanceof Error) {
    return error.message;
  }
  return typeof error === "string" ? error : JSON.stringify(error);
}

export async function readWorkspaceAdrMetadata(proposedBy: string): Promise<WorkspaceAdrScan> {
  const files = await vscode.workspace.findFiles(
    "**/docs/adr/*.md",
    "**/{.git,node_modules,target,.vscode-test}/**"
  );
  const metadata: DesignMetadata[] = [];
  const skipped: SkippedAdr[] = [];
  for (const uri of files) {
    const folder = vscode.workspace.getWorkspaceFolder(uri);
    if (!folder) {
      continue;
    }
    const relativePath = path.relative(folder.uri.fsPath, uri.fsPath).replace(/\\/g, "/");
    const content = Buffer.from(await vscode.workspace.fs.readFile(uri)).toString("utf8");
    const parsed = parseAdrMetadata(relativePath, content, proposedBy);
    if (parsed) {
      metadata.push(parsed);
      continue;
    }
    const raw = rawAdrStatus(content);
    skipped.push({
      adrPath: relativePath,
      reason: raw ? `unrecognised status "${raw}"` : "no readable title or Status line",
    });
  }
  metadata.sort((left, right) => left.adr_path.localeCompare(right.adr_path));
  skipped.sort((left, right) => left.adrPath.localeCompare(right.adrPath));
  return { metadata, skipped };
}
