// Prepare an explicit completion offer at the publication boundary (ADR-0065).
//
// Publishing already happened before this module is called. Every failure here
// is therefore silence, never a failed push: an unavailable evidence plane, an
// empty bundle, an expired claim, or an undecidable check changes nothing about
// what reached the remote.

import { mkdirSync, writeFileSync } from "node:fs";
import { join, relative, sep } from "node:path";

import { callTools, resolveServer } from "./claim-gate.mjs";

const SESSION_TOKEN = /^[0-9a-f]{32}$/;
const FINDINGS_PREVIEW_ITEMS = 3;
const FINDINGS_PREVIEW_BYTES = 2_048;

const boundedFindings = (findings) => {
  const preview = [];
  let usedBytes = 0;
  let truncated = false;

  for (const finding of findings) {
    if (
      preview.length === FINDINGS_PREVIEW_ITEMS ||
      usedBytes === FINDINGS_PREVIEW_BYTES
    ) {
      truncated = true;
      break;
    }
    let delivered = "";
    for (const character of String(finding)) {
      const characterBytes = Buffer.byteLength(character, "utf8");
      if (usedBytes + characterBytes > FINDINGS_PREVIEW_BYTES) {
        truncated = true;
        break;
      }
      delivered += character;
      usedBytes += characterBytes;
    }
    if (delivered.length > 0 || String(finding).length === 0)
      preview.push(delivered);
    if (delivered !== String(finding)) break;
  }

  return {
    preview,
    truncated: truncated || preview.length < findings.length,
  };
};

const compactCheck = (check) => {
  const sourceFindings = Array.isArray(check.findings_preview)
    ? check.findings_preview
    : Array.isArray(check.findings)
      ? check.findings
      : [];
  const findingCount = Number.isInteger(check.finding_count)
    ? check.finding_count
    : sourceFindings.length;
  const bounded = boundedFindings(sourceFindings);
  return {
    check: { id: check.id, token: check.token },
    conformance: {
      verdict: check.verdict,
      finding_count: findingCount,
      findings_preview: bounded.preview,
      findings_truncated:
        check.findings_truncated === true ||
        findingCount > sourceFindings.length ||
        bounded.truncated,
    },
  };
};

/**
 * Assemble the exact objects an agent may explicitly submit to complete_task.
 *
 * This function deliberately cannot submit them. ADR-0065 makes the boundary
 * an offer, not an automatic transition: publishing establishes that the work
 * exists; only the agent may attest that it is complete.
 *
 * An agent may hold several claims. Until claims record their task branch, one
 * publication cannot safely be assigned among them, so ambiguity declines the
 * offer silently rather than guessing which task the commit served.
 */
export const prepareCompletionOffer = ({
  repoRoot,
  sessionId,
  claims,
  lodestarServer,
  endedAt = Math.floor(Date.now() / 1000),
  resolveServerFn = resolveServer,
  callToolsFn = callTools,
}) => {
  if (!SESSION_TOKEN.test(sessionId ?? "") || claims?.length !== 1) return null;

  const claim = claims[0];
  if (
    !claim?.id ||
    !Number.isInteger(claim.claim_started_at) ||
    claim.claim_started_at > endedAt ||
    !Number.isInteger(claim.lease_expires_at) ||
    claim.lease_expires_at < endedAt
  ) {
    return null;
  }

  try {
    const mindleakServer = resolveServerFn(repoRoot, "mindleak");
    const intentServer =
      lodestarServer ?? resolveServerFn(repoRoot, "lodestar");
    if (!mindleakServer || !intentServer) return null;

    const [, evidence] = callToolsFn(mindleakServer, repoRoot, [
      { name: "open_session", arguments: { session_id: sessionId } },
      {
        name: "evidence_for",
        arguments: {
          session_id: sessionId,
          task_id: claim.id,
          started_at: claim.claim_started_at,
          ended_at: endedAt,
        },
      },
    ]);
    if (!evidence || typeof evidence !== "object") return null;
    const evidenceCounts = [
      evidence.changed_node_ids,
      evidence.execution_ids,
      evidence.commit_ids,
      evidence.provenance,
    ];
    if (
      !evidenceCounts.some(
        (values) => Array.isArray(values) && values.length > 0,
      )
    ) {
      return null;
    }

    const [, check] = callToolsFn(intentServer, repoRoot, [
      { name: "open_session", arguments: { session_id: sessionId } },
      {
        name: "check_conformance",
        arguments: {
          session_id: sessionId,
          task_id: claim.id,
          evidence,
        },
      },
    ]);
    if (
      !check ||
      typeof check !== "object" ||
      !Number.isInteger(check.id) ||
      typeof check.token !== "string" ||
      typeof check.verdict !== "string"
    ) {
      return null;
    }

    const compact = compactCheck(check);
    return {
      schema: 2,
      task_id: claim.id,
      evidence,
      ...compact,
    };
  } catch {
    return null;
  }
};

/** Persist the offer outside Git so the exact check/evidence can be submitted. */
export const persistCompletionOffer = ({
  repoRoot,
  offer,
  mkdir = mkdirSync,
  write = writeFileSync,
}) => {
  if (!offer) return null;
  try {
    const directory = join(repoRoot, "target", "completion-offers");
    mkdir(directory, { recursive: true });
    const task = offer.task_id.replace(/[^a-zA-Z0-9._-]/g, "-");
    const destination = join(directory, `${task}.json`);
    write(destination, `${JSON.stringify(offer, null, 2)}\n`, "utf8");
    return relative(repoRoot, destination).split(sep).join("/");
  } catch {
    return null;
  }
};

/** One bounded notice. Ignoring it is the whole decline path. */
export const completionOfferNotice = (offer, path) =>
  `completion is ready for ${offer.task_id} (${offer.conformance.verdict})\n` +
  `  exact evidence + check: ${path}\n` +
  '  submit explicitly with task_transition(task_id, to="complete", evidence, check, learned?)\n' +
  "  or ignore this offer; the push has already succeeded";
