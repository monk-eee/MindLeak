// Ask each MCP binary what it actually writes, instead of guessing from dates.
//
// A stale server is not detectable from timestamps or from how far its checkout
// is behind `main`. Measured across ten worktrees: two that were only **5
// commits behind** wrote absolute node ids, while others 17 and 38 behind wrote
// correct ones. Every worktree was "behind on crates", so any threshold-based
// warning would have fired on all ten -- and a warning that always fires is one
// people learn to skip, which is how the original defect survived three days.
//
// What does separate them is behaviour. Node ids are repo-relative by contract
// (ADR-0038); a binary that predates that writes the absolute path it was given.
// So hand each binary one file by absolute path, against a throwaway database,
// and read the id it produces. No heuristics, no false positives, and no live
// data touched.
//
// Cross-platform, dependency-free Node (toolchain rule). Usage:
//   node scripts/mcp-build-probe.mjs [--check] [<binary> ...]
// With no binaries it probes every build under the sibling checkouts of this
// repository. `--check` exits non-zero when any binary is stale, for CI or a
// pre-flight before trusting a fleet-wide result.

import { execFileSync } from "node:child_process";
import { existsSync, mkdtempSync, readdirSync, writeFileSync } from "node:fs";
import { join, dirname, basename } from "node:path";
import { tmpdir } from "node:os";

import { callTools } from "./claim-gate.mjs";

const BINARY =
  process.platform === "win32" ? "mindleak-mcp.exe" : "mindleak-mcp";
const PROBE_SESSION = "00000000000000000000000000000001";

/**
 * What a returned node id says about the binary that wrote it.
 *
 * An id carrying a drive letter or a leading slash is the absolute path the
 * caller passed, echoed back — the binary never made it repo-relative.
 */
export function verdictFor(nodeIds) {
  if (!nodeIds?.length) {
    return "unknown";
  }
  const absolute = nodeIds.some((id) =>
    /^(artifact|symbol):([A-Za-z]:\/|\/)/.test(id),
  );
  return absolute ? "stale" : "current";
}

/** Every `mindleak-mcp` build under `root`'s sibling checkouts. */
export function binariesUnder(root, exists = existsSync, list = readdirSync) {
  const parent = dirname(root);
  const self = basename(root).split("-")[0];
  const found = [];
  for (const entry of list(parent)) {
    if (!entry.startsWith(self)) continue;
    for (const profile of ["release", "debug"]) {
      const candidate = join(parent, entry, "target", profile, BINARY);
      if (exists(candidate)) found.push(candidate);
    }
  }
  return found;
}

/** Hand one binary a file by absolute path and report what id it wrote. */
function probe(binary) {
  const scratch = mkdtempSync(join(tmpdir(), "mcp-probe-"));
  const file = join(scratch, "probe.rs");
  writeFileSync(file, "pub fn probe() {}\n", "utf8");

  const env = { ...process.env };
  // A throwaway database and workspace: probing must never touch the real graph.
  process.env.MINDLEAK_DB = join(scratch, "probe.db");
  process.env.MINDLEAK_WORKSPACE = scratch;
  try {
    const [, outcome] = callTools(binary, scratch, [
      { name: "open_session", arguments: { session_id: PROBE_SESSION } },
      {
        name: "ingest_file",
        arguments: {
          session_id: PROBE_SESSION,
          path: file,
          content: "pub fn probe() {}\n",
        },
      },
    ]);
    return {
      verdict: verdictFor(outcome?.node_ids),
      wrote: outcome?.node_ids?.[0],
    };
  } catch (error) {
    return { verdict: "unknown", wrote: String(error).split("\n")[0] };
  } finally {
    process.env = env;
  }
}

function main() {
  const args = process.argv.slice(2);
  const check = args.includes("--check");
  const explicit = args.filter((a) => !a.startsWith("--"));
  const repoRoot = execFileSync("git", ["rev-parse", "--show-toplevel"], {
    encoding: "utf8",
  }).trim();
  const binaries = explicit.length ? explicit : binariesUnder(repoRoot);

  if (!binaries.length) {
    console.log("mcp-build-probe: no mindleak-mcp builds found.");
    return;
  }

  let stale = 0;
  for (const binary of binaries) {
    const { verdict, wrote } = probe(binary);
    if (verdict === "stale") stale += 1;
    console.log(
      `  ${verdict.toUpperCase().padEnd(8)}${binary}\n           wrote: ${wrote ?? "(nothing)"}`,
    );
  }

  if (!stale) {
    console.log(`\nmcp-build-probe: ${binaries.length} build(s), none stale.`);
    return;
  }
  console.error(
    `\nmcp-build-probe: ${stale} of ${binaries.length} build(s) still write absolute node ids.`,
  );
  console.error(
    "Rebuild them from a checkout containing ADR-0038's repo-relative paths, or stop driving them.",
  );
  if (check) process.exit(1);
}

if (
  import.meta.url === `file://${process.argv[1]}` ||
  process.argv[1]?.endsWith("mcp-build-probe.mjs")
) {
  main();
}
