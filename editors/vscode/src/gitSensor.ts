import * as vscode from "vscode";

import type { SensorClient } from "./terminalSensor";

const EMPTY_TREE = "4b825dc642cb6eb9a060e54bf8d69288fbee4904";

interface GitCommit {
  hash: string;
  message: string;
  parents: string[];
  authorDate?: Date;
  commitDate?: Date;
}

interface GitChange {
  uri: vscode.Uri;
}

interface GitRepository {
  rootUri: vscode.Uri;
  state: {
    HEAD?: { commit?: string };
    onDidChange: vscode.Event<void>;
  };
  onDidCommit: vscode.Event<void>;
  getCommit(ref: string): Promise<GitCommit>;
  diffBetween(ref1: string, ref2: string): Promise<GitChange[]>;
}

interface GitApi {
  repositories: GitRepository[];
  onDidOpenRepository: vscode.Event<GitRepository>;
  onDidCloseRepository: vscode.Event<GitRepository>;
}

interface GitExtension {
  enabled: boolean;
  getAPI(version: 1): GitApi;
}

/** Ingests commits reported by VS Code's built-in Git extension. */
export class GitSensor implements vscode.Disposable {
  private readonly subscriptions: vscode.Disposable[] = [];
  private readonly repositories = new Map<string, vscode.Disposable[]>();
  private readonly heads = new Map<string, string>();
  private readonly inFlight = new Set<string>();

  constructor(
    private readonly client: SensorClient,
    private readonly enabled: () => boolean,
    private readonly log: (message: string) => void,
    private readonly health: (status: string) => void
  ) {}

  async start(): Promise<void> {
    const extension = vscode.extensions.getExtension<GitExtension>("vscode.git");
    if (!extension) {
      this.health("Git capture degraded: built-in Git extension unavailable");
      return;
    }
    const git = await extension.activate();
    if (!git.enabled) {
      this.health("Git capture degraded: built-in Git extension disabled");
      return;
    }
    const api = git.getAPI(1);
    this.subscriptions.push(vscode.workspace.onDidChangeConfiguration(() => this.updateHealth()));
    for (const repository of api.repositories) {
      this.attach(repository);
    }
    this.subscriptions.push(api.onDidOpenRepository((repository) => this.attach(repository)));
    this.subscriptions.push(api.onDidCloseRepository((repository) => this.detach(repository)));
    this.updateHealth();
  }

  dispose(): void {
    for (const subscription of this.subscriptions) {
      subscription.dispose();
    }
    for (const subscriptions of this.repositories.values()) {
      for (const subscription of subscriptions) {
        subscription.dispose();
      }
    }
    this.repositories.clear();
  }

  private attach(repository: GitRepository): void {
    if (!vscode.workspace.getWorkspaceFolder(repository.rootUri)) {
      return;
    }
    const key = repository.rootUri.toString();
    if (this.repositories.has(key)) {
      return;
    }
    const current = repository.state.HEAD?.commit;
    if (current) {
      this.heads.set(key, current);
    }
    this.repositories.set(key, [
      repository.onDidCommit(() => void this.captureHead(repository, true)),
      repository.state.onDidChange(() => void this.captureHead(repository, false)),
    ]);
    this.updateHealth();
  }

  private detach(repository: GitRepository): void {
    const key = repository.rootUri.toString();
    for (const subscription of this.repositories.get(key) ?? []) {
      subscription.dispose();
    }
    this.repositories.delete(key);
    this.heads.delete(key);
    this.updateHealth();
  }

  private async captureHead(repository: GitRepository, explicitCommit: boolean): Promise<void> {
    const key = repository.rootUri.toString();
    const previous = this.heads.get(key);
    const head = repository.state.HEAD?.commit;
    if (!head || head === previous) {
      return;
    }
    if (!this.enabled()) {
      this.heads.set(key, head);
      return;
    }
    if (!this.client.isReady()) {
      return;
    }

    const flightKey = `${key}:${head}`;
    if (this.inFlight.has(flightKey)) {
      return;
    }
    this.inFlight.add(flightKey);
    try {
      const commit = await repository.getCommit(head);
      if (!explicitCommit && previous && !commit.parents.includes(previous)) {
        this.heads.set(key, head);
        return;
      }
      const changes = await repository.diffBetween(commit.parents[0] ?? EMPTY_TREE, head);
      const changedFiles = [...new Set(changes.map((change) => relativePath(change.uri)))].sort();
      const date = commit.commitDate ?? commit.authorDate ?? new Date();
      await this.client.callTool("ingest_commit", {
        sha: commit.hash,
        message: commit.message,
        changed_files: changedFiles,
        timestamp: Math.floor(date.getTime() / 1000),
      });
      this.heads.set(key, head);
      this.health(`Git capture active (${this.repositories.size} repositories)`);
    } catch (error) {
      this.health("Git capture degraded: ingestion failed");
      this.log(`Git capture error: ${(error as Error).message}`);
    } finally {
      this.inFlight.delete(flightKey);
    }
  }

  private updateHealth(): void {
    if (!this.enabled()) {
      this.health("Git capture disabled");
    } else if (this.repositories.size === 0) {
      this.health("Git capture waiting for a repository");
    } else {
      this.health(`Git capture active (${this.repositories.size} repositories)`);
    }
  }
}

function relativePath(uri: vscode.Uri): string {
  return vscode.workspace.asRelativePath(uri, false).replace(/\\/g, "/");
}
