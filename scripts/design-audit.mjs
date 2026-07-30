// Compare the ADR files on disk against the durable design ledger.
//
// The two drift. An ADR merges without ever being registered; a design is
// accepted in the ledger while its file still says Proposed; a reconciled
// import lands `accepted` with no decider, which freezes the row against
// accept_design (ADR-0047). Each of those was found by hand this far, one
// ad-hoc query at a time, and each was invisible until someone went looking.
//
// It reads the ledger through the lodestar MCP surface rather than opening
// spec.db, for two reasons: the server already resolves its own per-repository
// database (ADR-0038), so this does not fork that path rule; and list_designs
// already omits retired records, so a retired row cannot masquerade as an
// orphan.
//
// This is a local diagnostic, not a hook. CI has no ledger to read — the
// ADR-index guard compares files to files and can gate; this cannot.
//
// Platform-agnostic: node only. Usage:
//   node scripts/design-audit.mjs           report
//   node scripts/design-audit.mjs --check   exit non-zero when anything drifts

import { spawn, execFileSync } from "node:child_process";
import { createInterface } from "node:readline";
import { pathToFileURL } from "node:url";

import { isSuperseded, readAdrFiles } from "./adr-files.mjs";
import { resolveServer } from "./claim-gate.mjs";

const repoRoot = execFileSync("git", ["rev-parse", "--show-toplevel"], {
  encoding: "utf8",
}).trim();

/** File status -> the ledger status that means the same thing. */
const LEDGER_EQUIVALENT = {
  Proposed: "proposed",
  Accepted: "accepted",
  Rejected: "rejected",
};

/**
 * Findings comparing ADR files to ledger rows. Pure: no I/O, so the comparison
 * is testable without a server or a database.
 *
 * @param files  from readAdrFiles()
 * @param designs  list_designs rows (retired already excluded by the server)
 */
export const auditDesigns = (files, designs) => {
  const byPath = new Map(designs.map((d) => [d.adr_path, d]));
  const seen = new Set();
  const findings = [];

  for (const file of files) {
    const design = byPath.get(file.path);
    if (!design) {
      findings.push({
        kind: "unregistered",
        adr: file.number,
        detail: `${file.path} has no design row`,
      });
      continue;
    }
    seen.add(file.path);

    // "Superseded by [0032](...)" used to be a statement the ledger could not
    // hold, and was reported as a note. Since ADR-0050 it can: a file claiming
    // supersession that the ledger has not been told about is drift, and so is
    // the reverse.
    if (isSuperseded(file.status)) {
      if (!design.superseded) {
        findings.push({
          kind: "supersession",
          adr: file.number,
          detail: `file says "${file.status}"; the ledger has no supersession recorded. A person runs supersede_design; it is never inferred from the file`,
        });
      }
      continue;
    }

    if (design.superseded) {
      findings.push({
        kind: "supersession",
        adr: file.number,
        detail: `ledger says superseded by ${design.superseded.by_design}, recorded by ${design.superseded.by}; the file does not say so`,
      });
      continue;
    }

    const expected = LEDGER_EQUIVALENT[file.status.split(/\s+/)[0]];
    if (expected && design.status !== expected) {
      findings.push({
        kind: "disagreement",
        adr: file.number,
        detail: `file says ${file.status}, ledger says ${design.status}`,
      });
    }

    // A decision nobody made. reconcile_designs imports a status out of the
    // file, so an accepted row can arrive with no decider — and accept_design
    // then refuses it, because deciding twice is not an undo.
    if (design.status !== "proposed" && !design.decided_by) {
      findings.push({
        kind: "undecided",
        adr: file.number,
        detail: `ledger says ${design.status} but names no decider; reopen_undecided_design then accept_design`,
      });
    }
  }

  for (const design of designs) {
    if (seen.has(design.adr_path)) continue;
    findings.push({
      kind: "orphan",
      adr: design.id,
      detail: `ledger row for ${design.adr_path}, which is not an ADR here`,
    });
  }

  return findings;
};

const serverBinary = () => {
  // Shared resolver rather than a release-only path: a debug build is the
  // normal state for a developer, and an audit nobody can run audits nothing.
  const path = resolveServer(repoRoot, "lodestar");
  if (!path) {
    throw new Error(
      "design-audit: no lodestar-mcp binary found.\n" +
        "  Build one:  cargo build -p lodestar-mcp\n" +
        "  Or point at one:  set LODESTAR_MCP_BIN",
    );
  }
  return path;
};

/** Drive the lodestar MCP server over newline-delimited JSON-RPC and read the ledger. */
const readLedger = () =>
  new Promise((resolve, reject) => {
    const proc = spawn(serverBinary(), [], { stdio: ["pipe", "pipe", "pipe"] });
    const pending = new Map();
    let nextId = 1;
    let stderr = "";

    const send = (method, params) => {
      const id = nextId++;
      return new Promise((settle, fail) => {
        pending.set(id, { settle, fail });
        proc.stdin.write(
          `${JSON.stringify({ jsonrpc: "2.0", id, method, params })}\n`,
        );
      });
    };

    proc.stderr.on("data", (chunk) => {
      stderr += chunk.toString();
    });
    proc.on("error", reject);
    proc.on("exit", (code) => {
      for (const { fail } of pending.values()) {
        fail(new Error(`lodestar-mcp exited (code ${code}) ${stderr.trim()}`));
      }
    });

    createInterface({ input: proc.stdout }).on("line", (line) => {
      let message;
      try {
        message = JSON.parse(line);
      } catch {
        return; // not a JSON-RPC frame; the server logs to stderr, but be tolerant
      }
      const waiter = pending.get(message.id);
      if (!waiter) return;
      pending.delete(message.id);
      if (message.error)
        waiter.fail(new Error(message.error.message ?? "MCP error"));
      else waiter.settle(message.result);
    });

    (async () => {
      await send("initialize", {
        protocolVersion: "2024-11-05",
        capabilities: {},
        clientInfo: { name: "design-audit", version: "1" },
      });
      proc.stdin.write(
        `${JSON.stringify({ jsonrpc: "2.0", method: "notifications/initialized", params: {} })}\n`,
      );
      const result = await send("tools/call", {
        name: "design_query",
        arguments: { view: "ledger" },
      });
      const text = result?.content?.[0]?.text;
      if (typeof text !== "string")
        throw new Error("list_designs returned no payload");
      return JSON.parse(text);
    })()
      .then(resolve, reject)
      .finally(() => proc.kill());
  });

const main = async () => {
  const files = readAdrFiles();
  const designs = await readLedger();
  const findings = auditDesigns(files, designs);

  console.log(
    `design-audit: ${files.length} ADR files, ${designs.length} ledger rows`,
  );
  for (const finding of findings) {
    console.log(
      `  ${finding.kind.padEnd(13)} ${finding.adr}  ${finding.detail}`,
    );
  }
  if (!findings.length) console.log("  files and ledger agree");

  if (findings.length && process.argv.includes("--check")) {
    console.error(`design-audit: ${findings.length} item(s) drifted`);
    process.exit(1);
  }
};

// Only run when invoked directly, so the comparison can be imported and tested.
if (
  process.argv[1] &&
  import.meta.url === pathToFileURL(process.argv[1]).href
) {
  main().catch((error) => {
    console.error(error.message);
    process.exit(2);
  });
}
