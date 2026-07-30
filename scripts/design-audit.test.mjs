// Tests for the design audit's operator-facing advice. Run with: make script-test
//
// The audit's value is entirely in what it tells a person to do next, and that
// advice is prose — nothing compiles it, so it rots silently when the tool
// surface changes. It had rotted twice over: it named `reopen_undecided_design`
// and `accept_design`, neither of which the server has advertised since the
// design cluster collapsed to design_register/decide/promote/query, and the
// remedy itself was wrong.
//
// The guard scans design-audit.mjs rather than this file, so it cannot match its
// own list of retired names.
import { test } from "node:test";
import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const here = path.dirname(fileURLToPath(import.meta.url));
const source = fs.readFileSync(path.join(here, "design-audit.mjs"), "utf8");

/** Names the design cluster used before it collapsed to four verbs. */
const RETIRED = [
  "register_design",
  "accept_design",
  "reject_design",
  "retire_design",
  "supersede_design",
  "reopen_undecided_design",
  "attribute_design_decision",
  "plan_design_promotion",
  "promote_design",
  "revise_design_promotion",
  "design_board",
  "list_designs",
  "reconcile_designs",
];

test("the audit never names a design verb the server retired", () => {
  // Advice that names a tool nobody can find is worse than no advice: the
  // reader cannot tell whether they misread it or the tool is missing.
  const found = RETIRED.filter((name) =>
    new RegExp(`\\b${name}\\b`).test(source),
  );
  assert.deepEqual(
    found,
    [],
    `design-audit.mjs still names retired verb(s): ${found.join(", ")}`,
  );
});

test("the audit points an unattributed row at attribute, not at reopen", () => {
  // ADR-0051: `attribute` records the decider on a decision the ledger already
  // asserts, leaving status untouched, and takes exactly the rows `reopen`
  // refuses. Advising reopen here would discard an acceptance that already
  // holds and send the row back to proposed — a bigger act than the defect.
  const undecided = source.slice(source.indexOf('kind: "undecided"'));
  const detail = undecided.slice(0, undecided.indexOf("\n    });"));

  assert.match(detail, /design_decide/, "must name the verb that exists");
  assert.match(
    detail,
    /attribute/,
    "must name the decision that fits an unattributed row",
  );
  assert.doesNotMatch(
    detail,
    /\breopen\b/,
    "reopen would discard the recorded acceptance",
  );
});

test("supersession advice names the current verb and stays a human act", () => {
  const supersession = source.slice(source.indexOf('kind: "supersession"'));
  const detail = supersession.slice(0, supersession.indexOf("\n        });"));

  assert.match(detail, /design_decide/);
  assert.match(detail, /supersede/);
  // ADR-0050: supersession is recorded by a person, never inferred from a file.
  assert.match(detail, /never inferred/);
});

test("the audit still calls a verb the server actually advertises", () => {
  // The advice was stale while the call site was current, so the call site is
  // worth pinning too — a rename that fixed one and not the other would leave
  // the audit unable to run at all.
  assert.match(source, /name: "design_query"/);
});
