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
    HEAD?: { commit?: string; name?: string };
    onDidChange: vscode.Event<void>;
  };
  onDidCommit: vscode.Event<void>;
  onDidCheckout: vscode.Event<void>;
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

interface ObservedHead {
  commit: string;
  branch?: string;
}

/** Ingests commits reported by VS Code's built-in Git extension. */
export class GitSensor implements vscode.Disposable {
  private readonly subscriptions: vscode.Disposable[] = [];
  private readonly repositories = new Map<string, vscode.Disposable[]>();
  private readonly heads = new Map<string, ObservedHead>();
  private readonly inFlight = new Set<string>();
  private readonly explicitCommits = new Set<string>();

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
    this.heads.clear();
    this.inFlight.clear();
    this.explicitCommits.clear();
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
      this.rememberHead(repository);
    }
    this.repositories.set(key, [
      repository.onDidCommit(() => void this.captureHead(repository, true)),
      repository.onDidCheckout(() => this.rememberHead(repository)),
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
    this.clearExplicitCommits(key);
    this.updateHealth();
  }

  private async captureHead(repository: GitRepository, explicitCommit: boolean): Promise<void> {
    const key = repository.rootUri.toString();
    const previous = this.heads.get(key);
    const current = observedHead(repository);
    if (!current) {
      return;
    }
    const { commit: head, branch } = current;
    const flightKey = `${key}:${head}`;
    if (explicitCommit) {
      // onDidCommit fires after the repository state refresh. If that refresh
      // already started capture, marking the HEAD here upgrades the in-flight
      // observation instead of losing an amend/non-linear commit as a duplicate.
      this.explicitCommits.add(flightKey);
    }
    if (sameHead(previous, current)) {
      this.explicitCommits.delete(flightKey);
      return;
    }
    if (!this.enabled()) {
      this.rememberObservedHead(key, current);
      return;
    }
    if (!this.client.isReady()) {
      return;
    }

    if (this.inFlight.has(flightKey)) {
      return;
    }

    this.inFlight.add(flightKey);
    try {
      const commit = await repository.getCommit(head);

      // VS Code's checkout event fires after its status refresh. That refresh
      // can already be awaiting getCommit here; the checkout handler advances
      // `heads`, and this re-check cancels the stale capture before attribution.
      if (sameHead(this.heads.get(key), current)) {
        return;
      }

      const isExplicitCommit = explicitCommit || this.explicitCommits.has(flightKey);
      // A branch change is a checkout, even when the target is a descendant and
      // therefore names the previous HEAD as a parent. This decision belongs
      // here, after the await: VS Code fires onDidCommit/onDidCheckout after its
      // state refresh, and either may classify a capture already in flight.
      if (!isExplicitCommit && previous && previous.branch !== branch) {
        this.rememberObservedHead(key, current);
        return;
      }
      if (!isExplicitCommit && previous && !commit.parents.includes(previous.commit)) {
        // A state-only non-linear move may be reset, rebase, or an external
        // checkout. Refusing to guess is safer than attributing old history.
        this.rememberObservedHead(key, current);
        this.health("Git capture degraded: non-linear HEAD change not attributed");
        this.log(`Git capture skipped non-linear HEAD change ${previous.commit} -> ${head}`);
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
      this.heads.set(key, current);
      this.health(`Git capture active (${this.repositories.size} repositories)`);
    } catch (error) {
      this.health("Git capture degraded: ingestion failed");
      this.log(`Git capture error: ${(error as Error).message}`);
    } finally {
      this.inFlight.delete(flightKey);
      this.explicitCommits.delete(flightKey);
    }
  }

  private rememberHead(repository: GitRepository): void {
    const head = observedHead(repository);
    if (head) {
      this.rememberObservedHead(repository.rootUri.toString(), head);
    }
  }

  private rememberObservedHead(key: string, head: ObservedHead): void {
    this.clearExplicitCommits(key);
    this.heads.set(key, head);
  }

  private clearExplicitCommits(repositoryKey: string): void {
    const prefix = `${repositoryKey}:`;
    for (const key of this.explicitCommits) {
      if (key.startsWith(prefix)) {
        this.explicitCommits.delete(key);
      }
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

function observedHead(repository: GitRepository): ObservedHead | undefined {
  const { commit, name } = repository.state.HEAD ?? {};
  return commit ? { commit, branch: name } : undefined;
}

function sameHead(left: ObservedHead | undefined, right: ObservedHead): boolean {
  return left?.commit === right.commit && left.branch === right.branch;
}

function relativePath(uri: vscode.Uri): string {
  return vscode.workspace.asRelativePath(uri, false).replace(/\\/g, "/");
}
