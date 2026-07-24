import * as vscode from "vscode";

import { ConformanceRecord, EvidenceGroup, verdictIconId } from "./util";

/** A node in the Evidence Board tree: a task group, or one check within it. */
export type EvidenceNode =
  | { kind: "group"; group: EvidenceGroup }
  | { kind: "record"; group: EvidenceGroup; record: ConformanceRecord };

/**
 * A two-level tree of conformance proof: each task with a `conformance_history`
 * chain, expandable to its individual checks (the drift→aligned story a reviewer
 * needs). Fed from the `board` + `conformance_history` tools; grouping/order is
 * the pure {@link import("./util").evidenceGroups}. Actions resolve the task id
 * from the node, so vscode-coupled code here stays thin.
 */
export class EvidenceBoardViewProvider implements vscode.TreeDataProvider<EvidenceNode> {
  static readonly viewType = "mindleak.evidenceView";

  private groups: EvidenceGroup[] = [];
  private readonly emitter = new vscode.EventEmitter<void>();
  readonly onDidChangeTreeData = this.emitter.event;

  update(groups: EvidenceGroup[]): void {
    this.groups = Array.isArray(groups) ? groups : [];
    this.emitter.fire();
  }

  getTreeItem(node: EvidenceNode): vscode.TreeItem {
    if (node.kind === "group") {
      const checks = `${node.group.checkCount} check${node.group.checkCount === 1 ? "" : "s"}`;
      const item = new vscode.TreeItem(node.group.title, vscode.TreeItemCollapsibleState.Collapsed);
      item.description = `${node.group.latestVerdict} · ${checks}`;
      item.tooltip = `${node.group.taskId}\nLatest verdict: ${node.group.latestVerdict}`;
      item.iconPath = new vscode.ThemeIcon(verdictIconId(node.group.latestVerdict));
      item.contextValue = "evidence.group";
      return item;
    }
    const { record } = node;
    const item = new vscode.TreeItem(
      `#${record.id} · ${record.verdict}`,
      vscode.TreeItemCollapsibleState.None
    );
    item.description = record.findings || "";
    item.tooltip = record.findings || record.verdict;
    item.iconPath = new vscode.ThemeIcon(verdictIconId(record.verdict));
    item.contextValue = "evidence.record";
    return item;
  }

  getChildren(node?: EvidenceNode): EvidenceNode[] {
    if (!node) {
      return this.groups.map((group) => ({ kind: "group", group }));
    }
    if (node.kind === "group") {
      // Newest check first within a group, matching the top-level order.
      return node.group.records
        .slice()
        .reverse()
        .map((record) => ({ kind: "record", group: node.group, record }));
    }
    return [];
  }
}
