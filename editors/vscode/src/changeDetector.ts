import * as vscode from "vscode";

export interface PathChangeDetector {
  onDidChange: vscode.Event<string>;
}

/** Emits normalized workspace-relative paths for create/change/delete events. */
export class WorkspaceChangeDetector implements PathChangeDetector, vscode.Disposable {
  private readonly emitter = new vscode.EventEmitter<string>();
  private readonly watcher = vscode.workspace.createFileSystemWatcher("**/*");
  private readonly subscriptions: vscode.Disposable[];
  readonly onDidChange = this.emitter.event;

  constructor() {
    const emit = (uri: vscode.Uri): void => this.emit(uri);
    this.subscriptions = [
      this.watcher,
      this.watcher.onDidCreate(emit),
      this.watcher.onDidChange(emit),
      this.watcher.onDidDelete(emit),
      this.emitter,
    ];
  }

  dispose(): void {
    for (const subscription of this.subscriptions) {
      subscription.dispose();
    }
  }

  private emit(uri: vscode.Uri): void {
    if (uri.scheme !== "file" || !vscode.workspace.getWorkspaceFolder(uri)) {
      return;
    }
    this.emitter.fire(vscode.workspace.asRelativePath(uri, false).replace(/\\/g, "/"));
  }
}
