import * as path from "path";
import * as vscode from "vscode";

import {
  DesignGoal,
  DesignItem,
  DesignMetadata,
  DesignPromotion,
  formatDesignPromotion,
  parseAdrMetadata,
} from "./designBoard";
import { DesignBoardItem, DesignBoardViewProvider } from "./designBoardViewProvider";
import { McpClient } from "./mcpClient";

interface ConstitutionGoal extends DesignGoal {
  status: string;
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
      const metadata = await readWorkspaceAdrMetadata(this.agentId);
      await this.client.callTool("reconcile_designs", { designs: metadata });
      await this.refresh();
      this.log(`Design Board synchronized ${metadata.length} repository ADRs.`);
    } catch (error) {
      this.reportError("ADR synchronization", error);
    }
  }

  async refresh(): Promise<void> {
    if (!this.client.isReady()) {
      return;
    }
    try {
      const designs = (await this.client.callTool("list_designs", {})) as DesignItem[];
      const materialized = designs.filter((design) => design.promotion_status === "materialized");
      const promotionEntries = await Promise.all(
        materialized.map(async (design) => {
          const promotion = (await this.client.callTool("design_promotion", {
            id: design.id,
          })) as DesignPromotion | null;
          return [design.id, promotion] as const;
        })
      );
      const promotions = new Map<string, DesignPromotion>();
      for (const [id, promotion] of promotionEntries) {
        if (promotion) {
          promotions.set(id, promotion);
        }
      }
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
      await this.client.callTool("accept_design", { id: item.design.id, human });
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
      await this.client.callTool("reject_design", {
        id: item.design.id,
        human,
        reason: reason.trim(),
      });
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
      const goals = (await this.client.callTool("get_constitution", {})) as ConstitutionGoal[];
      const objectives = goals.filter(
        (goal) => goal.kind === "objective" && goal.status === "active"
      );
      const selected = await vscode.window.showQuickPick(
        objectives.map((goal) => ({
          label: goal.title,
          description: goal.id,
          goal,
        })),
        {
          title: `Promote: ${item.design.title}`,
          placeHolder: "Select the objective that will own the implementation tasks",
          ignoreFocusOut: true,
        }
      );
      if (!selected) {
        return;
      }
      const promotion = (await this.client.callTool("promote_design", {
        id: item.design.id,
        objective_goal_id: selected.goal.id,
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

  async openAdr(item?: DesignBoardItem): Promise<void> {
    if (!this.requireItem(item, "Open ADR")) {
      return;
    }
    const folder = vscode.workspace.workspaceFolders?.[0];
    if (!folder) {
      vscode.window.showWarningMessage("Open a workspace to inspect an ADR.");
      return;
    }
    const uri = vscode.Uri.file(path.join(folder.uri.fsPath, item.design.adr_path));
    try {
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
        ((await this.client.callTool("design_promotion", {
          id: item.design.id,
        })) as DesignPromotion | null);
      if (!promotion) {
        vscode.window.showInformationMessage(
          `No materialized implementation exists for ${item.design.title}.`
        );
        return;
      }
      const document = await vscode.workspace.openTextDocument({
        content: formatDesignPromotion(promotion),
        language: "markdown",
      });
      await vscode.window.showTextDocument(document, { preview: true });
    } catch (error) {
      this.reportError("Materialization inspection", error);
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
    const message = `${action} failed: ${(error as Error).message}`;
    this.log(message);
    vscode.window.showErrorMessage(`MindLeak ${message}`);
  }
}

export async function readWorkspaceAdrMetadata(proposedBy: string): Promise<DesignMetadata[]> {
  const files = await vscode.workspace.findFiles(
    "**/docs/adr/*.md",
    "**/{.git,node_modules,target,.vscode-test}/**"
  );
  const metadata: DesignMetadata[] = [];
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
    }
  }
  return metadata.sort((left, right) => left.adr_path.localeCompare(right.adr_path));
}
