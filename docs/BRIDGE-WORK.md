# Bridge Work

The Bridge's `/work` page operates on an enrolled repository's published Work
records. It uses the existing tenant-scoped command API and its explicit
preview/confirmation contract. It does not create enrollment, adopt a
constitution, start a supervisor, or turn a process exit into proof of success.

## Work With Tasks

Select a repository, then select a task. The detail view shows acceptance,
declared scope, questions, and links to evidence and conformance. Task identifiers,
version, lease, and history remain available under the record disclosures.

- **Create task:** supply a title, acceptance criteria, a published objective,
  and file or symbol scope. Enter one scope item per line; blank lines and exact
  duplicates are removed before preview. The browser generates the task and
  idempotency identifiers. Goal selection uses the repository's active
  constitution, not a manually copied identifier or an automatically adopted goal.
- **Assign:** choose a current, advertised agent session. The page resolves its
  node/session target and refreshes the task version and active goals before
  preparing a command. Tasks without a currently published goal or declared
  scope cannot be assigned from this page.
- **Answer questions:** select a published unanswered question and provide the
  answer. A question answered in the meantime cannot be silently answered again.
- **Submit review:** record the review disposition and rationale. This submits
  a review; it does not bypass evidence/conformance or mark the task completed.
- **More actions:** routing and lease release use their existing version and
  ownership guards. Steer, pause, resume, drain, and checkpoint requirements are
  offered only against the capabilities a current supervisor actually advertises.

The browser never asks for a principal token. The Bridge derives an omitted
principal from its verified profile. Explicit client-supplied identities still
undergo the same authorization checks; this is not permission to expose the
loopback developer profile remotely.

## Preview And Confirm

Every mutation requires a reason and a separate confirmation. Once prepared,
the payload is immutable. A lost network response can be retried with the same
request identity and body. Switching repository or task invalidates the preview.
Expired or conflicted commands require a new preview, never an automatic retry
with a fresh task version.

The result keeps delivery distinct from execution:

| Result | Meaning |
| --- | --- |
| Ready for confirmation | No command effect has been requested yet. |
| Pending delivery | The directive is queued; the worker has not applied it. |
| Accepted | The recipient acknowledged the directive, not its completion. |
| Command applied | That command took effect; this is not a task-completion verdict. |
| Conflicted, expired, refused, or failed | The command did not establish success. |

Claims without published Work records remain visible under publication
diagnostics. They are never fabricated into tasks with inferred acceptance or
lifecycle state.

## Verification

```text
node --test scripts/bridge-work-command-client.test.mjs scripts/bridge-work-page.test.mjs
cargo test --locked -p ackplane-bridge --test work_command_browser_integration
cargo test --locked -p ackplane-bridge --test work_command_runtime_integration
```

The database tests require `ACKPLANE_TEST_DATABASE_URL` naming an isolated
`ackplane_test` database with the normal Ackplane migrations and pgvector.
An absent database variable is not verification. See the
[developer guide](../DEVELOPERS.md) for database-gated validation and the
[command API](../crates/ackplane-bridge/src/work_command_api/mod.rs) for the
authorization boundary.

The runtime test creates and confirms scoped work through real Bridge routes,
selects an actually registered supervisor session, assigns through the command
API, and runs an OS child through the authenticated supervisor. It checks the
prompt's goal and scope, durable lifecycle receipts, lease release, cleanup,
and the distinction between process exit, review submission, and task completion.
Its child is a deterministic fixture, not a vendor model or a login check.
