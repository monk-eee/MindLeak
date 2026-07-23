import * as vscode from "vscode";

import { designBoardRows, DesignBoardRow, DesignItem, DesignPromotion } from "./designBoard";

export class DesignBoardItem extends vscode.TreeItem {
  constructor(readonly row: DesignBoardRow) {
    super(row.label, vscode.TreeItemCollapsibleState.None);
    this.description = row.description;
    this.tooltip = row.tooltip;
    this.contextValue = `design.${row.contextValue}`;
    this.iconPath = iconFor(row.contextValue);
    this.command = {
      command: "mindleak.design.openAdr",
      title: "Open ADR",
      arguments: [this],
    };
  }

  get design(): DesignItem {
    return this.row.item;
  }

  get promotion(): DesignPromotion | undefined {
    return this.row.promotion;
  }
}

export class DesignBoardViewProvider implements vscode.TreeDataProvider<DesignBoardItem> {
  static readonly viewType = "mindleak.designView";

  private designs: DesignItem[] = [];
  private promotions = new Map<string, DesignPromotion>();
  private readonly emitter = new vscode.EventEmitter<void>();
  readonly onDidChangeTreeData = this.emitter.event;

  update(designs: DesignItem[], promotions: ReadonlyMap<string, DesignPromotion>): void {
    this.designs = Array.isArray(designs) ? designs : [];
    this.promotions = new Map(promotions);
    this.emitter.fire();
  }

  getTreeItem(element: DesignBoardItem): vscode.TreeItem {
    return element;
  }

  getChildren(): DesignBoardItem[] {
    return designBoardRows(this.designs, this.promotions).map((row) => new DesignBoardItem(row));
  }
}

function iconFor(context: string): vscode.ThemeIcon {
  switch (context) {
    case "proposed":
      return new vscode.ThemeIcon("request-changes");
    case "pending":
      return new vscode.ThemeIcon("rocket");
    case "materialized":
      return new vscode.ThemeIcon("verified-filled");
    case "rejected":
      return new vscode.ThemeIcon("circle-slash");
    default:
      return new vscode.ThemeIcon("history");
  }
}
