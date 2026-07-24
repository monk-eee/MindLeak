import * as vscode from "vscode";

import { TelemetryDashboard } from "./util";

export interface TelemetryHandlers {
  onReady(): void;
  onRefresh(): void;
  onToggleLive(live: boolean): void;
}

/**
 * Renders the MindLeak real-time effectiveness readout in a sidebar webview:
 * graph size, success/error rates, latency, per-tool metrics, and an opt-in
 * live event log. All numbers are the pure {@link TelemetryDashboard}; this
 * class stays a thin transport between the extension and the webview.
 */
export class TelemetryViewProvider implements vscode.WebviewViewProvider {
  public static readonly viewType = "mindleak.telemetryView";
  private view?: vscode.WebviewView;
  private live = false;

  constructor(
    private readonly extensionUri: vscode.Uri,
    private readonly handlers: TelemetryHandlers
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
          this.handlers.onReady();
          break;
        case "refresh":
          this.handlers.onRefresh();
          break;
        case "toggleLive":
          this.live = Boolean(message.live);
          this.handlers.onToggleLive(this.live);
          break;
      }
    });
  }

  /** Whether the pane is currently visible (drives polling). */
  isVisible(): boolean {
    return this.view?.visible ?? false;
  }

  /** Whether the user has opted into the live event log. */
  isLive(): boolean {
    return this.live;
  }

  /** Push a fresh dashboard and (when live) log lines to the webview. */
  update(dashboard: TelemetryDashboard, logLines: string[], live: boolean): void {
    this.view?.webview.postMessage({ type: "telemetry", dashboard, logLines, live });
  }

  private getHtml(webview: vscode.Webview): string {
    const nonce = getNonce();
    const scriptUri = webview.asWebviewUri(
      vscode.Uri.joinPath(this.extensionUri, "media", "telemetry.js")
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
  <title>MindLeak Telemetry</title>
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
    }
    #toolbar button {
      background: var(--vscode-button-secondaryBackground, #333);
      color: var(--vscode-button-secondaryForeground, #eee);
      border: none;
      border-radius: 3px;
      padding: 3px 8px;
      cursor: pointer;
      font-size: 11px;
    }
    #toolbar button:hover {
      background: var(--vscode-button-hoverBackground, #444);
    }
    #toolbar label {
      margin-left: auto;
      display: flex;
      align-items: center;
      gap: 4px;
      font-size: 11px;
      opacity: 0.85;
    }
    .cards {
      display: grid;
      grid-template-columns: repeat(auto-fit, minmax(90px, 1fr));
      gap: 6px;
      padding: 8px;
    }
    .card {
      background: var(--vscode-editorWidget-background, rgba(127, 127, 127, 0.08));
      border: 1px solid var(--vscode-panel-border);
      border-radius: 4px;
      padding: 6px 8px;
    }
    .card .value {
      font-size: 18px;
      font-weight: 600;
    }
    .card .label {
      font-size: 10px;
      opacity: 0.7;
      text-transform: uppercase;
      letter-spacing: 0.03em;
    }
    .value.good {
      color: var(--vscode-testing-iconPassed, #3fbf6f);
    }
    .value.bad {
      color: var(--vscode-testing-iconFailed, #e5534b);
    }
    h3 {
      margin: 10px 8px 4px;
      font-size: 11px;
      text-transform: uppercase;
      letter-spacing: 0.03em;
      opacity: 0.7;
    }
    table {
      width: 100%;
      border-collapse: collapse;
      font-size: 11px;
    }
    th, td {
      text-align: right;
      padding: 2px 8px;
    }
    th:first-child, td:first-child {
      text-align: left;
    }
    thead th {
      opacity: 0.6;
      font-weight: 500;
      border-bottom: 1px solid var(--vscode-panel-border);
    }
    td.err {
      color: var(--vscode-testing-iconFailed, #e5534b);
    }
    td.ok {
      color: var(--vscode-testing-iconPassed, #3fbf6f);
    }
    #log {
      margin: 4px 8px 12px;
      padding: 6px;
      max-height: 220px;
      overflow-y: auto;
      font-family: var(--vscode-editor-font-family, monospace);
      font-size: 11px;
      white-space: pre;
      background: var(--vscode-textCodeBlock-background, rgba(127, 127, 127, 0.08));
      border-radius: 4px;
    }
    #log .line.error {
      color: var(--vscode-testing-iconFailed, #e5534b);
    }
    .muted {
      padding: 8px;
      opacity: 0.6;
      font-size: 11px;
    }
  </style>
</head>
<body>
  <div id="toolbar">
    <button id="refresh" title="Refresh now">Refresh</button>
    <label title="Stream recent tool calls in real time (off by default)">
      <input type="checkbox" id="live" /> Live log
    </label>
  </div>
  <div id="cards" class="cards"></div>
  <h3>Tools</h3>
  <table id="tools">
    <thead>
      <tr><th>Tool</th><th>Calls</th><th>Lifetime err%</th><th>Health</th><th>Avg ms</th></tr>
    </thead>
    <tbody></tbody>
  </table>
  <div id="logSection" style="display:none">
    <h3>Live log</h3>
    <div id="log"></div>
  </div>
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
