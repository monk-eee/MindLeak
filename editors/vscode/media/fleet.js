// @ts-nocheck
/* eslint-env browser */
/* global acquireVsCodeApi */
(function () {
  const vscode = acquireVsCodeApi();
  const cards = document.getElementById("cards");
  const agents = document.getElementById("agents");
  const health = document.getElementById("health");
  const notice = document.getElementById("notice");
  const generated = document.getElementById("generated");

  document.getElementById("refresh").addEventListener("click", () => {
    vscode.postMessage({ type: "refresh" });
  });

  /** The lease bar is only meaningful against a span; assume an hour. */
  const LEASE_SCALE_SECONDS = 3600;

  function el(tag, className, text) {
    const node = document.createElement(tag);
    if (className) {
      node.className = className;
    }
    if (text !== undefined && text !== null) {
      node.textContent = String(text);
    }
    return node;
  }

  function duration(seconds) {
    if (seconds === null || seconds === undefined || !isFinite(seconds)) {
      return "unknown";
    }
    const n = Math.abs(Math.floor(seconds));
    if (n < 60) return n + "s";
    if (n < 3600) return Math.floor(n / 60) + "m";
    if (n < 86400) return Math.floor(n / 3600) + "h";
    return Math.floor(n / 86400) + "d";
  }

  function card(label, value, tone) {
    const node = el("div", "card");
    node.appendChild(el("div", "value" + (tone ? " " + tone : ""), value));
    node.appendChild(el("div", "label", label));
    return node;
  }

  function renderCards(dashboard) {
    cards.textContent = "";
    const rows = dashboard.rows || [];
    const holding = rows.filter((r) => r.state === "holding").length;
    const lapsed = rows.filter((r) => r.state === "lapsed").length;
    cards.appendChild(card("Agents", rows.length));
    cards.appendChild(card("Holding", holding, holding > 0 ? "good" : undefined));
    cards.appendChild(card("Lapsed", lapsed, lapsed > 0 ? "bad" : undefined));
    cards.appendChild(
      card(
        "Stalled",
        (dashboard.stalled || []).length,
        (dashboard.stalled || []).length > 0 ? "warn" : undefined
      )
    );
    cards.appendChild(
      card(
        "Ailments",
        (dashboard.ailments || []).length,
        (dashboard.ailments || []).length > 0 ? "warn" : undefined
      )
    );
  }

  function renderMeta(row, now) {
    const meta = el("div", "meta");
    if (row.branch) {
      meta.appendChild(el("span", null, row.branch));
    } else {
      meta.appendChild(el("span", "unknown", "branch undeclared"));
    }
    const bits = [];
    if (row.head) bits.push(row.head.slice(0, 8));
    if (typeof row.behind === "number" && row.behind > 0) bits.push(row.behind + " behind");
    if (row.lastActive) bits.push("active " + duration(now - row.lastActive) + " ago");
    if (typeof row.observations === "number") bits.push(row.observations + " obs");
    if (bits.length > 0) {
      meta.appendChild(el("span", null, "  ·  " + bits.join("  ·  ")));
    }
    return meta;
  }

  function renderTask(row, task) {
    const node = el("div", "task");
    const title = el("div", "title", task.title);
    title.title = task.id;
    title.addEventListener("click", () => {
      vscode.postMessage({ type: "openTask", taskId: task.id });
    });
    node.appendChild(title);

    const expired = task.lease === "expired";
    const seconds = task.leaseSeconds;
    const label =
      seconds === null || seconds === undefined
        ? task.status + " · no lease"
        : expired
          ? task.status + " · lapsed " + duration(seconds) + " ago"
          : task.status + " · " + duration(seconds) + " left";
    node.appendChild(el("div", "lease " + (expired ? "expired" : "live"), label));

    if (typeof seconds === "number") {
      const bar = el("div", "bar" + (expired ? " expired" : ""));
      const fill = el("span");
      const pct = expired
        ? 100
        : Math.max(2, Math.min(100, Math.round((seconds / LEASE_SCALE_SECONDS) * 100)));
      fill.style.width = pct + "%";
      bar.appendChild(fill);
      node.appendChild(bar);
    }

    if (task.verbs && task.verbs.length > 0) {
      const verbs = el("div", "verbs");
      for (const verb of task.verbs) {
        const button = el("button", null, verb);
        button.addEventListener("click", () => {
          vscode.postMessage({
            type: "act",
            verb: verb,
            taskId: task.id,
            agentId: row.agentId,
          });
        });
        verbs.appendChild(button);
      }
      node.appendChild(verbs);
    }
    return node;
  }

  function renderAgents(dashboard) {
    agents.textContent = "";
    const rows = dashboard.rows || [];
    if (rows.length === 0) {
      agents.appendChild(
        el("div", "muted", dashboard.planes && dashboard.planes.intent ? "No live agents." : "")
      );
      return;
    }
    const now = dashboard.generatedAt;
    for (const row of rows) {
      const node = el("div", "agent " + row.state + (row.isSelf ? " self" : ""));
      const header = el("header");
      header.appendChild(el("span", "id", row.short));
      if (row.isSelf) {
        header.appendChild(el("span", "badge self", "this session"));
      }
      if (row.dirty === true) {
        header.appendChild(el("span", "badge dirty", "dirty"));
      }
      if (row.state === "lapsed") {
        header.appendChild(el("span", "badge", "lapsed"));
      }
      node.appendChild(header);
      node.appendChild(renderMeta(row, now));
      for (const task of row.tasks || []) {
        node.appendChild(renderTask(row, task));
      }
      agents.appendChild(node);
    }
    if (dashboard.hidden > 0) {
      agents.appendChild(el("div", "muted", dashboard.hidden + " idle session(s) hidden."));
    }
  }

  function renderHealth(dashboard) {
    health.textContent = "";
    const stalled = dashboard.stalled || [];
    const ailments = dashboard.ailments || [];
    if (stalled.length > 0) {
      health.appendChild(el("h3", null, "Stalled"));
      for (const entry of stalled) {
        const node = el("div", "finding");
        node.appendChild(el("div", null, entry.title || entry.task_id));
        node.appendChild(
          el("div", "why", (entry.kind || "stalled") + " · " + duration(entry.stalled_seconds))
        );
        health.appendChild(node);
      }
    }
    if (ailments.length > 0) {
      health.appendChild(el("h3", null, "Board health"));
      for (const finding of ailments) {
        const node = el("div", "finding");
        node.appendChild(el("div", null, finding.ailment || "finding"));
        if (finding.remedy) {
          node.appendChild(el("div", "why", finding.remedy));
        }
        health.appendChild(node);
      }
    }
  }

  window.addEventListener("message", (event) => {
    const message = event.data;
    if (!message || message.type !== "fleet") {
      return;
    }
    const dashboard = message.dashboard || {};
    if (dashboard.notice) {
      notice.textContent = dashboard.notice;
      notice.style.display = "block";
    } else {
      notice.style.display = "none";
    }
    generated.textContent = dashboard.generatedAt
      ? new Date(dashboard.generatedAt * 1000).toLocaleTimeString()
      : "";
    renderCards(dashboard);
    renderAgents(dashboard);
    renderHealth(dashboard);
  });

  vscode.postMessage({ type: "ready" });
})();
