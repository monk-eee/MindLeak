import * as vscode from "vscode";

import {
  AgentRoster,
  BoardFinding,
  fleetDashboard,
  FleetSnapshot,
  FleetVerb,
  PlaneHealth,
  StalledEntry,
} from "./fleet";
import { FleetViewProvider } from "./fleetViewProvider";
import { McpClient } from "./mcpClient";
import { boardTasks, LodestarTask } from "./util";

/** Lease granted by a claim or renewal started from the pane. */
const LEASE_SECONDS = 3600;

/**
 * Drives the Fleet pane: reads both planes, shapes one readout, and runs the
 * lifecycle verb a row offered.
 *
 * Each read is isolated, so one unanswered query degrades that section rather
 * than blanking the pane — a roster that silently drops a working agent is
 * worse than one that says a plane is down.
 */
export class FleetController {
  constructor(
    private readonly lodestar: McpClient,
    private readonly memory: McpClient,
    private readonly view: FleetViewProvider,
    private readonly selfAgentId: string | undefined,
    private readonly log: (message: string) => void,
    private readonly onBoardChanged: () => void | Promise<void>
  ) {}

  async refresh(): Promise<void> {
    const planes: PlaneHealth = {
      intent: this.lodestar.isReady(),
      memory: this.memory.isReady(),
    };

    const [snapshot, tasks, stalled, ailments, roster] = await Promise.all([
      this.read<FleetSnapshot>(this.lodestar, "fleet snapshot", () =>
        this.lodestar.callTool("fleet_view", {})
      ),
      this.read<LodestarTask[]>(this.lodestar, "board", async () =>
        // fleetDashboard's FleetTaskRow mapping only reads id/title/status/
        // lease_expires_at -- never scope/claim_window/receipt/acceptance.
        boardTasks(
          await this.lodestar.callTool("task_query", {
            view: "board",
            include_terminal: false,
            detail: false,
          })
        )
      ),
      this.read<StalledEntry[]>(this.lodestar, "stalled work", () =>
        this.lodestar.callTool("task_query", { view: "stalled" })
      ),
      this.read<BoardFinding[]>(this.lodestar, "board doctor", () =>
        this.lodestar.callTool("task_query", { view: "doctor" })
      ),
      this.read<AgentRoster>(this.memory, "agent roster", () =>
        this.memory.callTool("list_agents", {})
      ),
    ]);

    this.view.update(
      fleetDashboard({
        snapshot,
        roster,
        tasks,
        stalled,
        ailments,
        planes,
        selfAgentId: this.selfAgentId,
        nowUnix: Math.floor(Date.now() / 1000),
      })
    );
  }

  /** Run one verb, then refresh both this pane and the board that shares its state. */
  async act(verb: FleetVerb, taskId: string, agentId: string): Promise<void> {
    try {
      switch (verb) {
        case "renew":
          await this.lodestar.callTool("task_claim", {
            task_id: taskId,
            step: "renew",
            lease_secs: LEASE_SECONDS,
          });
          break;
        case "release":
          await this.lodestar.callTool("task_claim", { task_id: taskId, step: "release" });
          break;
        case "claim":
          await this.lodestar.callTool("task_claim", {
            task_id: taskId,
            step: "claim",
            lease_secs: LEASE_SECONDS,
          });
          break;
        case "pause": {
          const reason = await this.ask("Why is this work being paused?");
          if (!reason) {
            return;
          }
          await this.lodestar.callTool("task_transition", {
            task_id: taskId,
            to: "pause",
            reason,
          });
          break;
        }
        case "resume":
          await this.lodestar.callTool("task_transition", { task_id: taskId, to: "resume" });
          break;
        case "recover": {
          // Naming the owner is the point: a recovery that does not say who it
          // takes from is not a recovery.
          const reason = await this.ask(`Why is this claim being taken from ${agentId}?`);
          if (!reason) {
            return;
          }
          await this.lodestar.callTool("task_claim", {
            task_id: taskId,
            step: "recover",
            expected_owner: agentId,
            reason,
            lease_secs: LEASE_SECONDS,
          });
          break;
        }
      }
      vscode.window.showInformationMessage(`MindLeak: ${verb} on ${taskId}.`);
    } catch (err) {
      const detail = (err as Error).message;
      this.log(`fleet ${verb} on ${taskId} refused: ${detail}`);
      vscode.window.showWarningMessage(`MindLeak: ${verb} refused. ${detail}`);
    }
    await this.refresh();
    await this.onBoardChanged();
  }

  private async ask(prompt: string): Promise<string | undefined> {
    const value = await vscode.window.showInputBox({
      prompt,
      ignoreFocusOut: true,
      validateInput: (input) => (input.trim().length > 0 ? undefined : "A reason is required."),
    });
    return value?.trim() || undefined;
  }

  /**
   * One isolated read. The caller passes the call already bound to a literal
   * tool name — a name assembled here would read as a dynamic verb and defeat
   * the vocabulary guard that keeps the extension honest about what it calls.
   */
  private async read<T>(
    client: McpClient,
    label: string,
    call: () => Promise<unknown>
  ): Promise<T | null> {
    if (!client.isReady()) {
      return null;
    }
    try {
      return (await call()) as T;
    } catch (err) {
      this.log(`fleet read ${label} failed: ${(err as Error).message}`);
      return null;
    }
  }
}
