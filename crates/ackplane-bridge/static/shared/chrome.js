// Shared Bridge chrome: one source of truth for the brand mark and the
// grouped, icon-labeled site nav, rendered into every static page's mount
// points. Adding, renaming, or reordering a capability means editing NAV_ITEMS
// here once -- never a per-page copy again.
(function () {
  "use strict";

  const ICONS = {
    grid: '<rect x="1.5" y="1.5" width="5.5" height="5.5" rx="1"/><rect x="9" y="1.5" width="5.5" height="5.5" rx="1"/><rect x="1.5" y="9" width="5.5" height="5.5" rx="1"/><rect x="9" y="9" width="5.5" height="5.5" rx="1"/>',
    clipboard: '<rect x="3" y="2" width="10" height="12" rx="1.4"/><path d="M6 1.5h4v2H6z"/><path d="M5.5 7.5l1.4 1.4L9.5 6"/>',
    shield: '<path d="M8 1.5l5.5 2v4.2c0 3.4-2.3 5.9-5.5 6.8-3.2-.9-5.5-3.4-5.5-6.8V3.5z"/><path d="M5.6 8l1.7 1.7L10.6 6"/>',
    key: '<circle cx="5" cy="5" r="3"/><path d="M7.2 7.2L14 14M11 11l1.4 1.4M13 9l1.4 1.4"/>',
    pulse: '<path d="M1 8h3l1.5-4.5L8 12.5 9.5 5 11 8h4"/>',
    graph: '<path d="M4.4 5.3l3.1 6M11.5 4.6L8.8 11M5.2 3.7l5.4-.5"/><circle cx="3.2" cy="3.9" r="1.9"/><circle cx="12.6" cy="3.2" r="1.9"/><circle cx="8" cy="12.6" r="1.9"/>',
  };

  // The complete, canonical capability set (ADR-0105 decision 5). `id` is the
  // page's static-file stem, so it lines up 1:1 with what each page declares
  // in its own `data-current` attribute. A `null` href is a disabled
  // placeholder: a capability with no page behind it yet.
  const NAV_ITEMS = [
    { id: "index", label: "Fleet", href: "/", icon: "grid" },
    { id: "graph", label: "Graph", href: "/graph", icon: "graph" },
    {
      id: "work-group",
      label: "Work",
      icon: "clipboard",
      items: [
        { id: "agents", label: "Agents", href: "/agents" },
        { id: "work", label: "Work", href: "/work" },
        { id: "board-doctor", label: "Board Doctor", href: "/board-doctor" },
      ],
    },
    {
      id: "evidence-group",
      label: "Evidence",
      icon: "shield",
      items: [
        { id: "evidence", label: "Evidence", href: "/evidence" },
        { id: "telemetry", label: "Telemetry", href: "/telemetry" },
        { id: "context", label: "Context", href: "/context" },
        { id: "knowledge", label: "Knowledge", href: "/knowledge" },
      ],
    },
    {
      id: "authority-group",
      label: "Authority",
      icon: "key",
      items: [
        { id: "supervisors", label: "Supervisors", href: "/supervisors" },
        { id: "delegations", label: "Delegations", href: "/delegations" },
        { id: "decisions", label: "Decisions", href: "/decisions" },
        { id: "administration", label: "Administration", href: "/administration" },
        { id: "design", label: "Design", href: "/design" },
        { id: "constitution", label: "Constitution", href: "/constitution" },
      ],
    },
    { id: "live-feed", label: "Live Feed", href: "/live", icon: "pulse" },
  ];

  function icon(name) {
    return `<svg class="nav-icon" viewBox="0 0 16 16" aria-hidden="true">${ICONS[name] || ""}</svg>`;
  }

  function escapeHtml(value) {
    return String(value ?? "").replace(
      /[&<>"']/g,
      (character) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[character],
    );
  }

  function renderLink(item, current) {
    const label = escapeHtml(item.label);
    if (!item.href) {
      return `<a aria-disabled="true">${label}</a>`;
    }
    const currentAttr = item.id === current ? ' aria-current="page"' : "";
    const iconMarkup = item.icon ? icon(item.icon) : "";
    return `<a href="${item.href}"${currentAttr}>${iconMarkup}${label}</a>`;
  }

  function renderGroup(group, current) {
    const links = group.items.map((item) => renderLink(item, current)).join("");
    return `<details class="nav-group"><summary>${icon(group.icon)}${escapeHtml(group.label)}</summary><div class="nav-menu">${links}</div></details>`;
  }

  function renderNav(mount) {
    const current = mount.dataset.current || "";
    mount.innerHTML = NAV_ITEMS.map((item) => (item.items ? renderGroup(item, current) : renderLink(item, current))).join("");
  }

  function wireDisclosure(mount) {
    const groups = () => mount.querySelectorAll(".nav-group");
    groups().forEach((group) => {
      if (group.querySelector('[aria-current="page"]')) group.open = true;
      group.addEventListener("toggle", () => {
        if (!group.open) return;
        groups().forEach((other) => {
          if (other !== group) other.open = false;
        });
      });
    });
    document.addEventListener("click", (event) => {
      groups().forEach((group) => {
        if (group.open && !group.contains(event.target)) group.open = false;
      });
    });
    document.addEventListener("keydown", (event) => {
      if (event.key === "Escape") groups().forEach((group) => (group.open = false));
    });
  }

  function renderBrand(mount) {
    const subtitle = mount.dataset.subtitle || "";
    const subtitleMarkup = subtitle ? `<span class="brand-subtitle">${escapeHtml(subtitle)}</span>` : "";
    mount.innerHTML = `<img class="mark" src="/static/shared/mark.png" alt="" width="34" height="34"><span class="brand-copy"><span class="brand-name">MindLeak Bridge</span>${subtitleMarkup}</span>`;
  }

  function init() {
    document.querySelectorAll("[data-bridge-brand]").forEach(renderBrand);
    document.querySelectorAll("[data-bridge-nav]").forEach((mount) => {
      renderNav(mount);
      wireDisclosure(mount);
    });
  }

  if (document.readyState === "loading") {
    document.addEventListener("DOMContentLoaded", init);
  } else {
    init();
  }
})();
