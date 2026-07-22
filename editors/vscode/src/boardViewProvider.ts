import * as vscode from "vscode";

import { boardRows, LodestarTask } from "./util";

/** A single task row in the board tree. */
class BoardItem extends vscode.TreeItem {
  constructor(label: string, description: string, tooltip: string, status: string) {
    super(label, vscode.TreeItemCollapsibleState.None);
    this.description = description;
    this.tooltip = tooltip;
    this.contextValue = status;
    this.iconPath = iconFor(status);
  }
}

function iconFor(status: string): vscode.ThemeIcon {
  switch (status) {
    case "claimed":
      return new vscode.ThemeIcon("account");
    case "open":
      return new vscode.ThemeIcon("circle-outline");
    case "in_review":
      return new vscode.ThemeIcon("eye");
    case "blocked":
      return new vscode.ThemeIcon("error");
    case "done":
      return new vscode.ThemeIcon("check");
    default:
      return new vscode.ThemeIcon("circle-slash");
  }
}

/**
 * A tree view of the Lodestar task board — who owns what, at a glance. Fed from
 * the `board` MCP tool; rendering order/format is the pure {@link boardRows}.
 */
export class BoardViewProvider implements vscode.TreeDataProvider<BoardItem> {
  static readonly viewType = "mindleak.boardView";

  private tasks: LodestarTask[] = [];
  private readonly emitter = new vscode.EventEmitter<void>();
  readonly onDidChangeTreeData = this.emitter.event;

  update(tasks: LodestarTask[]): void {
    this.tasks = Array.isArray(tasks) ? tasks : [];
    this.emitter.fire();
  }

  getTreeItem(element: BoardItem): vscode.TreeItem {
    return element;
  }

  getChildren(): BoardItem[] {
    return boardRows(this.tasks).map(
      (r) => new BoardItem(r.label, r.description, r.tooltip, r.status)
    );
  }
}
