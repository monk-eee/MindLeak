- **The Bridge now shows the human decision queue, so an escalation reaches a
  person instead of sitting in the database (ADR-0115 item 5).** A new
  `/decisions` page and repository-scoped read API list the escalations an
  agent could not decide for itself, carrying what item 5 requires a reviewer
  to see: the proposed action and target, the reason, the context packet and
  evidence digests, the alternatives, the safe behavior while waiting, and the
  expiry. A request nobody answered before its expiry is shown as `expired`
  and never as an approval (item 6: no response is not an approval), and a
  resolved request stays visible with the principal and rationale that decided
  it (item 8). Reads are tenant- and repository-scoped like every other Bridge
  route. This slice is read-only: approving or refusing a request is a separate
  authorized command, not a page view.
