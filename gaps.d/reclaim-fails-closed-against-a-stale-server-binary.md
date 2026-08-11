- **`worktree-reclaim` fails closed against a stale server binary, and a
  refusal that looks like caution is never investigated — OBSERVED 2026-08-11,
  left OPEN.** The same command, in the same repository, in the same minute,
  gave two different answers depending only on which Lodestar binary it
  resolved:

  | `LODESTAR_MCP_BIN` | Result for `feat/certification-status` |
  |---|---|
  | `MindLeak/target/release/lodestar-mcp.exe` (stale) | `keep — held by a live Lodestar claim`, 0 reclaimable |
  | `~/.mindleak/bin/lodestar-mcp.exe` (shared install) | `reclaim — 2.46 GiB` |

  There was no live claim. The covering task `task:7fe8a3f0b7ae` was `done`
  with `owner: null` and `lease_expires_at: null`, its pull request was merged,
  and the only other task naming that branch was `abandoned`.
  `scripts/worktree-reclaim.mjs` resolves its server the same way
  `scripts/claim-gate.mjs` does — `LODESTAR_MCP_BIN`, else
  `target/{release,debug}` — so a checkout carrying an old build silently reads
  a ledger it cannot parse and reports the wrong answer.

  The live-claim check itself is deliberate and correct (see the earlier
  `fix/worktree-reclaim-honors-live…` work in the graph). The defect is that
  its input can be wrong without saying so. `scripts/canonical-push.mjs`
  already detects this exact condition and refuses with a precise message —
  "the deployed binary is almost certainly older than the ledger it is reading"
  — naming the shared install as the fix. Reclaim performs no such check.

  Impact is high and slow. Failing closed reads as correct caution, so nobody
  investigates a `keep`: the tool appears to be doing its job while reclaiming
  nothing. This repository has already reached 88 worktrees and 82,891 files
  once, which is the collapse this tool exists to prevent, and a single
  worktree here held 2.46 GiB of build output. A permanently silent reclaim is
  how it gets back there.

  Left for later. The fix is small — reuse the staleness detection
  `canonical-push` already has, and make an unreadable board a loud refusal
  rather than a `keep` — but it changes what a `keep` line means to every
  caller and to the scheduled use the tool was written for, so it wants review
  rather than a drive-by. Observed while reclaiming a merged worktree.
