import * as vscode from "vscode";

import { DesignBoardRow, DesignItem, DesignPromotion, projectDesignBoard } from "./designBoard";

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

class DesignBoardMetaItem extends vscode.TreeItem {
  constructor(label: string, contextValue: string, icon: string, command?: vscode.Command) {
    super(label, vscode.TreeItemCollapsibleState.None);
    this.contextValue = contextValue;
    this.iconPath = new vscode.ThemeIcon(icon);
    this.command = command;
  }
}

type DesignBoardTreeItem = DesignBoardItem | DesignBoardMetaItem;

export class DesignBoardViewProvider implements vscode.TreeDataProvider<DesignBoardTreeItem> {
  static readonly viewType = "mindleak.designView";

  private designs: DesignItem[] = [];
  private promotions = new Map<string, DesignPromotion>();
  private expanded = false;
  private deferredVisible = false;
  private readonly emitter = new vscode.EventEmitter<void>();
  readonly onDidChangeTreeData = this.emitter.event;

  update(designs: DesignItem[], promotions: ReadonlyMap<string, DesignPromotion>): void {
    this.designs = Array.isArray(designs) ? designs : [];
    this.promotions = new Map(promotions);
    this.emitter.fire();
  }

  get includeDeferred(): boolean {
    return this.deferredVisible;
  }

  expand(): void {
    this.expanded = true;
    this.emitter.fire();
  }

  setIncludeDeferred(include: boolean): void {
    this.deferredVisible = include;
    this.expanded = false;
    this.emitter.fire();
  }

  getTreeItem(element: DesignBoardTreeItem): vscode.TreeItem {
    return element;
  }

  getChildren(): DesignBoardTreeItem[] {
    const projection = projectDesignBoard(this.designs, this.promotions, this.expanded);
    const items: DesignBoardTreeItem[] = [
      new DesignBoardMetaItem(
        `${projection.awaitingDecision} awaiting decision`,
        "design.summary",
        "list-selection"
      ),
      ...projection.rows.map((row) => new DesignBoardItem(row)),
    ];
    if (projection.hidden > 0) {
      items.push(
        new DesignBoardMetaItem(`Show ${projection.hidden} more`, "design.expand", "ellipsis", {
          command: "mindleak.design.expand",
          title: "Show All Designs",
        })
      );
    }
    return items;
  }
}

function iconFor(context: string): vscode.ThemeIcon {
  switch (context) {
    case "proposed":
      return new vscode.ThemeIcon("request-changes");
    case "pending":
      return new vscode.ThemeIcon("rocket");
    case "deferred":
      return new vscode.ThemeIcon("debug-pause");
    case "materialized":
      return new vscode.ThemeIcon("verified-filled");
    case "rejected":
      return new vscode.ThemeIcon("circle-slash");
    default:
      return new vscode.ThemeIcon("history");
  }
}
