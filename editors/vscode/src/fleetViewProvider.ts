import * as vscode from "vscode";

import { FleetDashboard, FleetVerb } from "./fleet";

export interface FleetHandlers {
  onReady(): void;
  onRefresh(): void;
  onAct(verb: FleetVerb, taskId: string, agentId: string): void;
  onOpenTask(taskId: string): void;
}

/**
 * Renders the live agent fleet in a sidebar webview: who is working, on what
 * branch, holding which claim, and how long that claim has left. All shaping is
 * the pure {@link FleetDashboard}; this class stays a transport between the
 * extension and the webview.
 */
export class FleetViewProvider implements vscode.WebviewViewProvider {
  public static readonly viewType = "mindleak.fleetView";
  private view?: vscode.WebviewView;
  private last?: FleetDashboard;

  constructor(
    private readonly extensionUri: vscode.Uri,
    private readonly handlers: FleetHandlers
  ) {}

  resolveWebviewView(webviewView: vscode.WebviewView): void {
    this.view = webviewView;
    webviewView.webview.options = {
      enableScripts: true,
      localResourceRoots: [vscode.Uri.joinPath(this.extensionUri, "media")],
    };
    webviewView.webview.html = this.getHtml(webviewView.webview);

    webviewView.webview.onDidReceiveMessage((message) => {
      switch (message?.type) {
        case "ready":
          if (this.last) {
            this.update(this.last);
          }
          this.handlers.onReady();
          break;
        case "refresh":
          this.handlers.onRefresh();
          break;
        case "act":
          if (
            typeof message.verb === "string" &&
            typeof message.taskId === "string" &&
            typeof message.agentId === "string"
          ) {
            this.handlers.onAct(message.verb as FleetVerb, message.taskId, message.agentId);
          }
          break;
        case "openTask":
          if (typeof message.taskId === "string") {
            this.handlers.onOpenTask(message.taskId);
          }
          break;
      }
    });
  }

  /** Whether the pane is currently visible (drives polling). */
  isVisible(): boolean {
    return this.view?.visible ?? false;
  }

  /** Push a fresh readout to the webview. */
  update(dashboard: FleetDashboard): void {
    this.last = dashboard;
    this.view?.webview.postMessage({ type: "fleet", dashboard });
  }

  private getHtml(webview: vscode.Webview): string {
    const nonce = getNonce();
    const scriptUri = webview.asWebviewUri(
      vscode.Uri.joinPath(this.extensionUri, "media", "fleet.js")
    );
    const csp = [
      `default-src 'none'`,
      `style-src ${webview.cspSource} 'unsafe-inline'`,
      `script-src 'nonce-${nonce}'`,
    ].join("; ");

    return `<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="UTF-8" />
  <meta http-equiv="Content-Security-Policy" content="${csp}" />
  <meta name="viewport" content="width=device-width, initial-scale=1.0" />
  <title>MindLeak Fleet</title>
  <style>
    body {
      margin: 0;
      font-family: var(--vscode-font-family);
      font-size: var(--vscode-font-size);
      color: var(--vscode-foreground);
    }
    #toolbar {
      display: flex;
      align-items: center;
      gap: 8px;
      padding: 6px 8px;
      border-bottom: 1px solid var(--vscode-panel-border);
      position: sticky;
      top: 0;
      background: var(--vscode-sideBar-background, inherit);
      z-index: 2;
    }
    button {
      background: var(--vscode-button-secondaryBackground, #333);
      color: var(--vscode-button-secondaryForeground, #eee);
      border: none;
      border-radius: 3px;
      padding: 3px 8px;
      cursor: pointer;
      font-size: 11px;
    }
    button:hover { background: var(--vscode-button-hoverBackground, #444); }
    #generated { margin-left: auto; font-size: 10px; opacity: 0.6; }
    #notice {
      margin: 8px;
      padding: 6px 8px;
      border-radius: 4px;
      font-size: 11px;
      background: var(--vscode-inputValidation-warningBackground, rgba(210, 153, 34, 0.15));
      border: 1px solid var(--vscode-inputValidation-warningBorder, #d29922);
    }
    .cards {
      display: grid;
      grid-template-columns: repeat(auto-fit, minmax(70px, 1fr));
      gap: 6px;
      padding: 8px;
    }
    .card {
      background: var(--vscode-editorWidget-background, rgba(127, 127, 127, 0.08));
      border: 1px solid var(--vscode-panel-border);
      border-radius: 4px;
      padding: 6px 8px;
    }
    .card .value { font-size: 18px; font-weight: 600; }
    .card .label {
      font-size: 10px; opacity: 0.7;
      text-transform: uppercase; letter-spacing: 0.03em;
    }
    .value.warn { color: var(--vscode-list-warningForeground, #d29922); }
    .value.bad { color: var(--vscode-testing-iconFailed, #e5534b); }
    .value.good { color: var(--vscode-testing-iconPassed, #3fbf6f); }
    h3 {
      margin: 10px 8px 4px;
      font-size: 11px; text-transform: uppercase;
      letter-spacing: 0.03em; opacity: 0.7;
    }
    .agent {
      margin: 6px 8px;
      border: 1px solid var(--vscode-panel-border);
      border-left-width: 3px;
      border-radius: 4px;
      padding: 6px 8px;
      background: var(--vscode-editorWidget-background, rgba(127, 127, 127, 0.05));
    }
    .agent.self { border-left-color: var(--vscode-charts-blue, #3794ff); }
    .agent.lapsed { border-left-color: var(--vscode-testing-iconFailed, #e5534b); }
    .agent.holding { border-left-color: var(--vscode-testing-iconPassed, #3fbf6f); }
    .agent.idle { border-left-color: var(--vscode-panel-border); opacity: 0.75; }
    .agent header {
      display: flex; align-items: baseline; gap: 6px; flex-wrap: wrap;
    }
    .id { font-family: var(--vscode-editor-font-family, monospace); font-size: 11px; }
    .badge {
      font-size: 9px; text-transform: uppercase; letter-spacing: 0.04em;
      padding: 1px 5px; border-radius: 8px;
      background: var(--vscode-badge-background, #4d4d4d);
      color: var(--vscode-badge-foreground, #fff);
    }
    .badge.self { background: var(--vscode-charts-blue, #3794ff); color: #fff; }
    .badge.dirty { background: var(--vscode-list-warningForeground, #d29922); color: #000; }
    .meta { margin-top: 3px; font-size: 10px; opacity: 0.75; }
    .meta .unknown { opacity: 0.55; font-style: italic; }
    .task {
      margin-top: 5px; padding-top: 5px;
      border-top: 1px dashed var(--vscode-panel-border);
    }
    .task .title { font-size: 11px; cursor: pointer; }
    .task .title:hover { text-decoration: underline; }
    .lease { font-size: 10px; margin-top: 2px; }
    .lease.live { color: var(--vscode-testing-iconPassed, #3fbf6f); }
    .lease.expired { color: var(--vscode-testing-iconFailed, #e5534b); }
    .bar {
      height: 3px; border-radius: 2px; margin-top: 3px;
      background: var(--vscode-panel-border); overflow: hidden;
    }
    .bar > span { display: block; height: 100%; background: var(--vscode-testing-iconPassed, #3fbf6f); }
    .bar.expired > span { background: var(--vscode-testing-iconFailed, #e5534b); }
    .verbs { margin-top: 4px; display: flex; gap: 4px; flex-wrap: wrap; }
    .verbs button { font-size: 10px; padding: 2px 6px; }
    .finding {
      margin: 4px 8px; font-size: 11px;
      padding: 4px 6px; border-radius: 3px;
      background: var(--vscode-textCodeBlock-background, rgba(127, 127, 127, 0.08));
    }
    .finding .why { opacity: 0.7; font-size: 10px; }
    .muted { padding: 8px; opacity: 0.6; font-size: 11px; }
  </style>
</head>
<body>
  <div id="toolbar">
    <button id="refresh" title="Refresh now">Refresh</button>
    <span id="generated"></span>
  </div>
  <div id="notice" style="display:none"></div>
  <div id="cards" class="cards"></div>
  <div id="agents"></div>
  <div id="health"></div>
  <script nonce="${nonce}" src="${scriptUri}"></script>
</body>
</html>`;
  }
}

function getNonce(): string {
  const chars = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";
  let text = "";
  for (let i = 0; i < 32; i++) {
    text += chars.charAt(Math.floor(Math.random() * chars.length));
  }
  return text;
}
