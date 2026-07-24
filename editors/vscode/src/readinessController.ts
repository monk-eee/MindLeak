import { McpClient } from "./mcpClient";
import { deriveReadiness, ReadinessInput, ReadinessSnapshot } from "./readiness";
import { ReadinessViewProvider } from "./readinessViewProvider";

export interface RuntimeHealth {
  memory: string;
  intent: string;
  terminal: string;
  git: string;
}

interface ReadinessFacts {
  graph?: ReadinessInput["graph"];
  actionableTasks: number;
  actionableDesigns: number;
  memoryError?: string;
  intentError?: string;
}

export class ReadinessController {
  private health: RuntimeHealth;
  private facts: ReadinessFacts = { actionableTasks: 0, actionableDesigns: 0 };
  private current: ReadinessSnapshot;

  constructor(
    private readonly memory: McpClient,
    private readonly intent: McpClient,
    private readonly provider: ReadinessViewProvider,
    private readonly agentId: string,
    private readonly hasActiveFile: () => boolean,
    initialHealth: RuntimeHealth,
    private readonly log: (message: string) => void
  ) {
    this.health = initialHealth;
    this.current = this.derive();
    this.provider.update(this.current);
  }

  snapshot(): ReadinessSnapshot {
    return this.current;
  }

  setHealth(health: RuntimeHealth): void {
    this.health = health;
    this.publish();
  }

  setGraph(graph: NonNullable<ReadinessInput["graph"]>): void {
    this.facts.graph = graph;
    this.facts.memoryError = undefined;
    this.publish();
  }

  setActionableTasks(count: number): void {
    this.facts.actionableTasks = count;
    this.facts.intentError = undefined;
    this.publish();
  }

  async refresh(): Promise<ReadinessSnapshot> {
    const facts: ReadinessFacts = { actionableTasks: 0, actionableDesigns: 0 };
    if (this.memory.isReady()) {
      try {
        facts.graph = await this.memory.callTool("graph_stats", {});
      } catch (error) {
        facts.memoryError = (error as Error).message;
        this.log(`readiness memory error: ${facts.memoryError}`);
      }
    }
    if (this.intent.isReady()) {
      try {
        const [tasks, designs] = await Promise.all([
          this.intent.callTool("board", { include_terminal: false }),
          this.intent.callTool("design_board", {}),
        ]);
        facts.actionableTasks = Array.isArray(tasks) ? tasks.length : 0;
        facts.actionableDesigns = Array.isArray(designs) ? designs.length : 0;
      } catch (error) {
        facts.intentError = (error as Error).message;
        this.log(`readiness intent error: ${facts.intentError}`);
      }
    }
    this.facts = facts;
    this.publish();
    return this.current;
  }

  private publish(): void {
    this.current = this.derive();
    this.provider.update(this.current);
  }

  private derive(): ReadinessSnapshot {
    return deriveReadiness({
      memoryConnected: this.memory.isReady(),
      intentConnected: this.intent.isReady(),
      memoryServer: this.memory.serverIdentity(),
      intentServer: this.intent.serverIdentity(),
      memoryError: this.memory.isReady() ? this.facts.memoryError : this.health.memory,
      intentError: this.intent.isReady() ? this.facts.intentError : this.health.intent,
      graph: this.facts.graph,
      actionableTasks: this.facts.actionableTasks,
      actionableDesigns: this.facts.actionableDesigns,
      hasActiveFile: this.hasActiveFile(),
      terminalHealth: this.health.terminal,
      gitHealth: this.health.git,
      agentId: this.agentId,
    });
  }
}
