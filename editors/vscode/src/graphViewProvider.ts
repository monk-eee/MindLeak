import * as vscode from "vscode";

export interface GraphHandlers {
  onReady(): void;
  onRefresh(): void;
  onPrune(): void;
  onExport(): void;
}

/** Renders the MindLeak context graph in a sidebar webview using Cytoscape.js. */
export class GraphViewProvider implements vscode.WebviewViewProvider {
  public static readonly viewType = "mindleak.graphView";
  private view?: vscode.WebviewView;
  private statusText = "starting…";

  constructor(
    private readonly extensionUri: vscode.Uri,
    private readonly handlers: GraphHandlers
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
          this.status(this.statusText);
          this.handlers.onReady();
          break;
        case "refresh":
          this.handlers.onRefresh();
          break;
        case "prune":
          this.handlers.onPrune();
          break;
        case "export":
          this.handlers.onExport();
          break;
      }
    });
  }

  /** Push a graph snapshot to the webview. */
  update(subgraph: unknown, meta?: Record<string, unknown>): void {
    this.view?.webview.postMessage({ type: "graph", subgraph, meta });
  }

  status(text: string): void {
    this.statusText = text;
    this.view?.webview.postMessage({ type: "status", text });
  }

  private getHtml(webview: vscode.Webview): string {
    const nonce = getNonce();
    const scriptUri = webview.asWebviewUri(
      vscode.Uri.joinPath(this.extensionUri, "media", "main.js")
    );
    const styleUri = webview.asWebviewUri(
      vscode.Uri.joinPath(this.extensionUri, "media", "style.css")
    );
    const cytoscapeUri = webview.asWebviewUri(
      vscode.Uri.joinPath(this.extensionUri, "media", "vendor", "cytoscape.min.js")
    );
    const csp = [
      `default-src 'none'`,
      `img-src ${webview.cspSource}`,
      `style-src ${webview.cspSource} 'unsafe-inline'`,
      `script-src 'nonce-${nonce}'`,
    ].join("; ");

    return `<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="UTF-8" />
  <meta http-equiv="Content-Security-Policy" content="${csp}" />
  <meta name="viewport" content="width=device-width, initial-scale=1.0" />
  <link href="${styleUri}" rel="stylesheet" />
  <title>MindLeak Graph</title>
</head>
<body>
  <div id="toolbar">
    <button id="refresh" title="Refresh">Refresh</button>
    <button id="prune" title="Prune decayed edges">Prune</button>
    <button id="export" title="Export snapshot JSON">Export</button>
    <span id="status">starting…</span>
  </div>
  <div id="legend">
    <span class="dot artifact"></span>File
    <span class="dot symbol"></span>Symbol
    <span class="dot intent"></span>Intent
    <span class="dot execution"></span>Execution
  </div>
  <div id="cy"></div>
  <script nonce="${nonce}" src="${cytoscapeUri}"></script>
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
