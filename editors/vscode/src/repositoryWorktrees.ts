import { execFile } from "node:child_process";
import { promisify } from "node:util";

const run = promisify(execFile);
const GIT_TIMEOUT_MS = 5_000;
const GIT_OUTPUT_LIMIT = 1 << 20;

export type WorktreeReader = (workspace: string) => Promise<string[]>;

function normalizePath(value: string): string {
  return value.replace(/\\/g, "/").replace(/\/+$/, "");
}

/** Whether `candidate` is inside one of this repository's worktrees. */
export function belongsToWorktree(candidate: string, roots: readonly string[]): boolean {
  const target = normalizePath(candidate);
  const foldedTarget = process.platform === "win32" ? target.toLowerCase() : target;
  return roots.some((root) => {
    const normalizedRoot = normalizePath(root);
    const foldedRoot = process.platform === "win32" ? normalizedRoot.toLowerCase() : normalizedRoot;
    return foldedTarget === foldedRoot || foldedTarget.startsWith(`${foldedRoot}/`);
  });
}

/** Discover every worktree attached to the repository containing `workspace`. */
export async function readRepositoryWorktrees(workspace: string): Promise<string[]> {
  try {
    const { stdout } = await run("git", ["worktree", "list", "--porcelain"], {
      cwd: workspace,
      timeout: GIT_TIMEOUT_MS,
      maxBuffer: GIT_OUTPUT_LIMIT,
    });
    return stdout
      .split(/\r?\n/)
      .filter((line) => line.startsWith("worktree "))
      .map((line) => normalizePath(line.slice("worktree ".length)))
      .filter(Boolean);
  } catch {
    return [];
  }
}

/**
 * Cached repository membership for editor events outside the current workspace.
 * An unknown path refreshes once so a worktree created after activation is seen.
 */
export class RepositoryWorktrees {
  private roots: string[];

  constructor(
    private readonly workspace: string,
    private readonly readRoots: WorktreeReader = readRepositoryWorktrees
  ) {
    this.roots = [normalizePath(workspace)];
  }

  async contains(candidate: string): Promise<boolean> {
    if (belongsToWorktree(candidate, this.roots)) {
      return true;
    }
    const refreshed = await this.readRoots(this.workspace);
    this.roots = refreshed.length > 0 ? refreshed : [normalizePath(this.workspace)];
    return belongsToWorktree(candidate, this.roots);
  }
}
