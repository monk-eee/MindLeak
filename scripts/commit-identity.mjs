// Commit-identity guard. A commit records who did the work, and the whole
// evidence chain leans on that: `git blame`, the merge commits conformance
// derives from, and ADR-0038's rule that a commit id is evidence-bearing. An
// author that is not a real person breaks all of it silently, because git is
// perfectly happy to commit under any identity and nothing downstream checks.
//
// This is not hypothetical. On 2026-07-30 the shared repository config held
// user.email "sha@example.invalid": 204 commits on main carried it before
// anyone noticed, across every linked worktree, because a repository-level
// override outranks the correct global identity and the common dir is shared by
// every worktree. It was the second occurrence -- the same contamination was
// cleaned up once before and came back.
//
// Where it comes from: `git config user.email x@example.invalid` writes the
// LOCAL repository config by default, so a git fixture whose GIT_DIR or
// GIT_COMMON_DIR still points at the real repository configures the real
// repository. The committed test suites all scrub those variables already. Ad
// hoc scratch scripts under target/ do not, and never will reliably, which is
// why this check lives at commit time rather than in a test.
//
// One narrow rule, so the guard fires on real problems and stays quiet
// otherwise: the address must not sit in a domain the IETF reserves for
// documentation and testing (RFC 2606, RFC 6761). Those can never receive mail,
// so they are never a real contributor -- which makes this unambiguous rather
// than a heuristic about what a name should look like.
//
// Platform-agnostic: node and git only. Usage:
//   node scripts/commit-identity.mjs

import { execFileSync } from "node:child_process";
import { pathToFileURL } from "node:url";

// Reserved by RFC 2606 / RFC 6761. Never deliverable, so never a contributor.
const RESERVED_TLDS = ["invalid", "test", "example", "localhost"];
const RESERVED_DOMAINS = ["example.com", "example.net", "example.org"];

/// Whether an address belongs to a domain reserved for documentation and
/// testing. Pure, so the interesting cases are testable without a repository.
export function isReservedAddress(email) {
  if (typeof email !== "string") return false;
  const at = email.lastIndexOf("@");
  if (at === -1) return false;
  const domain = email
    .slice(at + 1)
    .trim()
    .toLowerCase()
    .replace(/\.$/, "");
  if (domain === "") return false;
  if (RESERVED_DOMAINS.includes(domain)) return true;
  const tld = domain.slice(domain.lastIndexOf(".") + 1);
  return RESERVED_TLDS.includes(tld);
}

/// The identity git would actually use, honouring the environment overrides
/// that outrank configuration. Returns the address and where it came from, so
/// the refusal can name the right place: an env override does NOT live in the
/// config file `git config --show-origin` would report, and naming that file
/// would send the reader editing an innocent one. Returns null when nothing is
/// configured; that is git's own error to report, not this guard's.
export function effectiveEmail(env, configured) {
  const override = env.GIT_AUTHOR_EMAIL ?? env.EMAIL;
  if (typeof override === "string" && override.trim() !== "") {
    const source = env.GIT_AUTHOR_EMAIL ? "GIT_AUTHOR_EMAIL" : "EMAIL";
    return {
      email: override.trim(),
      source: "environment",
      origin: `the ${source} environment variable`,
    };
  }
  const email = (configured ?? "").trim();
  return email === "" ? null : { email, source: "config", origin: null };
}

/// The message shown when the guard refuses, kept here so a test can pin that
/// it names the offending address and how to clear it.
export function refusal(email, origin) {
  return [
    `commit-identity: refusing to commit as ${email}`,
    origin ? `  that address is configured in ${origin}` : null,
    "  It is a domain reserved for testing (RFC 2606), so it is not a real",
    "  contributor. It usually arrives from a git fixture that wrote the real",
    "  repository's config because GIT_DIR/GIT_COMMON_DIR were still inherited.",
    "",
    "  Clear the override so your global identity applies again:",
    "    git config --local --unset user.email",
    "    git config --local --unset user.name",
  ]
    .filter((line) => line !== null)
    .join("\n");
}

const gitValue = (args) => {
  try {
    return execFileSync("git", args, { encoding: "utf8" }).trim();
  } catch {
    return null;
  }
};

function main() {
  const identity = effectiveEmail(
    process.env,
    gitValue(["config", "--get", "user.email"]),
  );
  if (identity === null || !isReservedAddress(identity.email)) return 0;

  let origin = identity.origin;
  if (identity.source === "config") {
    const reported = gitValue([
      "config",
      "--show-origin",
      "--get",
      "user.email",
    ]);
    origin = reported ? reported.split(/\s+/)[0] : null;
  }
  console.error(refusal(identity.email, origin));
  return 1;
}

if (import.meta.url === pathToFileURL(process.argv[1] ?? "").href) {
  process.exit(main());
}
