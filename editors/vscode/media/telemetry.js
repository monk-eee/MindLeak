// MindLeak telemetry & effectiveness renderer (inside the VS Code webview).
(function () {
  const vscode = acquireVsCodeApi();

  const cardsEl = document.getElementById("cards");
  const toolsBody = document.querySelector("#tools tbody");
  const logSection = document.getElementById("logSection");
  const logEl = document.getElementById("log");
  const liveEl = document.getElementById("live");

  document.getElementById("refresh").addEventListener("click", () => {
    vscode.postMessage({ type: "refresh" });
  });
  liveEl.addEventListener("change", () => {
    vscode.postMessage({ type: "toggleLive", live: liveEl.checked });
  });

  function card(value, label, tone) {
    const el = document.createElement("div");
    el.className = "card";
    const v = document.createElement("div");
    v.className = "value" + (tone ? " " + tone : "");
    v.textContent = value;
    const l = document.createElement("div");
    l.className = "label";
    l.textContent = label;
    el.append(v, l);
    return el;
  }

  function renderCards(d) {
    cardsEl.replaceChildren(
      card(String(d.nodes), "Nodes"),
      card(String(d.activeEdges), "Active edges"),
      card(d.successRatePct + "%", "Success", d.successRatePct >= 95 ? "good" : "bad"),
      card(d.errorRatePct + "%", "Errors", d.totalErrors > 0 ? "bad" : "good"),
      card(d.avgLatencyMs + " ms", "Avg latency"),
      card(String(d.totalEvents), "Events")
    );
  }

  function renderTools(tools) {
    toolsBody.replaceChildren();
    if (!tools.length) {
      const row = document.createElement("tr");
      const cell = document.createElement("td");
      cell.colSpan = 4;
      cell.className = "muted";
      cell.textContent = "No tool calls recorded yet.";
      row.append(cell);
      toolsBody.append(row);
      return;
    }
    for (const tool of tools) {
      const row = document.createElement("tr");
      const cells = [
        [tool.name, ""],
        [String(tool.calls), ""],
        [tool.errorRatePct + "%", tool.errors > 0 ? "err" : ""],
        [String(tool.avgMs), ""],
      ];
      for (const [text, cls] of cells) {
        const td = document.createElement("td");
        if (cls) {
          td.className = cls;
        }
        td.textContent = text;
        row.append(td);
      }
      toolsBody.append(row);
    }
  }

  function renderLog(lines, live) {
    logSection.style.display = live ? "" : "none";
    if (!live) {
      return;
    }
    logEl.replaceChildren();
    if (!lines.length) {
      const empty = document.createElement("div");
      empty.className = "line";
      empty.textContent = "Waiting for events…";
      logEl.append(empty);
      return;
    }
    for (const line of lines) {
      const el = document.createElement("div");
      el.className = "line" + (/ error /.test(" " + line + " ") ? " error" : "");
      el.textContent = line;
      logEl.append(el);
    }
  }

  window.addEventListener("message", (event) => {
    const message = event.data;
    if (message?.type !== "telemetry") {
      return;
    }
    if (typeof message.live === "boolean") {
      liveEl.checked = message.live;
    }
    renderCards(message.dashboard);
    renderTools(message.dashboard.tools || []);
    renderLog(message.logLines || [], Boolean(message.live));
  });

  vscode.postMessage({ type: "ready" });
})();
