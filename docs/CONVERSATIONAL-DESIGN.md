# Conversational Design

Use Copilot, with the model you choose, to clarify a request and turn it into
a server-backed design. Bridge remains the operational view; its web form is
not required to write requirements or proposals.

## Setup

Follow the [Industrial quickstart](INDUSTRIAL-QUICKSTART.md) to run Ackplane and
Bridge, enroll the repository's node companion, and connect `ackplane-mcp` to
Copilot. `make install-industrial` installs both `ackplane-mcp` and
`ackplane-workctl`; make the install directory available on `PATH`.

This workflow needs an MCP client that supports prompts and an agent that can
write a proposal file and run the operator CLI with the developer's permission.
It uses the existing loopback, self-hosted Bridge profile. It is not a remote
Bridge authentication mechanism. No model name, provider, API key, or backend
LLM call is configured by the workflow.

## Start in Copilot

Select the `design_workflow` prompt supplied by `ackplane-mcp`. Provide:

| Argument        | Value                                                                  |
| --------------- | ---------------------------------------------------------------------- |
| `request`       | What you want to build, investigate, or change.                        |
| `bridge_url`    | Your explicit loopback Bridge origin, such as `http://127.0.0.1:3000`. |
| `repository_id` | The existing Industrial repository ID, not a guessed folder name.      |

For example: "Make receipts survive reconnects. Keep two workers' evidence
separate, and prove that retrying does not execute the work again."

Copilot inspects relevant code and available records, asks about material
uncertainties, and drafts the problem, decision, alternatives, constraints,
scope, risks, and observable acceptance criteria. The original request remains
in the proposal. Unavailable server context is reported as unknown, never
silently replaced with Local-plane data.

The MCP prompt itself performs no reads or writes against Ackplane or Bridge.
It reports any startup refusal alongside its offline guidance. Existing
enrolled-node tool authority is unchanged: the agent uses `ackplane-workctl`
as an explicitly permitted local operator client, not a hidden MCP write tool.

## Review and Persist

Copilot writes a UTF-8 JSON proposal, for example:

```json
{
  "design_id": "design:receipt-reconnect-v1",
  "title": "Preserve receipts across reconnects",
  "summary": "## Original request\nKeep two workers' receipts separate across reconnects.\n\n## Decision\nRetry the original durable receipt.\n\n## Acceptance\nA lost acknowledgement does not lose evidence or repeat execution.",
  "source_version": "conversation-v1"
}
```

Those four fields are required. Optional fields are
`constitution_version_id`, `work_task_id`, `evidence_id`, and `display_label`.
References must already exist in the same tenant and repository. The complete
file is limited to 64 KiB, with a nonblank summary of at most 60 KiB. Caller
identity and lifecycle fields are rejected; Bridge supplies its verified
operator principal. A display label is not identity.

The agent runs these commands on your behalf:

```text
ackplane-workctl design preview --bridge-url http://127.0.0.1:3000 --repository-id repository:example --file proposal.json
ackplane-workctl design propose --bridge-url http://127.0.0.1:3000 --repository-id repository:example --file proposal.json --confirm-digest <reviewed-digest>
ackplane-workctl design show --bridge-url http://127.0.0.1:3000 --repository-id repository:example --design-id design:receipt-reconnect-v1
```

Preview is offline. It returns the exact proposal, target, and a confirmation
digest. Publication requires that digest and a separate explicit instruction
to publish the reviewed proposal. The CLI then reads the stored record back.
Changed content or a different target needs a new preview. A repeated ID with
different content is a conflict, not an update; revisions need a distinct ID
and source version. The summary should name the proposal they replace.

An identical proposal retry is idempotent. After a timeout, retain the same
file and identity instead of creating a second design. HTTP errors and failed
read-back never become a success claim. Redirects are refused, and requests
and response bodies are bounded.

## Adopt and Hand Off

Publishing is not adoption. Copilot uses `design decision-preview` to show the
current record and a proposed lifecycle decision with its rationale. After
explicit approval, `design decide` submits that exact decision and digest.
The existing Bridge compare-and-swap checks the observed lifecycle state.
Changed records require another preview; an uncertain decision write requires
`design show` before retrying.

For an accepted design, Copilot proposes bounded tasks with explicit acceptance
criteria and file/symbol scope. The existing `submit create_work` command
records a `pending_confirmation` command. Only `confirm create_work`, with the
returned command ID and identical payload, creates Work. The CLI lets Bridge
resolve an omitted operator principal; a supplied forged principal is still
refused. Always inspect the returned `status` and `outcome`: Work refusals are
typed JSON results, including when the process exit code is zero.

After Work exists, `design materialization-preview` reviews the link from the
design to an existing constitution publication and one or more Work tasks.
`design materialize` records that link only with the matching confirmation
digest and a stable idempotency key. It creates neither tasks nor execution
authority. Identical retries return the same revision, and task-reference
ordering is canonical. `design show` reads the resulting history.

Run `ackplane-workctl help` for each command's complete flags. The agent can
perform the authoring and confirmation sequence without a requirements web
form. [Bridge Work](BRIDGE-WORK.md) documents the subsequent operator actions.

## Boundaries

- Confirmation digests bind reviewed content and target. They do not prove
  human approval, provide two-party authentication, or sandbox an agent with
  local operator access. Conversational approval remains an explicit user act.
- Work creation, assignment, execution, review, and completion are separate.
  Dispatch requires an authorized Work command and observed node/session IDs
  and task version. Neither a proposed design nor this prompt starts a worker.
- Existing ContextPacket compilation supplies worker identity, authority,
  policy, scope, acceptance, evidence requirements, and bounded memory. Prior
  observed outcomes can inform later prompts; this is not model-weight training
  and memory never overrides authority.
- Concurrent materializations of the same design serialize within one
  transaction. Identical requests replay one receipt, distinct requests receive
  unique revisions, and changed content under the same key is a typed conflict.
  Locks are scoped to tenant, repository, and design. Failed references and
  cancelled writers roll back; unrelated designs remain writable. A refused or
  uncertain write is still not evidence of recorded provenance.

## Verification

Real-stdio tests exercise MCP discovery and prompt retrieval. CLI tests cover
offline preview, input and target validation, confirmation binding, redirects,
HTTP failures, and uncertain read-back. The PostgreSQL integration drives the
built CLI against real Bridge handlers: proposal, retry, explicit adoption,
two separately confirmed tasks, and an idempotent design/Work/constitution
link. It also checks tenant isolation and forged-principal refusal. Controlled
PostgreSQL lock tests exercise eight concurrent writers, typed conflicts,
cancellation, rollback, one-connection pools, and independent scopes.

```text
cargo test -p ackplane-mcp -p ackplane-workctl --locked
cargo test -p ackplane-server --lib design_materialization_store --locked
```

Set `ACKPLANE_TEST_DATABASE_URL` to a disposable PostgreSQL database for the
database tests. An unset gate reports a skip, not a verified server round trip.
