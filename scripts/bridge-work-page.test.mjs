import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";
import { mountWorkPage } from "../crates/ackplane-bridge/static/work-page.mjs";

const pageUrl = new URL(
  "../crates/ackplane-bridge/static/work.html",
  import.meta.url,
);

test("available Work commands are no longer permanently disabled by the page", () => {
  const html = readFileSync(pageUrl, "utf8");
  assert.doesNotMatch(html, /control\.disabled\s*=\s*true/);
  assert.match(html, /<dialog\b/);
  assert.match(html, /type="module" src="\/static\/work-page\.mjs"/);
});

class Element {
  constructor(tag, document) {
    this.tagName = tag;
    this.document = document;
    this.children = [];
    this.listeners = new Map();
    this.attributes = new Map();
    this.dataset = {};
    this.value = "";
    this.type = "";
    this.disabled = false;
    this.hidden = false;
    this.open = false;
    this.checked = false;
    this.text = "";
  }
  set id(value) {
    this.identifier = value;
    this.document.nodes.set(value, this);
  }
  get id() {
    return this.identifier;
  }
  set textContent(value) {
    this.text = String(value);
    this.children = [];
  }
  get textContent() {
    return this.text + this.children.map((child) => child.textContent).join("");
  }
  append(...nodes) {
    this.children.push(...nodes);
  }
  replaceChildren(...nodes) {
    this.text = "";
    this.children = nodes;
  }
  setAttribute(key, value) {
    this.attributes.set(key, value);
  }
  getAttribute(key) {
    return this.attributes.get(key);
  }
  addEventListener(event, listener) {
    this.listeners.set(event, listener);
  }
  fire(event) {
    if (event === "click" && this.disabled) return;
    return this.listeners.get(event)?.({ preventDefault() {} });
  }
  focus() {
    this.document.activeElement = this;
  }
  showModal() {
    this.open = true;
  }
  close() {
    this.open = false;
  }
  reportValidity() {
    return true;
  }
}

const task = {
  task_id: "task/one",
  title: "Repair build",
  state: "open",
  version: 3,
  goal_id: "goal:build",
  owner_id: null,
  owner_session_id: null,
  declared_paths: ["src/build.rs"],
  declared_symbols: ["bare_name", "symbol:src/build.rs:build"],
  updated_at_seconds: 100,
};
const detail = {
  task,
  acceptance: "Build passes",
  history: [
    {
      from_state: null,
      to_state: "open",
      actor_id: "human",
      recorded_at_seconds: 100,
    },
  ],
  waits: [],
};
const supervisor = {
  supervisor_id: "supervisor/one",
  node_id: "node/one",
  freshness: "current",
  supported_directives: ["assign"],
  supports_checkpoint: false,
};
const session = {
  session_id: "session/one",
  worker_id: "Builder",
  runtime: "local_machine",
  state: "started",
};
const pending = {
  status: "pending_confirmation",
  command_id: "command/one",
  receipt_id: "receipt/one",
  outcome: "pending_confirmation",
  idempotent_replay: false,
};
const operations = [
  "create_work",
  "assign",
  "answer_wait",
  "submit_review",
  "route_work",
  "release_lease",
  "pause",
  "resume",
  "steer",
  "drain",
];
const capabilities = operations.map((operation) => ({
  operation,
  state: "available_without_policy",
  reason: "Verified human",
}));
const success = (body) => ({
  ok: true,
  status: 200,
  json: async () => structuredClone(body),
});

function fixture(overrides = {}) {
  const document = {
    nodes: new Map(),
    createElement(tag) {
      return new Element(tag, this);
    },
    getElementById(id) {
      return this.nodes.get(id);
    },
  };
  for (const match of readFileSync(pageUrl, "utf8").matchAll(
    /<([a-z][a-z0-9]*)\b[^>]*\bid="([^"]+)"[^>]*>/g,
  )) {
    const node = document.createElement(match[1]);
    node.id = match[2];
    node.hidden = /\bhidden\b/.test(match[0]);
    node.disabled = /\bdisabled\b/.test(match[0]);
  }
  const calls = [],
    state = {
      detail: structuredClone(detail),
      supervisors: [structuredClone(supervisor)],
      sessions: [structuredClone(session)],
      commands: structuredClone(capabilities),
      constitution: {
        found: true,
        status: "adopted",
        clauses: [
          {
            id: "goal:build",
            title: "Reliable builds",
            kind: "objective",
            status: "active",
          },
          {
            id: "rule:scope",
            title: "Stay in scope",
            kind: "constraint",
            status: "active",
          },
        ],
      },
      responses: [pending],
      items: [structuredClone(task)],
      total: 1,
      publication: { state: "current", claims_only_total: 0, claims_only: [] },
      clock: 1000,
      ...overrides,
    };
  const fetchImpl = async (url, options = {}) => {
    calls.push({ url, ...options });
    const custom = await state.fetch?.(url, options);
    if (custom) return custom;
    if (options.method === "POST") {
      const result = state.responses.shift();
      if (result instanceof Error) throw result;
      return success(result);
    }
    if (url.endsWith("/supervisors"))
      return success({ entries: state.supervisors });
    if (url.endsWith("/sessions")) return success({ entries: state.sessions });
    if (url.endsWith("/constitution")) return success(state.constitution);
    if (url.includes("/work?"))
      return success({
        items: state.items,
        total: state.total,
        page: Number(new URL(url, "http://bridge").searchParams.get("page")),
        page_size: 20,
        commands: state.commands,
        publication: state.publication,
      });
    return success(state.detail);
  };
  let sequence = 0;
  const app = mountWorkPage({
    document,
    fetchImpl,
    location: { search: "" },
    history: { replaceState() {} },
    now: () => state.clock,
    uuid: () => `uuid-${++sequence}`,
  });
  const element = (id) => document.getElementById(id);
  const button = (kind) =>
    [
      ...element("command-controls").children,
      ...element("secondary-controls").children,
    ]
      .map((slot) => slot.children[0])
      .find((control) => control.dataset.command === kind);
  element("repository-id").value = "repo/one";
  return {
    app,
    state,
    document,
    element,
    button,
    calls,
    posts: () => calls.filter((call) => call.method === "POST"),
  };
}

async function ready(kind = "assign", overrides = {}) {
  const context = fixture(overrides);
  await context.app.load();
  await context.button(kind).fire("click");
  context.element("command-rationale").value = "Reviewed by the operator";
  if (context.element("field-worker"))
    context.element("field-worker").value = "0";
  return context;
}

test("advertised Create and Assign now open a native dialog instead of staying disabled", async () => {
  const context = await ready();
  assert.equal(context.button("create_work").disabled, false);
  assert.equal(context.button("assign").disabled, false);
  assert.equal(context.element("command-dialog").open, true);
  assert.match(context.element("field-worker").textContent, /Builder/);
  assert.equal(context.element("field-target_node_id"), undefined);
  assert.equal(context.posts().length, 0);
});

test("assignment fetches the latest task version then awaits explicit immutable confirmation", async () => {
  const context = await ready();
  context.state.detail.task.version = 9;
  await context.element("command-form").fire("submit");
  const request = JSON.parse(context.posts()[0].body);
  assert.equal(request.expected_task_version, 9);
  assert.equal(request.existing_task_id, "task/one");
  assert.equal(request.target_node_id, "node/one");
  assert.equal(request.target_session_id, "session/one");
  assert.equal(request.expires_at_seconds, 1600);
  assert.equal(request.issuing_principal_id, undefined);
  assert.equal(context.posts().length, 1);
  assert.equal(context.element("command-fields").disabled, true);
  assert.equal(context.element("command-fields").hidden, true);
  assert.equal(context.element("confirm-command").hidden, false);
  assert.match(context.element("preview-values").textContent, /Builder/);
  context.element("field-worker").value = "different";
  context.state.responses.push({
    ...pending,
    status: "executed",
    outcome: "pending_delivery",
    reason: "queued",
  });
  await context.element("confirm-command").fire("click");
  assert.deepEqual(JSON.parse(context.posts()[1].body), {
    kind: "assign",
    target_node_id: "node/one",
    target_session_id: "session/one",
  });
  assert.match(
    context.element("command-status").textContent,
    /Pending delivery\. Not yet applied/,
  );
  assert.equal(context.element("confirm-command").hidden, true);
});

test("created work accepts an adopted constitution and binds its active goal and explicit scope", async () => {
  const context = await ready("create_work");
  context.element("field-title").value = "New task";
  context.element("field-acceptance").value = "Tests pass";
  assert.equal(context.element("field-goal_id").tagName, "select");
  assert.match(context.element("field-goal_id").textContent, /Reliable builds/);
  assert.doesNotMatch(
    context.element("field-goal_id").textContent,
    /Stay in scope/,
  );
  context.element("field-goal_id").value = "goal:build";
  context.element("field-declared_paths").value =
    " src/build.rs\r\n tests/build.rs\n src/build.rs ";
  context.element("field-declared_symbols").value = "symbol:src/build.rs:build";
  await context.app.prepare();
  assert.deepEqual(JSON.parse(context.posts()[0].body), {
    idempotency_key: "uuid-2",
    rationale: "Reviewed by the operator",
    expires_at_seconds: 1600,
    kind: "create_work",
    task_id: "task:uuid-1",
    title: "New task",
    acceptance: "Tests pass",
    goal_id: "goal:build",
    declared_paths: ["src/build.rs", "tests/build.rs"],
    declared_symbols: ["symbol:src/build.rs:build"],
  });
  assert.match(
    context.element("preview-values").textContent,
    /tests\/build.rs/,
  );
  context.element("field-declared_paths").value = "outside-preview.rs";
  context.state.responses.push({
    ...pending,
    status: "executed",
    outcome: "applied",
    reason: "Created",
  });
  await context.app.confirm();
  const confirmed = JSON.parse(context.posts()[1].body);
  assert.deepEqual(confirmed.declared_paths, [
    "src/build.rs",
    "tests/build.rs",
  ]);
  assert.deepEqual(confirmed.declared_symbols, ["symbol:src/build.rs:build"]);
});

test("unscoped or goal-less work cannot be offered to a worker that would refuse its context", async () => {
  for (const patch of [
    { goal_id: null },
    { declared_paths: [], declared_symbols: [] },
  ]) {
    const context = fixture({
      detail: { ...detail, task: { ...task, ...patch } },
    });
    await context.app.load();
    assert.equal(context.button("assign").disabled, true);
    assert.match(context.button("assign").title, /goal|scope/i);
  }
});

test("creating work refuses an empty scope, an unpublished goal, or a goal retired during preview", async () => {
  for (const invalid of ["scope", "goal", "retired"]) {
    const context = await ready("create_work");
    context.element("field-title").value = "Scoped task";
    context.element("field-acceptance").value = "Checks pass";
    context.element("field-goal_id").value =
      invalid === "goal" ? "goal:unpublished" : "goal:build";
    context.element("field-declared_paths").value =
      invalid === "scope" ? " \n " : "src/build.rs";
    if (invalid === "retired")
      context.state.constitution.clauses[0].status = "retired";
    await context.app.prepare();
    assert.equal(context.posts().length, 0);
    assert.match(context.element("command-status").textContent, /goal|scope/i);
  }
});

test("symbol-only scope remains explicit and missing constitution leaves reads available", async () => {
  const context = await ready("create_work");
  context.element("field-title").value = "One symbol";
  context.element("field-acceptance").value = "Symbol checks pass";
  context.element("field-goal_id").value = "goal:build";
  context.element("field-declared_symbols").value = "symbol:src/build.rs:build";
  await context.app.prepare();
  assert.deepEqual(JSON.parse(context.posts()[0].body).declared_paths, []);
  context.state.constitution = { found: false, clauses: [] };
  await context.app.load();
  assert.equal(context.button("create_work").disabled, true);
  assert.match(context.button("create_work").title, /goal|constitution/i);
  assert.match(context.element("rows").textContent, /Repair build/);
});

test("missing versions, authorization, stale workers and unadvertised capabilities fail closed", async () => {
  for (const patch of [
    { detail: { ...detail, task: { ...task, version: undefined } } },
    {
      commands: capabilities.map((entry) => ({
        ...entry,
        state: "authorization_unavailable",
        reason: "No verified human",
      })),
    },
    { supervisors: [{ ...supervisor, freshness: "stale" }] },
    { sessions: [{ ...session, state: "completed" }] },
    { supervisors: [{ ...supervisor, supported_directives: [] }] },
  ]) {
    const context = fixture(patch);
    await context.app.load();
    assert.equal(context.button("assign").disabled, true);
    assert.ok(context.button("assign").title);
    context.app.openCommand("assign");
    assert.equal(context.element("command-dialog").open, false);
    assert.equal(context.posts().length, 0);
  }
  const context = fixture();
  await context.app.load();
  assert.equal(context.button("pause").disabled, true);
});

test("lost preview and confirmation responses retry identical bodies without editable payloads", async () => {
  const context = await ready("assign", {
    responses: [
      new Error("connection lost"),
      pending,
      new Error("connection lost"),
      { ...pending, status: "executed", outcome: "applied", reason: "done" },
    ],
  });
  await context.app.prepare();
  assert.equal(context.element("command-fields").disabled, true);
  context.element("command-rationale").value = "Changed";
  await context.app.prepare();
  assert.equal(context.posts()[0].body, context.posts()[1].body);
  await context.app.confirm();
  await context.app.confirm();
  assert.equal(context.posts()[2].body, context.posts()[3].body);
  assert.match(
    context.element("command-status").textContent,
    /Command applied/,
  );
  assert.doesNotMatch(
    context.element("command-status").textContent,
    /Task completed/i,
  );
});

test("changing repository invalidates a pending command before confirmation", async () => {
  const context = await ready();
  await context.app.prepare();
  context.element("repository-id").value = "repo/two";
  await context.element("repository-id").fire("input");
  await context.app.confirm();
  assert.equal(context.posts().length, 1);
  assert.equal(context.element("command-dialog").open, false);
  assert.equal(context.element("task-detail").hidden, true);
  assert.equal(context.button("create_work").disabled, true);
});

test("late task detail cannot replace a newer selection or revive its command", async () => {
  const context = fixture();
  await context.app.load();
  let resolve;
  context.state.fetch = (url) =>
    url.endsWith("/work/old")
      ? new Promise((done) => {
          resolve = done;
        })
      : undefined;
  const old = context.app.selectTask("old");
  await context.app.selectTask("task/one");
  resolve(
    success({
      ...detail,
      task: { ...task, task_id: "old", title: "Old selection" },
    }),
  );
  await old;
  assert.equal(context.element("task-title").textContent, "Repair build");
});

test("tenant visibility failures clear old tasks and never leave enabled commands", async () => {
  const context = fixture();
  await context.app.load();
  context.state.fetch = () => ({ ok: false, status: 404 });
  await context.app.load();
  assert.match(
    context.element("notice").textContent,
    /not visible to this tenant/,
  );
  assert.doesNotMatch(context.element("rows").textContent, /Repair build/);
  assert.equal(context.button("create_work").disabled, true);
});

test("detail preserves scope, waits, history and actual evidence and conformance routes as text", async () => {
  const context = fixture({
    detail: { ...detail, acceptance: "<img src=x onerror=alert(1)>" },
  });
  await context.app.load();
  assert.equal(
    context.element("acceptance").textContent,
    "<img src=x onerror=alert(1)>",
  );
  assert.equal(context.element("acceptance").children.length, 0);
  assert.match(context.element("task-scope").textContent, /src\/build\.rs/);
  assert.match(context.element("task-history").textContent, /human/);
  const [evidence, conformance] = context.element("record-links").children;
  assert.equal(
    evidence.href,
    "/evidence?repository=repo%2Fone&task=task%2Fone",
  );
  assert.equal(
    conformance.href,
    "/api/v1/repositories/repo%2Fone/tasks/task%2Fone/conformance",
  );
  const seeds = new URL(
    context.element("scope-link").children[0].href,
    "http://bridge",
  ).searchParams.get("seeds");
  assert.equal(seeds, "artifact:src/build.rs,symbol:src/build.rs:build");
});

test("claims-only publication remains diagnostic and never becomes invented tasks", async () => {
  const context = fixture({
    items: [],
    total: 0,
    publication: {
      state: "claims_only",
      claims_only_total: 1,
      claims_only: [
        {
          task_id: "unpublished",
          owner_id: "worker",
          declared_paths: ["src/a.rs"],
        },
      ],
    },
  });
  await context.app.load();
  assert.match(context.element("publication-title").textContent, /Claims only/);
  assert.match(context.element("claims-only-rows").textContent, /unpublished/);
  assert.doesNotMatch(context.element("rows").textContent, /unpublished/);
  assert.equal(context.button("create_work").disabled, false);
});

test("answering a published question uses its ID and refuses an answer that became stale", async () => {
  const wait = {
    wait_id: "wait/one",
    question: "Which build?",
    asked_by: "Builder",
    answer: null,
  };
  const context = await ready("answer_wait", {
    detail: { ...detail, task: { ...task, state: "waiting" }, waits: [wait] },
  });
  context.element("field-wait_id").value = wait.wait_id;
  context.element("field-answer").value = "The release build";
  await context.app.prepare();
  assert.equal(JSON.parse(context.posts()[0].body).wait_id, wait.wait_id);
  assert.equal(JSON.parse(context.posts()[0].body).answer, "The release build");
  context.app.openCommand("answer_wait");
  context.element("command-rationale").value = "Answer";
  context.element("field-wait_id").value = wait.wait_id;
  context.element("field-answer").value = "Changed answer";
  context.state.detail.waits[0].answer = "Already answered";
  await context.app.prepare();
  assert.equal(context.posts().length, 1);
  assert.match(context.element("command-status").textContent, /No unanswered/);
});

test("review keeps the command reason separate from both supported review dispositions", async () => {
  for (const disposition of ["accept", "request_changes"]) {
    const context = await ready("submit_review", {
      detail: { ...detail, task: { ...task, state: "in_review" } },
    });
    context.element("field-disposition").value = disposition;
    context.element("field-review_rationale").value = "Evidence checked";
    await context.app.prepare();
    const request = JSON.parse(context.posts()[0].body);
    assert.equal(request.disposition, disposition);
    assert.equal(request.review_rationale, "Evidence checked");
    assert.equal(request.rationale, "Reviewed by the operator");
  }
});

test("a finished worker can submit its still-claimed task for review without marking it completed", async () => {
  const context = fixture({
    detail: { ...detail, task: { ...task, state: "claimed" } },
    sessions: [{ ...session, state: "completed" }],
  });
  await context.app.load();
  assert.equal(context.button("submit_review").disabled, false);
  await context.button("submit_review").fire("click");
  context.element("command-rationale").value =
    "Review the completed worker output";
  context.element("field-disposition").value = "accept";
  context.element("field-review_rationale").value =
    "Evidence attached for conformance review";
  await context.app.prepare();
  assert.equal(JSON.parse(context.posts()[0].body).kind, "submit_review");
  assert.equal(context.state.detail.task.state, "claimed");
  assert.equal(context.posts().length, 1);
});

test("route and release lease send the supported payload with fresh owner and lease guards", async () => {
  const route = await ready("route_work");
  route.element("field-route_reference").value = "queue:build";
  await route.app.prepare();
  assert.equal(
    JSON.parse(route.posts()[0].body).route_reference,
    "queue:build",
  );
  const lease = await ready("release_lease", {
    detail: {
      ...detail,
      task: { ...task, owner_id: "old-owner", lease_expires_at_seconds: 1200 },
    },
  });
  lease.state.detail.task.owner_id = "new-owner";
  lease.state.detail.task.lease_expires_at_seconds = 1500;
  await lease.app.prepare();
  const request = JSON.parse(lease.posts()[0].body);
  assert.equal(request.expected_owner_id, "new-owner");
  assert.equal(request.expected_lease_expires_at_seconds, 1500);
});

test("pause resume steer and drain use only advertised targets and typed payload fields", async () => {
  for (const kind of ["pause", "resume", "steer", "drain"]) {
    const context = await ready(kind, {
      supervisors: [
        {
          ...supervisor,
          supported_directives: [kind],
          supports_checkpoint: true,
        },
      ],
    });
    if (kind === "pause" || kind === "steer") {
      await context.element("field-worker").fire("change");
      context.element("field-checkpoint_required").checked = true;
    }
    if (kind === "steer")
      context.element("field-instruction").value = "Run focused tests";
    await context.app.prepare();
    const request = JSON.parse(context.posts()[0].body);
    assert.equal(request.kind, kind);
    assert.equal(request.target_session_id, session.session_id);
    if (kind === "pause" || kind === "steer")
      assert.equal(request.checkpoint_required, true);
    if (kind === "steer")
      assert.equal(request.instruction, "Run focused tests");
    if (kind === "drain") {
      assert.equal(request.deadline_seconds, 1300);
      assert.equal(request.deadline, undefined);
    }
  }
});

test("agent capability loss during preview blocks the command without guessing a replacement", async () => {
  const context = await ready();
  context.state.supervisors[0].supported_directives = [];
  await context.app.prepare();
  assert.equal(context.posts().length, 0);
  assert.match(
    context.element("command-status").textContent,
    /No current agent/,
  );
  assert.equal(context.element("command-fields").disabled, false);
});

test("expired previews and refused receipts never remain confirmable", async () => {
  const expired = await ready();
  await expired.app.prepare();
  expired.state.clock = 1600;
  await expired.app.confirm();
  assert.equal(expired.posts().length, 1);
  assert.equal(expired.element("confirm-command").hidden, true);
  assert.match(expired.element("command-status").textContent, /expired/);
  for (const response of [
    { status: "refused", reason: "not permitted" },
    { status: "authorization_unavailable", reason: "no principal" },
    { status: "command_not_found" },
    {
      ...pending,
      status: "executed",
      outcome: "conflicted",
      reason: "stale version",
    },
  ]) {
    const context = await ready("assign", { responses: [response] });
    await context.app.prepare();
    await context.app.confirm();
    assert.equal(context.posts().length, 1);
    assert.equal(context.element("confirm-command").hidden, true);
    assert.equal(context.element("command-status").dataset.error, "true");
  }
});

test("blank required text is rejected before any command submission", async () => {
  const context = await ready();
  context.element("command-rationale").value = "   ";
  await context.app.prepare();
  assert.equal(context.posts().length, 0);
  assert.equal(context.element("command-fields").disabled, false);
  assert.match(
    context.element("command-status").textContent,
    /reason is required/,
  );
});

test("late repository responses cannot replace a newer tenant-scoped board", async () => {
  const context = fixture();
  let resolve;
  context.state.fetch = (url) =>
    url.includes("repo%2Fone/work?")
      ? new Promise((done) => {
          resolve = done;
        })
      : undefined;
  const old = context.app.load();
  context.element("repository-id").value = "repo/two";
  context.element("repository-id").fire("input");
  context.state.items = [{ ...task, title: "Current board" }];
  await context.app.load();
  resolve(
    success({
      items: [{ ...task, title: "Stale board" }],
      total: 1,
      page: 1,
      page_size: 20,
      commands: capabilities,
    }),
  );
  await old;
  assert.match(context.element("rows").textContent, /Current board/);
  assert.doesNotMatch(context.element("rows").textContent, /Stale board/);
});

test("filter pagination empty state and worker inventory failures retain usable task reads", async () => {
  const context = fixture({ total: 21 });
  context.element("state-filter").value = "waiting";
  await context.app.load();
  assert.match(
    context.calls.find((call) => call.url.includes("/work?")).url,
    /state=waiting/,
  );
  assert.equal(context.element("next-page").disabled, false);
  await context.element("next-page").fire("click");
  assert.match(context.element("pager-summary").textContent, /21-21 of 21/);
  assert.equal(context.element("prev-page").disabled, false);
  context.state.items = [];
  context.state.total = 0;
  context.state.fetch = (url) =>
    url.endsWith("/supervisors") ? { ok: false, status: 503 } : undefined;
  await context.app.load();
  assert.match(context.element("rows").textContent, /No tasks/);
  assert.equal(context.button("create_work").disabled, false);
  assert.match(context.element("worker-status").textContent, /503/);
});

test("cancelling a preview sends no confirmation and restores keyboard focus", async () => {
  const context = await ready();
  await context.app.prepare();
  await context.element("command-dialog").fire("cancel");
  assert.equal(context.posts().length, 1);
  assert.equal(context.element("command-dialog").open, false);
  assert.equal(context.document.activeElement, context.button("assign"));
});

// A lost confirmation used to become unrecoverable at expiry and encourage a
// new command. Retry the immutable identity so the server can return its receipt.
test("lost confirmations remain recoverable after expiry without creating a second command", async () => {
  for (const outcome of ["applied", "expired"]) {
    const receipt = {
      ...pending,
      status: "executed",
      outcome,
      reason:
        outcome === "applied" ? "Original effect" : "No effect before expiry",
      idempotent_replay: outcome === "applied",
    };
    const context = await ready("assign", {
      responses: [pending, new Error("confirmation response lost"), receipt],
    });
    await context.app.prepare();
    context.state.clock = 1599;
    await context.app.confirm();
    context.state.clock = 1600;
    await context.app.confirm();
    assert.equal(context.posts().length, 3);
    assert.equal(context.posts()[1].url, context.posts()[2].url);
    assert.equal(context.posts()[1].body, context.posts()[2].body);
    assert.equal(context.element("confirm-command").hidden, true);
    assert.match(
      context.element("command-status").textContent,
      new RegExp(receipt.reason),
    );
    assert.equal(
      context.element("command-status").dataset.error,
      String(outcome === "expired"),
    );
  }
});

test(
  "preview in flight stays singular and a changed selection cannot revive its receipt",
  { timeout: 1000 },
  async () => {
    const context = await ready();
    let resolve, started;
    const submitted = new Promise((done) => {
      started = done;
    });
    context.state.fetch = (url, options) =>
      options.method === "POST"
        ? new Promise((done) => {
            resolve = done;
            started();
          })
        : undefined;
    const preview = context.app.prepare();
    await submitted;
    await context.app.prepare();
    assert.equal(context.posts().length, 1);
    await context.app.selectTask("task/one");
    resolve(success(pending));
    await preview;
    await context.app.confirm();
    assert.equal(context.element("command-dialog").open, false);
    assert.equal(context.posts().length, 1);
  },
);

test("unsupported checkpoints have a visible accessible reason and cannot be requested", async () => {
  const context = await ready("pause", {
    supervisors: [{ ...supervisor, supported_directives: ["pause"] }],
  });
  await context.element("field-worker").fire("change");
  assert.equal(context.element("field-checkpoint_required").disabled, true);
  assert.equal(
    context
      .element("field-checkpoint_required")
      .getAttribute("aria-describedby"),
    "checkpoint-reason",
  );
  assert.match(
    context.element("checkpoint-reason").textContent,
    /does not advertise checkpoint/,
  );
  context.element("field-checkpoint_required").checked = true;
  await context.app.prepare();
  assert.equal(context.posts().length, 0);
});

test("closing an accepted command refreshes the selected task without claiming it was applied", async () => {
  const context = await ready();
  await context.app.prepare();
  context.state.responses.push({
    ...pending,
    status: "executed",
    outcome: "accepted",
    reason: "received",
  });
  await context.app.confirm();
  assert.match(
    context.element("command-status").textContent,
    /Accepted\. Not yet applied/,
  );
  context.state.items = [
    { ...task, task_id: "other", title: "Different task" },
    task,
  ];
  context.state.total = 2;
  await context.element("cancel-command").fire("click");
  assert.equal(context.element("task-title").textContent, "Repair build");
  assert.equal(context.element("detail-content").hidden, false);
});
