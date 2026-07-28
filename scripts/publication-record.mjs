// Record a publication in the Memory Plane (ADR-0009 evidence).
//
// The claim gate already refuses to publish without a live claim, so every
// push arrives here knowing what the work was for. Until now it recorded that
// nowhere: `evidence_for` returned an empty bundle for work that had just been
// validated and published through the gate, `check_conformance` answered
// `needs_human` on absent evidence, and `complete_task` refused. Measured on
// this repository, 18 of 21 human-blocked tasks stopped for exactly that
// reason - one defect wearing eighteen hats.
//
// The push is the right moment: it is the point where a commit stops being a
// draft and becomes a fact about the world, and it is already the one path
// where the ledger is not optional. Recording here makes evidence a
// by-product of publishing rather than a separate discipline nobody remembers.

import { callTools, resolveServer } from "./claim-gate.mjs";

/**
 * The `ingest_commit` arguments for a published head.
 *
 * Pure, so the shape of what gets recorded is testable without a graph: the
 * interesting failure is a bundle that carries no changed files, which reads
 * as "no provenance-bearing mutation" and is indistinguishable from never
 * having recorded anything at all.
 */
export const publicationRecord = ({
  sessionId,
  sha,
  message,
  changedFiles,
  timestamp,
}) => ({
  session_id: sessionId,
  sha,
  message,
  changed_files: changedFiles ?? [],
  timestamp,
});

/**
 * Ingest the just-published commit, returning a short notice to print or
 * `null` when there is nothing to say.
 *
 * Never throws. The commit is already on the remote by the time this runs, so
 * a failure here cannot un-publish it -- turning an unreachable graph into a
 * failed push would trade a missing record for a broken publisher.
 */
export const recordPublication = ({
  repoRoot,
  sessionId,
  sha,
  message,
  changedFiles,
  timestamp = Math.floor(Date.now() / 1000),
}) => {
  const server = resolveServer(repoRoot, "mindleak");
  if (!server || !/^[0-9a-f]{32}$/.test(sessionId ?? "")) {
    return "published commit not recorded; the Memory Plane was unreachable, so this work will not certify";
  }
  try {
    callTools(server, repoRoot, [
      { name: "open_session", arguments: { session_id: sessionId } },
      {
        name: "ingest_commit",
        arguments: publicationRecord({
          sessionId,
          sha,
          message,
          changedFiles,
          timestamp,
        }),
      },
    ]);
  } catch {
    return "published commit not recorded; the Memory Plane rejected the write, so this work will not certify";
  }
  return null;
};
