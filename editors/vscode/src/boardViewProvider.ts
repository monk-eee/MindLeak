import * as vscode from "vscode";

import { BoardRow, boardRows, LodestarTask, taskContextValue } from "./util";

/** A single task row in the board tree. */
export class BoardItem extends vscode.TreeItem {
  constructor(
    readonly task: LodestarTask,
    row: BoardRow,
    currentAgent?: string
  ) {
    super(row.label, vscode.TreeItemCollapsibleState.None);
    this.description = row.description;
    this.tooltip = row.tooltip;
    this.contextValue = taskContextValue(task, Math.floor(Date.now() / 1000), currentAgent);
    this.iconPath = iconFor(row.status);
  }
}

function iconFor(status: string): vscode.ThemeIcon {
  switch (status) {
    case "claimed":
      return new vscode.ThemeIcon("account");
    case "needs_input":
      return new vscode.ThemeIcon("comment-unresolved");
    case "paused":
      return new vscode.ThemeIcon("debug-pause");
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
  private items: BoardItem[] = [];
  private readonly emitter = new vscode.EventEmitter<void>();
  readonly onDidChangeTreeData = this.emitter.event;

  constructor(private readonly currentAgent?: string) {}

  update(tasks: LodestarTask[]): void {
    this.tasks = Array.isArray(tasks) ? tasks : [];
    this.items = boardRows(this.tasks).map((row) => {
      const task = this.tasks.find((candidate) => candidate.id === row.id);
      return new BoardItem(task!, row, this.currentAgent);
    });
    this.emitter.fire();
  }

  find(taskId: string): BoardItem | undefined {
    return this.items.find((item) => item.task.id === taskId);
  }

  getTreeItem(element: BoardItem): vscode.TreeItem {
    return element;
  }

  getChildren(): BoardItem[] {
    return this.items;
  }
}
