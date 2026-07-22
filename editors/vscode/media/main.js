// MindLeak context-graph renderer (Cytoscape.js inside the VS Code webview).
(function () {
  const vscode = acquireVsCodeApi();
  const statusEl = document.getElementById("status");

  const colors = {
    artifact: "#4f9cf9",
    symbol: "#f5a742",
    intent: "#3fbf6f",
    execution: "#e5534b",
  };

  let cy;

  function initCytoscape() {
    cy = cytoscape({
      container: document.getElementById("cy"),
      elements: [],
      style: [
        {
          selector: "node",
          style: {
            "background-color": (n) => colors[n.data("type")] || "#888",
            label: "data(label)",
            color: "#ddd",
            "font-size": 8,
            "text-wrap": "ellipsis",
            "text-max-width": 90,
            "text-valign": "bottom",
            "text-margin-y": 2,
            width: 16,
            height: 16,
          },
        },
        {
          selector: "edge",
          style: {
            width: (e) => 0.5 + 3 * (e.data("effective") || 0.2),
            "line-color": "#5a5a5a",
            "target-arrow-color": "#5a5a5a",
            "target-arrow-shape": "triangle",
            "curve-style": "bezier",
            label: "data(relation)",
            "font-size": 6,
            color: "#999",
            "text-rotation": "autorotate",
          },
        },
        {
          selector: "node.seed",
          style: { "border-width": 2, "border-color": "#fff", width: 22, height: 22 },
        },
      ],
      layout: { name: "cose", animate: false, padding: 10 },
    });
  }

  function render(subgraph, meta) {
    if (!cy) {
      initCytoscape();
    }
    const nodes = (subgraph.nodes || []).map((n) => ({
      data: {
        id: n.id,
        label: n.label,
        type: n.type,
        depth: n.depth,
      },
      classes: n.depth === 0 ? "seed" : "",
    }));
    const ids = new Set(nodes.map((n) => n.data.id));
    const edges = (subgraph.edges || [])
      .filter((e) => ids.has(e.source_id) && ids.has(e.target_id))
      .map((e) => ({
        data: {
          id: `${e.source_id}__${e.relation}__${e.target_id}`,
          source: e.source_id,
          target: e.target_id,
          relation: e.relation,
          effective: e.effective,
        },
      }));

    cy.elements().remove();
    cy.add(nodes);
    cy.add(edges);
    cy.layout({ name: "cose", animate: false, padding: 10 }).run();

    const n = meta && typeof meta.nodes === "number" ? meta.nodes : nodes.length;
    const ae = meta && typeof meta.active_edges === "number" ? meta.active_edges : edges.length;
    statusEl.textContent = `${n} nodes · ${ae} active edges`;
  }

  window.addEventListener("message", (event) => {
    const msg = event.data;
    if (msg.type === "graph") {
      render(msg.subgraph || {}, msg.meta);
    } else if (msg.type === "status") {
      statusEl.textContent = msg.text;
    }
  });

  document
    .getElementById("refresh")
    .addEventListener("click", () => vscode.postMessage({ type: "refresh" }));
  document
    .getElementById("prune")
    .addEventListener("click", () => vscode.postMessage({ type: "prune" }));
  document
    .getElementById("export")
    .addEventListener("click", () => vscode.postMessage({ type: "export" }));

  vscode.postMessage({ type: "ready" });
})();
