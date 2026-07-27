// ADR number guard (ADR-0038). Refuses a new `docs/adr/NNNN-*.md` whose number
// is already claimed under a different title on any other ref.
//
// ADR numbers are a shared counter with no coordination: every concurrent agent
// reads "the next number" from its own branch, which cannot see a sibling
// branch's in-flight ADR. Two agents pick the same number, and the collision
// only surfaces at merge — by which point both ADRs are written, cross-linked,
// and cited in commit messages. Renumbering afterwards is pure waste; this
// repository has already spent two commits on exactly that.
//
// Reading every ref instead of just the working tree is the whole point: the
// conflict lives in the branch you cannot see.
//
// Platform-agnostic: git + node only. Usage:
//   node scripts/adr-number-guard.mjs [<file> ...]
// With no arguments it checks the staged set, which is what the hook does.

import { execFileSync } from "node:child_process";

const ADR_PATH = /^docs\/adr\/(\d{4})-(.+)\.md$/;

const capture = (args) => {
  try {
    return execFileSync("git", args, { encoding: "utf8" }).trim();
  } catch {
    return "";
  }
};

const parseAdr = (path) => {
  const match = ADR_PATH.exec(path.replace(/\\/g, "/").trim());
  return match ? { number: match[1], slug: match[2] } : null;
};

/** Every ADR number in use on any ref, mapped to the titles that claim it. */
const claimedAcrossRefs = () => {
  const refs = capture([
    "for-each-ref",
    "--format=%(refname:short)",
    "refs/heads",
    "refs/remotes",
  ])
    .split("\n")
    .filter(Boolean);

  const claimed = new Map();
  for (const ref of refs) {
    const listing = capture([
      "ls-tree",
      "-r",
      "--name-only",
      ref,
      "--",
      "docs/adr",
    ]);
    if (!listing) continue;
    for (const line of listing.split("\n")) {
      const adr = parseAdr(line);
      if (!adr) continue;
      if (!claimed.has(adr.number)) claimed.set(adr.number, new Map());
      const bySlug = claimed.get(adr.number);
      if (!bySlug.has(adr.slug)) bySlug.set(adr.slug, new Set());
      bySlug.get(adr.slug).add(ref);
    }
  }
  return claimed;
};

const nextFreeNumber = (claimed, wanted) => {
  let candidate = Number.parseInt(wanted, 10);
  while (claimed.has(String(candidate).padStart(4, "0"))) candidate += 1;
  return String(candidate).padStart(4, "0");
};

/**
 * Whether this exact ADR already exists on the integration branch.
 *
 * If it does, it won the number by landing, and the other claimant is the one
 * that has to move. Blocking here would punish the winner — and worse, it would
 * fire on every merge commit that carries the landed ADR back into a branch,
 * making the guard impossible to satisfy without bypassing it. Asked in
 * anger, that is exactly what a guard must not do.
 */
const alreadyOnMain = (adr) => {
  for (const ref of ["origin/main", "main"]) {
    const listing = capture([
      "ls-tree",
      "-r",
      "--name-only",
      ref,
      "--",
      `docs/adr/${adr.number}-${adr.slug}.md`,
    ]);
    if (listing.trim()) return true;
  }
  return false;
};

const targets = process.argv.slice(2).length
  ? process.argv.slice(2)
  : capture(["diff", "--cached", "--name-only", "--diff-filter=A"]).split("\n");

const candidates = targets.map(parseAdr).filter(Boolean);
if (candidates.length === 0) process.exit(0);

const claimed = claimedAcrossRefs();
let conflicted = false;

for (const adr of candidates) {
  const bySlug = claimed.get(adr.number);
  if (!bySlug) continue;
  const others = [...bySlug.keys()].filter((slug) => slug !== adr.slug);
  if (others.length === 0) continue;

  if (alreadyOnMain(adr)) {
    console.warn(
      `adr-number-guard: ADR-${adr.number} is contested, but ${adr.number}-${adr.slug}.md ` +
        `is already on main, so the other claimant must renumber:`,
    );
    for (const slug of others) {
      const refs = [...bySlug.get(slug)].sort().join(", ");
      console.warn(`  ${adr.number}-${slug}.md  on ${refs}`);
    }
    continue;
  }

  conflicted = true;
  console.error(
    `adr-number-guard: ADR-${adr.number} is already claimed by a different decision.`,
  );
  console.error(`  yours:  ${adr.number}-${adr.slug}.md`);
  for (const slug of others) {
    const refs = [...bySlug.get(slug)].sort().join(", ");
    console.error(`  theirs: ${adr.number}-${slug}.md  on ${refs}`);
  }
  console.error(
    `\n  Renumber to ADR-${nextFreeNumber(claimed, adr.number)} before this lands.` +
      "\n  Fixing it after the merge means rewriting the file, its cross-links, and" +
      "\n  every commit message that cites it.\n",
  );
}

process.exit(conflicted ? 1 : 0);
