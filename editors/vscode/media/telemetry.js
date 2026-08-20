// MindLeak telemetry & effectiveness renderer (inside the VS Code webview).
(function () {
  const vscode = acquireVsCodeApi();

  const cardsEl = document.getElementById("cards");
  const toolsBody = document.querySelector("#tools tbody");
  const actionsSection = document.getElementById("actionsSection");
  const actionsEl = document.getElementById("actions");
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
      card(d.successRatePct + "%", "Lifetime success", d.successRatePct >= 95 ? "good" : "bad"),
      card(String(d.failingTools), "Failing now", d.failingTools > 0 ? "bad" : "good"),
      card(String(d.degradedTools), "Degraded now", d.degradedTools > 0 ? "warn" : "good"),
      card(String(d.totalErrors), "Lifetime errors"),
      card(d.avgLatencyMs + " ms", "Avg latency"),
      card(String(d.backgroundReadCalls), "Background reads"),
      card(String(d.preflightReadCalls), "Preflight reads"),
      card(
        String(d.memoryPreflightMisses),
        "Skipped preflight (sample)",
        d.memoryPreflightMisses > 0 ? "bad" : "good"
      )
    );
  }

  function renderActions(recommendations) {
    actionsSection.style.display = recommendations.length ? "" : "none";
    actionsEl.replaceChildren();
    for (const recommendation of recommendations) {
      const item = document.createElement("li");
      item.textContent = recommendation;
      actionsEl.append(item);
    }
  }

  function renderTools(tools) {
    toolsBody.replaceChildren();
    if (!tools.length) {
      const row = document.createElement("tr");
      const cell = document.createElement("td");
      cell.colSpan = 5;
      cell.className = "muted";
      cell.textContent = "No tool calls recorded yet.";
      row.append(cell);
      toolsBody.append(row);
      return;
    }
    for (const tool of tools) {
      const row = document.createElement("tr");
      const health = tool.currentlyFailing
        ? ["failing", "err"]
        : tool.currentlyDegraded
          ? ["degraded", "warn"]
          : ["ok", "ok"];
      const cells = [
        [tool.name, ""],
        [String(tool.calls), ""],
        [tool.errorRatePct + "%", ""],
        health,
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
    renderActions(message.dashboard.recommendations || []);
    renderLog(message.logLines || [], Boolean(message.live));
  });

  vscode.postMessage({ type: "ready" });
})();
