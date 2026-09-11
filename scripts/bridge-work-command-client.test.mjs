import assert from "node:assert/strict";
import { test } from "node:test";

import { WorkCommandClient } from "../crates/ackplane-bridge/static/work-command-client.mjs";

const command = {
  issuing_principal_id: "verified-human",
  idempotency_key: "command-one",
  rationale: "Assign the reviewed task",
  existing_task_id: "task-one",
  expected_task_version: 3,
  expires_at_seconds: 2000000000,
};
const payload = {
  kind: "assign",
  target_node_id: "node-one",
  target_session_id: "session-one",
};
const pending = {
  status: "pending_confirmation",
  command_id: "command/one",
  receipt_id: "receipt-one",
  outcome: "pending_confirmation",
  idempotent_replay: false,
};

function fixture(responses, overrides = {}) {
  const calls = [];
  const client = new WorkCommandClient({
    repositoryId: "repository/one",
    command,
    payload,
    fetchImpl: async (url, options) => {
      calls.push({ url, ...options });
      const response = responses.shift();
      if (response instanceof Error) throw response;
      return { ok: true, status: 200, json: async () => response };
    },
    ...overrides,
  });
  return { client, calls };
}

test("preparing an assignment never confirms or starts it automatically", async () => {
  const { client, calls } = fixture([pending]);
  await assert.rejects(client.confirm(), /confirmation/i);
  assert.equal(calls.length, 0);
  assert.deepEqual(await client.preview(), pending);
  assert.equal(calls.length, 1);
  assert.equal(
    calls[0].url,
    "/api/v1/repositories/repository%2Fone/work/commands",
  );
  assert.equal(calls[0].method, "POST");
  assert.equal(calls[0].credentials, "same-origin");
  assert.deepEqual(JSON.parse(calls[0].body), { ...command, ...payload });
});

test("confirmation sends the exact previewed payload even if the caller edits its form", async () => {
  const mutablePayload = { ...payload };
  const executed = {
    ...pending,
    status: "executed",
    outcome: "pending_delivery",
    reason: "queued",
  };
  const { client, calls } = fixture([pending, executed], {
    payload: mutablePayload,
  });
  await client.preview();
  mutablePayload.target_session_id = "different-agent";
  const result = await client.confirm();
  assert.equal(result.outcome, "pending_delivery");
  assert.equal(
    calls[1].url,
    "/api/v1/repositories/repository%2Fone/work/commands/command%2Fone/confirm",
  );
  assert.deepEqual(JSON.parse(calls[1].body), payload);
});

test("a lost preview response retries the same command identity and body", async () => {
  const { client, calls } = fixture([new Error("connection lost"), pending]);
  await assert.rejects(client.preview(), /connection lost/);
  assert.deepEqual(await client.preview(), pending);
  assert.equal(calls[0].body, calls[1].body);
});

test("a lost confirmation response remains retryable without changing the payload", async () => {
  const executed = {
    ...pending,
    status: "executed",
    outcome: "applied",
    reason: "applied",
  };
  const { client, calls } = fixture([
    pending,
    new Error("connection lost"),
    executed,
  ]);
  await client.preview();
  await assert.rejects(client.confirm(), /connection lost/);
  assert.deepEqual(await client.confirm(), executed);
  assert.equal(calls[1].body, calls[2].body);
  assert.equal(calls[1].url, calls[2].url);
});

test("HTTP success carrying a refusal never enables confirmation", async () => {
  for (const response of [
    { status: "refused", reason: "command_not_permitted" },
    { status: "authorization_unavailable", reason: "no principal" },
    {
      ...pending,
      status: "executed",
      outcome: "conflicted",
      reason: "stale version",
    },
  ]) {
    const { client, calls } = fixture([response]);
    assert.deepEqual(await client.preview(), response);
    await assert.rejects(client.confirm(), /confirmation/i);
    assert.equal(calls.length, 1);
  }
});

test("malformed confirmation responses fail closed", async () => {
  for (const response of [
    null,
    {},
    { ...pending, command_id: "" },
    { ...pending, outcome: "applied" },
  ]) {
    const { client } = fixture([response]);
    await assert.rejects(client.preview(), /response/i);
    await assert.rejects(client.confirm(), /confirmation/i);
  }
});

test("overlapping requests cannot create a second submission", async () => {
  let release;
  const response = new Promise((resolve) => {
    release = resolve;
  });
  const { client } = fixture([], {
    fetchImpl: async () => response,
  });
  const preview = client.preview();
  await assert.rejects(client.preview(), /progress/i);
  release({ ok: true, status: 200, json: async () => pending });
  assert.deepEqual(await preview, pending);
});

test("a failed HTTP request does not become a confirmable command", async () => {
  const { client } = fixture([], {
    fetchImpl: async () => ({ ok: false, status: 403 }),
  });
  await assert.rejects(client.preview(), /403/);
  await assert.rejects(client.confirm(), /confirmation/i);
});
