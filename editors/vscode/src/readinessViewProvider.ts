import * as vscode from "vscode";

import { ReadinessRow, ReadinessSnapshot, readinessRows } from "./readiness";

class ReadinessItem extends vscode.TreeItem {
  constructor(row: ReadinessRow) {
    super(row.label, vscode.TreeItemCollapsibleState.None);
    this.id = row.id;
    this.description = row.description;
    this.tooltip = row.tooltip;
    this.contextValue = `readiness.${row.kind}`;
    this.iconPath = new vscode.ThemeIcon(row.icon);
    if (row.action) {
      this.command = {
        command: row.action.command,
        title: row.action.label,
        arguments: row.action.arguments,
      };
    }
  }
}

export class ReadinessViewProvider implements vscode.TreeDataProvider<ReadinessItem> {
  static readonly viewType = "mindleak.readinessView";

  private snapshot?: ReadinessSnapshot;
  private readonly emitter = new vscode.EventEmitter<void>();
  readonly onDidChangeTreeData = this.emitter.event;

  update(snapshot: ReadinessSnapshot): void {
    this.snapshot = snapshot;
    this.emitter.fire();
  }

  getTreeItem(item: ReadinessItem): vscode.TreeItem {
    return item;
  }

  getChildren(): ReadinessItem[] {
    return this.snapshot ? readinessRows(this.snapshot).map((row) => new ReadinessItem(row)) : [];
  }
}
