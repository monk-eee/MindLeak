#!/usr/bin/env node
// Report the governed module count to the ratchet that watches it.
//
// `control:rust-module-length` has a reviewed baseline and a committed
// measurer, and until now nothing told it anything. A registered control that
// is never observed is the same shape as the six script suites and the merge
// audit found earlier: a mechanism that exists, works, and runs nowhere.
//
// It reports; it does not block. The clause resolves at `review` and the
// control's power is `observed`, so a script that failed the push on a
// regression would be enforcing harder than the rule it serves (ADR-0034). The
// number rising is a question for a human — cohesion outranks size, and a
// cohesive module above the line should stay whole.
//
// What it *does* fail on is being unable to report at all. A reporter that
// quietly says nothing is indistinguishable from one reporting a pass, and that
// is the failure this whole class of bug is made of.
import { execFileSync } from "node:child_process";
import path from "node:path";

import { callTools, resolveServer } from "./claim-gate.mjs";
import { measure, THRESHOLD } from "./measure-module-length.mjs";

const CONTROL = "control:rust-module-length";
const SCOPE = "artifact:crates/**";

const repoRoot = execFileSync("git", ["rev-parse", "--show-toplevel"], {
  encoding: "utf8",
}).trim();

const fail = (message) => {
  console.error(`observe-module-length: ${message}`);
  process.exit(1);
};

const sessionId = process.env.LODESTAR_SESSION_ID || "";
if (!/^[0-9a-f]{32}$/.test(sessionId)) {
  fail(
    "LODESTAR_SESSION_ID must be a 128-bit hex token; without a session the observation cannot be attributed",
  );
}

const server = resolveServer(repoRoot, "lodestar");
if (!server) {
  fail(
    "no lodestar-mcp binary found; build one or set LODESTAR_MCP_BIN. Reporting nothing is not the same as reporting a pass",
  );
}

const modules = measure(path.join(repoRoot, "crates"));
const over = modules.filter((m) => m.lines > THRESHOLD);
const measured = over.length;

const [, observation] = callTools(server, repoRoot, [
  { name: "open_session", arguments: { session_id: sessionId } },
  {
    name: "observe_ratchet",
    arguments: {
      control_id: CONTROL,
      measured,
      scope: SCOPE,
      evidence_refs: ["scripts/measure-module-length.mjs"],
    },
  },
]);

if (typeof observation === "string") {
  fail(`the ratchet refused the observation: ${observation}`);
}
if (!observation || !observation.status) {
  fail(`the ratchet returned nothing usable: ${JSON.stringify(observation)}`);
}

console.log(
  `observe-module-length: ${measured} of ${modules.length} modules over ${THRESHOLD} non-test lines`,
);
console.log(
  `observe-module-length: ${observation.status}, resolving ${observation.effective} — ${observation.finding}`,
);

if (observation.status === "fail") {
  console.log(
    "observe-module-length: the count rose. Split the module, or accept a new baseline\n" +
      "  with accept_ratchet_baseline if cohesion genuinely demands it stays whole —\n" +
      "  it requires a reason as well as your name, and that reason is the record of\n" +
      "  the exception. Accept it for someone else's module and it is your name on it.",
  );
  for (const m of over) {
    console.log(`    ${String(m.lines).padStart(5)}  crates/${m.path}`);
  }
}
