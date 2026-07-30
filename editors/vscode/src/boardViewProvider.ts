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
    // The icon is chosen in `boardIconId`, beside the sort that already knows an
    // expired claim is ready work — status alone cannot tell the two apart.
    this.iconPath = new vscode.ThemeIcon(row.icon);
  }
}

/**
 * A tree view of the Lodestar task board — who owns what, at a glance. Fed from
 * `task_query(view=board)`; rendering order/format is the pure {@link boardRows}.
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
