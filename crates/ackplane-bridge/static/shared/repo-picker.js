// Shared Bridge chrome: populates every [data-repo-datalist] element with
// the tenant's enrolled repository ids, so a repository <input list="...">
// offers real suggestions instead of requiring an exact id from memory.
// The input stays a plain text field underneath -- an id outside this list
// (not yet enrolled, or beyond the bounded page count below) still submits
// normally, this is autocomplete, never a closed set of choices.
(function () {
  "use strict";

  const MAX_PAGES = 5;
  const PAGE_SIZE = 100;

  function escapeHtml(value) {
    return String(value ?? "").replace(
      /[&<>"']/g,
      (character) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[character],
    );
  }

  async function fetchRepositoryIds() {
    const ids = [];
    for (let page = 1; page <= MAX_PAGES; page += 1) {
      const params = new URLSearchParams({
        page: String(page),
        page_size: String(PAGE_SIZE),
        sort: "repository_id:asc",
      });
      const response = await fetch(`/api/v1/fleet?${params}`);
      if (!response.ok) break;
      const data = await response.json();
      const repositories = data.repositories || [];
      for (const repository of repositories) {
        if (repository.repository_id) ids.push(repository.repository_id);
      }
      if (repositories.length < PAGE_SIZE || ids.length >= data.total) break;
    }
    return ids;
  }

  function populate(datalist, ids) {
    datalist.innerHTML = ids.map((id) => `<option value="${escapeHtml(id)}"></option>`).join("");
  }

  async function init() {
    const datalists = document.querySelectorAll("[data-repo-datalist]");
    if (datalists.length === 0) return;
    try {
      const ids = await fetchRepositoryIds();
      datalists.forEach((datalist) => populate(datalist, ids));
    } catch {
      // Suggestions are a convenience; a fetch failure leaves every input a
      // plain text field, never blocking typing or submitting an id by hand.
    }
  }

  if (document.readyState === "loading") {
    document.addEventListener("DOMContentLoaded", init);
  } else {
    init();
  }
})();
