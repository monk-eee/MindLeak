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
    // stderr is ignored because several probes here are expected to fail: a ref
    // that does not exist, or a branch with no configured upstream. Letting git
    // print `fatal:` for a handled miss makes a healthy run look broken.
    return execFileSync("git", args, {
      encoding: "utf8",
      stdio: ["ignore", "pipe", "ignore"],
    }).trim();
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
const onIntegrationBranch = (number, slug) => {
  for (const ref of ["origin/main", "main"]) {
    const listing = capture([
      "ls-tree",
      "-r",
      "--name-only",
      ref,
      "--",
      `docs/adr/${number}-${slug}.md`,
    ]);
    if (listing.trim()) return true;
  }
  return false;
};

/** The refs this branch answers for: its own tip, and the upstream a push replaces. */
const ownRefs = () => {
  const refs = new Set();
  const branch = capture(["symbolic-ref", "--quiet", "--short", "HEAD"]);
  if (!branch) return refs;
  refs.add(branch);
  const upstream = capture([
    "rev-parse",
    "--abbrev-ref",
    "--symbolic-full-name",
    `${branch}@{upstream}`,
  ]);
  if (upstream) refs.add(upstream);
  // A branch published as `HEAD:refs/heads/<branch>` has a remote-tracking ref
  // but no configured upstream, so the same name on each remote counts too.
  for (const remote of capture(["remote"]).split("\n").filter(Boolean)) {
    refs.add(`${remote}/${branch}`);
  }
  return refs;
};

/**
 * Every ADR slug this change replaces: outright deletions, and the old name of
 * a rename.
 *
 * Both have to count, because `git mv` is the natural way to correct a filename
 * and git reports it as a rename rather than a delete. `--name-status` names the
 * old path second in both cases (`D\tpath`, `R100\told\tnew`), so one listing
 * answers for both.
 */
const stagedReplacements = () =>
  new Set(
    capture(["diff", "--cached", "-M", "--name-status", "--diff-filter=DR"])
      .split("\n")
      .map((line) => parseAdr(line.split("\t")[1] ?? ""))
      .filter(Boolean)
      .map((entry) => `${entry.number}-${entry.slug}`),
  );

const inHead = (number, slug) =>
  Boolean(
    capture([
      "ls-tree",
      "-r",
      "--name-only",
      "HEAD",
      "--",
      `docs/adr/${number}-${slug}.md`,
    ]).trim(),
  );

/**
 * Whether this branch has already replaced that slug.
 *
 * Retitling swaps `NNNN-old.md` for `NNNN-new.md`. Until the branch merges, the
 * old slug is still reachable from this branch and its upstream, so a ref scan
 * reads the decision as its own rival — from the index before the commit, and
 * from the very remote ref the push is about to replace afterwards. Blocking
 * either would make a retitle impossible without bypassing the guard, which is
 * the one thing a guard must never teach.
 *
 * The allowance stays narrow in both directions: a slug held on any ref this
 * branch does not answer for is a real rival, and a slug still standing in this
 * branch's own tree is a real duplicate.
 *
 * The one exception is a slug that has *landed*. A decision published as
 * `NNNN-old.md` is on main and on every branch cut from it, so the ownership
 * test can never pass and its filename could never be corrected — the guard's
 * own advice was to renumber an accepted decision, rewriting its identity and
 * every cross-link that cites it to fix a typo. Being on main is what makes
 * that slug this decision's former name rather than a rival: an unlanded ADR on
 * a sibling branch is not there, so a genuine collision is still refused, and a
 * renumber onto a landed decision is refused too because this branch has not
 * replaced that slug.
 */
const supersededByThisBranch = (adr, slug, refs, own, replacements) => {
  const replaced =
    replacements.has(`${adr.number}-${slug}`) || !inHead(adr.number, slug);
  if (!replaced) return false;
  if (onIntegrationBranch(adr.number, slug)) return true;
  return [...refs].every((ref) => own.has(ref));
};

const targets = process.argv.slice(2).length
  ? process.argv.slice(2)
  : capture(["diff", "--cached", "--name-only", "--diff-filter=A"]).split("\n");

const candidates = targets.map(parseAdr).filter(Boolean);
if (candidates.length === 0) process.exit(0);

const claimed = claimedAcrossRefs();
const own = ownRefs();
const replacements = stagedReplacements();
let conflicted = false;

for (const adr of candidates) {
  const bySlug = claimed.get(adr.number);
  if (!bySlug) continue;
  const rivals = [...bySlug.keys()].filter((slug) => slug !== adr.slug);
  const superseded = rivals.filter((slug) =>
    supersededByThisBranch(adr, slug, bySlug.get(slug), own, replacements),
  );
  const others = rivals.filter((slug) => !superseded.includes(slug));

  if (superseded.length > 0) {
    console.warn(
      `adr-number-guard: ADR-${adr.number} is retitled by this branch; ` +
        `${superseded.map((slug) => `${adr.number}-${slug}.md`).join(", ")} is replaced.`,
    );
  }
  if (others.length === 0) continue;

  if (onIntegrationBranch(adr.number, adr.slug)) {
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
