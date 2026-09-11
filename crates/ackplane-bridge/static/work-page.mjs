import { WorkCommandClient } from "./work-command-client.mjs";

const labels = { create_work: "Create task", assign: "Assign", answer_wait: "Answer questions", submit_review: "Submit review", route_work: "Route", release_lease: "Release lease", pause: "Pause", resume: "Resume", steer: "Steer", drain: "Drain" };
const targeted = new Set(["assign", "pause", "resume", "steer", "drain"]);
const definitions = {
  create_work: [["title", "Title"], ["acceptance", "Acceptance", "textarea"], ["goal_id", "Goal (optional)"]],
  answer_wait: [["wait_id", "Question", "select"], ["answer", "Answer", "textarea"]],
  submit_review: [["disposition", "Decision", "select"], ["review_rationale", "Review rationale", "textarea"]],
  route_work: [["route_reference", "Route reference"]],
  steer: [["instruction", "Instruction", "textarea"], ["checkpoint_required", "Require checkpoint", "checkbox"]],
  pause: [["checkpoint_required", "Require checkpoint", "checkbox"]],
  drain: [["deadline", "Deadline (minutes from preview)", "number"]],
};
const outcomeText = { pending_confirmation: "Ready for confirmation.", pending_delivery: "Pending delivery. Not yet applied.", accepted: "Accepted. Not yet applied.", applied: "Command applied.", conflicted: "Task changed. Close and prepare a new command.", failed: "Command failed.", expired: "Command expired.", refused: "Command refused.", authorization_unavailable: "Authorization unavailable.", command_not_found: "Command no longer available." };

export function mountWorkPage({ document: doc = globalThis.document, fetchImpl = globalThis.fetch, location = globalThis.location, history = globalThis.history, now = () => Math.floor(Date.now() / 1000), uuid = () => globalThis.crypto.randomUUID() } = {}) {
  const element = (id) => doc.getElementById(id);
  const make = (tag, text = "", className = "") => { const node = doc.createElement(tag); node.textContent = text; node.className = className; return node; };
  const on = (id, event, handler) => element(id).addEventListener(event, handler);
  const words = (value) => String(value ?? "Unknown").replaceAll("_", " ");
  const when = (seconds) => Number.isFinite(seconds) ? new Date(seconds * 1000).toLocaleString() : "Not reported";
  const api = (repository) => `/api/v1/repositories/${encodeURIComponent(repository)}`;
  const pair = (node, label, value) => node.append(make("dt", label), make("dd", String(value ?? "Not reported")));
  const taskButtons = new Map(), commandButtons = new Map();
  let repository = "", page = 1, commands = [], workers = [], detail = null, selected = "", loadToken = 0, selectionToken = 0, flow = null, workerError = "";
  const unanswered = (value) => (value?.waits || []).filter((wait) => wait.answer == null && wait.answered_at_seconds == null);
  const workerName = (worker) => `${worker.worker_id} (${words(worker.runtime)}, process ${words(worker.state)})`;
  const owner = (task) => workers.find((worker) => worker.session_id === task.owner_session_id)?.worker_id || task.owner_id || "Unassigned";
  const eligible = (kind) => workers.filter((worker) => worker.freshness === "current" && worker.supported_directives.includes(kind) && ["started", "checkpointed", "paused", "draining", "reconnected"].includes(worker.state));
  function say(message, error = false, id = "notice") { element(id).textContent = message; element(id).dataset.error = String(error); }
  async function get(url) {
    const response = await fetchImpl(url, { credentials: "same-origin", redirect: "error", headers: { Accept: "application/json" } });
    if (!response.ok) throw new Error(response.status === 404 ? "Repository or record is not visible to this tenant." : `Request unavailable (HTTP ${response.status}).`);
    return response.json();
  }
  async function getWorkers(repo) {
    const inventory = await get(`${api(repo)}/supervisors`);
    return (await Promise.all(inventory.entries.map(async (supervisor) => {
      const sessions = await get(`${api(repo)}/supervisors/${encodeURIComponent(supervisor.supervisor_id)}/sessions`);
      return sessions.entries.map((session) => ({ ...supervisor, ...session }));
    }))).flat();
  }
  function commandReason(kind) {
    const capability = commands.find((entry) => entry.operation === kind);
    if (!capability) return "Command not advertised for this repository.";
    if (capability.state !== "available_without_policy") return capability.reason || "Authorization unavailable.";
    if (kind === "create_work") return "";
    if (!detail) return "Select a task with available detail.";
    if (!Number.isSafeInteger(detail.task.version) || detail.task.version < 0) return "Task version unavailable. Reload after the server is updated.";
    if (["completed", "abandoned"].includes(detail.task.state)) return `Task is ${words(detail.task.state)}.`;
    if (kind === "answer_wait" && !unanswered(detail).length) return "No unanswered questions.";
    if (kind === "release_lease" && (!detail.task.owner_id || !Number.isFinite(detail.task.lease_expires_at_seconds))) return "No published lease to release.";
    if (targeted.has(kind) && !eligible(kind).length) return workerError || "No current agent session advertises this capability.";
    return "";
  }
  function renderCommands() {
    element("command-controls").replaceChildren(); element("secondary-controls").replaceChildren(); commandButtons.clear();
    Object.entries(labels).forEach(([kind, label], index) => {
      if (index >= 4 && !commands.some((entry) => entry.operation === kind)) return;
      const reason = commandReason(kind), slot = make("div", "", "action"), button = make("button", label), hint = make("small", reason);
      hint.id = `reason-${kind}`; hint.hidden = !reason; button.type = "button"; button.disabled = Boolean(reason); button.title = reason || label;
      button.dataset.command = kind; button.setAttribute("aria-describedby", hint.id); button.addEventListener("click", () => openCommand(kind));
      slot.append(button, hint); element(index < 4 ? "command-controls" : "secondary-controls").append(slot); commandButtons.set(kind, button);
    });
    element("more-actions").hidden = !element("secondary-controls").children.length;
  }
  function graphLink(scope) {
    const seeds = [...(scope.declared_paths || []).map((path) => `artifact:${path}`), ...(scope.declared_symbols || []).filter((symbol) => symbol.startsWith("symbol:"))].slice(0, 12);
    if (!seeds.length) return null;
    const link = make("a", "Context graph"); link.href = `/graph?${new URLSearchParams({ repository, seeds: seeds.join(","), depth: "2" })}`; return link;
  }
  function renderPublication(publication) {
    const value = publication || { state: "unknown" }, claims = value.claims_only || [];
    element("publication").hidden = false; element("publication").dataset.state = value.state;
    const title = { current: "Work publication current", claims_only: "Claims only; Work not published", not_published: "Work not published" }[value.state] || "Publication status unavailable";
    element("publication-title").textContent = `${title} (${value.claims_only_total || 0} unmatched claims)`;
    element("publication-summary").textContent = claims.length ? "Active claims without Work records. Task acceptance and lifecycle are not published for these claims." : "No unmatched claims reported.";
    element("claims-only-rows").replaceChildren();
    for (const claim of claims) {
      const row = make("li", `${claim.task_id} | Owner: ${claim.owner_id} | Branch: ${claim.branch || "Not reported"} | Lease: ${when(claim.lease_expires_at_seconds)}\nPaths: ${(claim.declared_paths || []).join(", ") || "Not declared"}\nSymbols: ${(claim.declared_symbols || []).join(", ") || "Not declared"}\n`);
      const link = graphLink(claim); if (link) row.append(link); element("claims-only-rows").append(row);
    }
    element("doctor-link").href = `${api(repository)}/work/doctor`;
  }
  function emptyRows(message) { const row = make("tr"), cell = make("td", message); cell.colSpan = 3; row.append(cell); element("rows").replaceChildren(row); }
  function renderTasks(result) {
    element("rows").replaceChildren(); taskButtons.clear();
    if (!result.items.length) emptyRows("No tasks match this filter.");
    for (const task of result.items) {
      const row = make("tr"), title = make("td"), button = make("button", task.title, "task-button"), badge = make("td", words(task.state), "badge");
      button.type = "button"; button.append(make("small", task.task_id)); button.setAttribute("aria-pressed", String(task.task_id === selected));
      button.addEventListener("click", () => selectTask(task.task_id)); taskButtons.set(task.task_id, button); title.append(button); badge.dataset.state = task.state;
      row.append(title, badge, make("td", owner(task))); element("rows").append(row);
    }
    const end = Math.min(result.page * result.page_size, result.total), start = result.total ? (result.page - 1) * result.page_size + 1 : 0;
    element("pager-summary").textContent = `${start}-${end} of ${result.total}`; element("prev-page").disabled = result.page <= 1; element("next-page").disabled = end >= result.total;
  }
  function list(id, entries, empty) { element(id).replaceChildren(...(entries.length ? entries : [empty]).map((text) => make("li", text))); }
  function renderDetail() {
    const task = detail.task; element("task-title").textContent = task.title; element("detail-content").hidden = false;
    element("task-summary").textContent = `${words(task.state)} | ${owner(task)}`;
    element("task-meta").replaceChildren();
    for (const [label, value] of [["Task", task.task_id], ["State", words(task.state)], ["Owner", owner(task)], ["Session", task.owner_session_id], ["Lease", when(task.lease_expires_at_seconds)], ["Goal", task.goal_id], ["Version", task.version], ["Updated", when(task.updated_at_seconds)]]) pair(element("task-meta"), label, value);
    element("acceptance").textContent = detail.acceptance || "Not published";
    list("task-scope", [...(task.declared_paths || []).map((path) => `Path: ${path}`), ...(task.declared_symbols || []).map((symbol) => `Symbol: ${symbol}`)], "No scope declared.");
    element("scope-link").replaceChildren(); const scopeLink = graphLink(task); if (scopeLink) element("scope-link").append(scopeLink);
    list("task-waits", detail.waits.map((wait) => `${wait.question}\nAsked by ${wait.asked_by} at ${when(wait.asked_at_seconds)}\n${wait.answer == null ? `Awaiting ${wait.audience || "human response"}` : `${wait.answer}\nAnswered by ${wait.answered_by} at ${when(wait.answered_at_seconds)}`}`), "No questions.");
    list("task-history", detail.history.map((event) => `${words(event.from_state || "created")} -> ${words(event.to_state)} | ${event.actor_id} | ${when(event.recorded_at_seconds)}`), "No history published.");
    const evidence = make("a", "Evidence and review"), conformance = make("a", "Conformance records (JSON)");
    evidence.href = `/evidence?${new URLSearchParams({ repository, task: task.task_id })}`; conformance.href = `${api(repository)}/tasks/${encodeURIComponent(task.task_id)}/conformance`;
    element("record-links").replaceChildren(evidence, conformance); renderCommands();
  }
  function invalidateCommand() { flow = null; if (element("command-dialog").open) element("command-dialog").close(); }
  function clear() {
    ++loadToken; ++selectionToken; invalidateCommand(); repository = ""; selected = ""; detail = null; commands = []; workers = []; workerError = "";
    taskButtons.clear(); element("task-detail").hidden = true; element("publication").hidden = true; element("worker-status").textContent = "";
    element("prev-page").disabled = true; element("next-page").disabled = true; element("pager-summary").textContent = ""; element("load-work").disabled = false;
    emptyRows("Select a repository."); renderCommands(); say("");
  }
  async function load(nextPage = 1, preferredTask = "") {
    clear(); repository = element("repository-id").value.trim(); page = nextPage;
    if (!repository) { element("repository-id").focus(); say("Select a repository.", true); return; }
    const token = loadToken, repo = repository, query = new URLSearchParams({ page: String(page), page_size: "20" });
    if (element("state-filter").value) query.set("state", element("state-filter").value);
    element("load-work").disabled = true; element("rows").setAttribute("aria-busy", "true"); emptyRows("Loading tasks..."); say("Loading tasks...");
    try {
      const [result, agents] = await Promise.all([get(`${api(repo)}/work?${query}`), getWorkers(repo).then((entries) => ({ entries })).catch((error) => ({ entries: [], error: error.message }))]);
      if (token !== loadToken) return;
      if (!Array.isArray(result.items)) throw new Error("Invalid Work list response.");
      workers = agents.entries; workerError = agents.error ? `Agent inventory unavailable: ${agents.error}` : ""; commands = result.commands || [];
      element("worker-status").textContent = workerError; renderTasks(result); renderPublication(result.publication); renderCommands(); say("");
      history.replaceState(null, "", `/work?${new URLSearchParams({ repository_id: repo, state: element("state-filter").value, page: String(page) })}`);
      if (preferredTask || result.items.length) await selectTask(preferredTask || result.items[0].task_id, false);
    } catch (error) { if (token === loadToken) { emptyRows("Work list unavailable. Retry Load."); say(error.message, true); } }
    finally { if (token === loadToken) { element("load-work").disabled = false; element("rows").setAttribute("aria-busy", "false"); } }
  }
  async function selectTask(taskId, focus = true) {
    invalidateCommand(); selected = taskId; detail = null; const token = ++selectionToken, repo = repository;
    element("task-detail").hidden = false; element("detail-content").hidden = true; element("task-title").textContent = "Task"; say("Loading task...", false, "detail-status"); renderCommands();
    for (const [id, button] of taskButtons) button.setAttribute("aria-pressed", String(id === taskId));
    try {
      const result = await get(`${api(repo)}/work/${encodeURIComponent(taskId)}`);
      if (token !== selectionToken || repo !== repository) return;
      if (result.task?.task_id !== taskId) throw new Error("Task detail does not match the selection.");
      detail = result; renderDetail(); say("", false, "detail-status"); if (focus) element("task-title").focus();
    } catch (error) { if (token === selectionToken && repo === repository) say(error.message, true, "detail-status"); }
  }
  function field(key, label, type = "text", choices = []) {
    const wrapper = make("label", label), control = make(["select", "textarea"].includes(type) ? type : "input");
    control.id = `field-${key}`; wrapper.htmlFor = control.id; control.required = !["goal_id", "checkpoint_required"].includes(key);
    if (!["select", "textarea"].includes(type)) control.type = type;
    if (type === "select") for (const [value, text] of [["", "Select..."], ...choices]) { const option = make("option", text); option.value = value; control.append(option); }
    if (type === "number") { control.min = "1"; control.max = "10"; control.step = "1"; control.value = "5"; }
    wrapper.append(control); element("command-inputs").append(wrapper); flow.inputs.set(key, control); return control;
  }
  function openCommand(kind) {
    if (commandReason(kind)) return;
    invalidateCommand(); flow = { kind, repo: repository, taskId: selected, inputs: new Map(), targets: eligible(kind), busy: false };
    element("command-inputs").replaceChildren(); element("command-rationale").value = ""; element("command-fields").disabled = false;
    element("command-fields").hidden = false;
    element("command-title").textContent = labels[kind]; element("command-context").textContent = `${repository}${kind === "create_work" ? "" : ` / ${detail.task.title}`}`;
    element("command-preview").hidden = true; element("prepare-command").hidden = false; element("prepare-command").textContent = "Preview"; element("confirm-command").hidden = true;
    element("cancel-command").textContent = "Cancel";
    if (targeted.has(kind)) field("worker", "Agent", "select", flow.targets.map((worker, index) => [String(index), `${workerName(worker)} / ${worker.supervisor_id}`]));
    for (const [key, label, type] of definitions[kind] || []) {
      const choices = key === "wait_id" ? unanswered(detail).map((wait) => [wait.wait_id, wait.question]) : key === "disposition" ? [["accept", "Accept"], ["request_changes", "Request changes"]] : [];
      field(key, label, type, choices);
    }
    if (flow.inputs.has("checkpoint_required")) {
      const hint = make("small", "", "muted"); hint.id = "checkpoint-reason"; element("command-inputs").append(hint); flow.inputs.get("checkpoint_required").setAttribute("aria-describedby", hint.id);
      const update = () => { const checkbox = flow.inputs.get("checkpoint_required"), worker = flow.targets[flow.inputs.get("worker").value]; checkbox.disabled = !worker?.supports_checkpoint; if (checkbox.disabled) checkbox.checked = false; hint.textContent = checkbox.disabled ? "Agent does not advertise checkpoint support." : ""; hint.hidden = !checkbox.disabled; };
      flow.inputs.get("worker").addEventListener("change", update); update();
    }
    say("", false, "command-status"); setBusy(flow, false); element("command-dialog").showModal(); (flow.inputs.values().next().value || element("command-rationale")).focus();
  }
  function setBusy(current, busy) {
    current.busy = busy;
    for (const id of ["prepare-command", "confirm-command", "cancel-command", "close-command"]) element(id).disabled = busy;
  }
  function payloadFor(current, values, worker) {
    const payload = { kind: current.kind };
    if (worker) Object.assign(payload, { target_node_id: worker.node_id, target_session_id: worker.session_id });
    switch (current.kind) {
      case "create_work": Object.assign(payload, { task_id: `task:${uuid()}`, title: values.title, acceptance: values.acceptance }); if (values.goal_id) payload.goal_id = values.goal_id; break;
      case "answer_wait": if (!unanswered(detail).some((wait) => wait.wait_id === values.wait_id)) throw new Error("This question was already answered. Close and reload."); Object.assign(payload, { wait_id: values.wait_id, answer: values.answer }); break;
      case "submit_review": Object.assign(payload, { disposition: values.disposition, review_rationale: values.review_rationale }); break;
      case "route_work": payload.route_reference = values.route_reference; break;
      case "release_lease": Object.assign(payload, { expected_owner_id: detail.task.owner_id, expected_lease_expires_at_seconds: detail.task.lease_expires_at_seconds }); break;
      case "steer": payload.instruction = values.instruction; payload.checkpoint_required = values.checkpoint_required; break;
      case "pause": payload.checkpoint_required = values.checkpoint_required; break;
      case "drain": payload.deadline_seconds = now() + Number(values.deadline) * 60; break;
      case "assign": case "resume": break;
      default: throw new Error("Unsupported command.");
    }
    if (payload.checkpoint_required && !worker?.supports_checkpoint) throw new Error("Agent no longer advertises checkpoint support.");
    return payload;
  }
  function previewValues(current, payload, command, worker) {
    const node = element("preview-values"); node.replaceChildren();
    for (const [label, value] of [["Action", labels[current.kind]], ["Repository", current.repo], ["Task", payload.title || detail?.task.title], ["Task ID", payload.task_id || current.taskId], ["Expected version", command.expected_task_version ?? "New task"], ["Reason", command.rationale], ["Expires", when(command.expires_at_seconds)]]) pair(node, label, value);
    if (worker) pair(node, "Agent", `${workerName(worker)} / ${worker.supervisor_id}`);
    for (const [key, value] of Object.entries(payload)) {
      if (["kind", "title", "task_id", "target_node_id", "target_session_id"].includes(key)) continue;
      pair(node, words(key), key.endsWith("_seconds") ? when(value) : typeof value === "boolean" ? value ? "Yes" : "No" : value);
    }
    element("command-preview").hidden = false;
    element("command-fields").hidden = true;
  }
  function showResult(current, result) {
    current.result = result; const status = result.status === "executed" ? result.outcome : result.status;
    say([outcomeText[status] || words(status), result.reason, result.command_id ? `Command: ${result.command_id}` : "", result.receipt_id ? `Receipt: ${result.receipt_id}` : ""].filter(Boolean).join("\n"), ["failed", "expired", "conflicted", "refused", "authorization_unavailable", "command_not_found"].includes(status), "command-status");
    element("prepare-command").hidden = true; element("confirm-command").hidden = result.status !== "pending_confirmation";
    element("cancel-command").textContent = result.status === "pending_confirmation" ? "Cancel" : "Close";
  }
  async function prepare() {
    const current = flow; if (!current || current.busy || !element("command-form").reportValidity()) return;
    setBusy(current, true); element("command-fields").disabled = true; say("Preparing command...", false, "command-status");
    try {
      if (!current.client) {
        const values = Object.fromEntries([...current.inputs].map(([key, control]) => [key, control.type === "checkbox" ? control.checked : control.value.trim()]));
        const rationale = element("command-rationale").value.trim(), candidate = current.targets[values.worker];
        if (!rationale) throw new Error("A reason is required.");
        for (const [key, control] of current.inputs) if (control.required && !values[key]) throw new Error("Complete the required fields.");
        if (current.kind === "drain" && (!Number.isInteger(Number(values.deadline)) || Number(values.deadline) < 1 || Number(values.deadline) > 10)) throw new Error("Deadline must be between 1 and 10 whole minutes.");
        const [fresh, agents] = await Promise.all([current.kind === "create_work" ? null : get(`${api(current.repo)}/work/${encodeURIComponent(current.taskId)}`), targeted.has(current.kind) ? getWorkers(current.repo) : workers]);
        if (flow !== current) return;
        workers = agents;
        if (fresh) { if (fresh.task?.task_id !== current.taskId) throw new Error("Task detail does not match the selection."); detail = fresh; renderDetail(); }
        const reason = commandReason(current.kind); if (reason) throw new Error(reason);
        const worker = targeted.has(current.kind) ? eligible(current.kind).find((entry) => entry.node_id === candidate?.node_id && entry.session_id === candidate?.session_id) : null;
        if (targeted.has(current.kind) && !worker) throw new Error("Selected agent is no longer available.");
        const payload = payloadFor(current, values, worker), command = { idempotency_key: uuid(), rationale, expires_at_seconds: now() + 600 };
        if (current.kind !== "create_work") Object.assign(command, { existing_task_id: current.taskId, expected_task_version: detail.task.version });
        current.expires = command.expires_at_seconds; current.createdTaskId = payload.task_id;
        current.client = new WorkCommandClient({ repositoryId: current.repo, command, payload, fetchImpl }); previewValues(current, payload, command, worker);
      }
      if (now() >= current.expires) throw new Error("Command expired. Close and prepare a new command.");
      const result = await current.client.preview(); if (flow === current) showResult(current, result);
    } catch (error) {
      if (flow === current) { say(error.message, true, "command-status"); element("command-fields").disabled = Boolean(current.client); element("prepare-command").textContent = current.client ? "Retry preview" : "Preview"; }
    } finally { if (flow === current) setBusy(current, false); }
  }
  async function confirm() {
    const current = flow; if (!current || current.busy || current.result?.status !== "pending_confirmation") return;
    if (now() >= current.expires) { say("Command expired. Close and prepare a new command.", true, "command-status"); element("confirm-command").hidden = true; return; }
    setBusy(current, true); say("Confirming command...", false, "command-status");
    try { const result = await current.client.confirm(); if (flow === current) showResult(current, result); }
    catch (error) { if (flow === current) say(`${error.message} Retry confirmation with the unchanged preview.`, true, "command-status"); }
    finally { if (flow === current) setBusy(current, false); }
  }
  function closeCommand() {
    if (flow?.busy) return;
    const refresh = flow?.result?.status === "executed", kind = flow?.kind, preferredTask = flow?.createdTaskId || flow?.taskId; invalidateCommand();
    if (refresh) { element("load-work").focus(); return load(page, preferredTask); }
    commandButtons.get(kind)?.focus();
  }
  on("repository-form", "submit", (event) => { event.preventDefault(); return load(); });
  on("repository-id", "input", () => { if (element("repository-id").value.trim() !== repository) clear(); });
  on("state-filter", "change", () => load());
  on("prev-page", "click", () => { if (page > 1 && !element("prev-page").disabled) return load(page - 1); });
  on("next-page", "click", () => { if (!element("next-page").disabled) return load(page + 1); });
  on("command-form", "submit", (event) => { event.preventDefault(); return prepare(); });
  on("confirm-command", "click", confirm); on("cancel-command", "click", closeCommand); on("close-command", "click", closeCommand);
  on("command-dialog", "cancel", (event) => { event.preventDefault(); return closeCommand(); });
  renderCommands(); const params = new URLSearchParams(location.search), initial = params.get("repository_id");
  if (initial) { element("repository-id").value = initial; element("state-filter").value = params.get("state") || ""; const initialPage = Number(params.get("page")); load(Number.isSafeInteger(initialPage) && initialPage > 0 ? initialPage : 1); }
  return { load, selectTask, openCommand, prepare, confirm };
}

if (globalThis.document?.getElementById("work-page")) mountWorkPage();
