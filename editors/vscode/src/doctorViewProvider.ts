import * as vscode from "vscode";

import { BoardFinding, doctorAilmentIcon, DoctorGroup } from "./fleet";

/** A node in the Board Doctor tree: an ailment group, or one finding within it. */
export type DoctorNode =
  | { kind: "group"; group: DoctorGroup }
  | { kind: "finding"; group: DoctorGroup; finding: BoardFinding };

/**
 * A two-level tree of `task_query(view=doctor)` findings, grouped by ailment
 * ({@link import("./fleet").doctorGroups}) so a crowded board reads as a small
 * number of conditions rather than an undifferentiated list. Read-only: this
 * view diagnoses and never mutates (ADR-0015) — which duplicate is the real
 * work is a call only the reader can make.
 */
export class DoctorViewProvider implements vscode.TreeDataProvider<DoctorNode> {
  static readonly viewType = "mindleak.doctorView";

  private groups: DoctorGroup[] = [];
  private readonly emitter = new vscode.EventEmitter<void>();
  readonly onDidChangeTreeData = this.emitter.event;

  update(groups: DoctorGroup[]): void {
    this.groups = Array.isArray(groups) ? groups : [];
    this.emitter.fire();
  }

  /** Every finding across every group, for a badge count. */
  get findingCount(): number {
    return this.groups.reduce((total, group) => total + group.findings.length, 0);
  }

  getTreeItem(node: DoctorNode): vscode.TreeItem {
    if (node.kind === "group") {
      const count = node.group.findings.length;
      const item = new vscode.TreeItem(node.group.label, vscode.TreeItemCollapsibleState.Expanded);
      item.description = `${count} finding${count === 1 ? "" : "s"}`;
      item.iconPath = new vscode.ThemeIcon(doctorAilmentIcon(node.group.ailment));
      item.contextValue = "doctor.group";
      return item;
    }
    const { finding } = node;
    const taskIds = finding.task_ids ?? [];
    const item = new vscode.TreeItem(
      finding.subject || taskIds[0] || "finding",
      vscode.TreeItemCollapsibleState.None
    );
    item.description = taskIds.length
      ? `${taskIds.length} task${taskIds.length === 1 ? "" : "s"}`
      : undefined;
    item.tooltip = finding.remedy || undefined;
    item.iconPath = new vscode.ThemeIcon(doctorAilmentIcon(node.group.ailment));
    item.contextValue = "doctor.finding";
    return item;
  }

  getChildren(node?: DoctorNode): DoctorNode[] {
    if (!node) {
      return this.groups.map((group) => ({ kind: "group", group }));
    }
    if (node.kind === "group") {
      return node.group.findings.map((finding) => ({
        kind: "finding",
        group: node.group,
        finding,
      }));
    }
    return [];
  }
}
