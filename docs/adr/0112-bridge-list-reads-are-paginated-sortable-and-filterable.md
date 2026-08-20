# ADR-0112: Bridge list reads are paginated, sortable, and filterable server-side

- Status: Accepted
- Date: 2026-08-21
- Deciders: MindLeak maintainers
- Depends on: [ADR-0095](0095-the-bridge-uses-an-authenticated-projection-api.md)
  (the Bridge's read API), [ADR-0105](0105-bridge-is-the-server-version-of-the-vsix.md)
  (the Bridge is the server form of the VSIX, for fleets larger than one
  developer's SQLite files)
- Related: [ADR-0111](0111-bridge-recovers-a-stranded-claim-as-a-tenant-scoped-administrative-action.md)
  (Bridge's first claim mutation; this ADR is Bridge's first read-scaling
  decision)

## Context

Every Bridge list read shipped so far either has no bound at all or a fixed,
silent one:

- `GET /api/v1/fleet` (`FleetStore::repositories`) returns **every** enrolled
  repository for a tenant in one response, unbounded. A tenant with a few
  repositories never notices; a tenant with thousands gets one unbounded
  Postgres result set and one unbounded JSON payload on every page load.
- `GET /api/v1/repositories/:id/timeline`, `/claims`, and `/knowledge` each
  cap at a fixed 50 items (`TIMELINE_LIMIT`, `ACTIVE_WORK_LIMIT`,
  `KNOWLEDGE_LIMIT`) with no way to see item 51. Each constant's own comment
  already says why: "ADR-0095 does not yet define a paging contract."

Sorting and filtering exist today only as a client-side illusion: the Fleet
page's freshness/coordination buttons filter an array already sitting in the
browser, which only works because the array is currently unbounded. The
moment `/api/v1/fleet` is paginated, client-side filtering silently breaks —
a filter would only ever see whichever page happened to load, not the whole
fleet.

This is exactly the gap ADR-0105 named directly: Bridge is the *server* form
of the VSIX, for fleets and organisations larger than one developer's local
SQLite files. A single-workspace VSIX view never needed paging, sorting, or
a search box, because it never has to hold more than one repository's data.
Bridge's whole reason to exist is holding many repositories' data at once,
which is precisely the case that breaks an unbounded, client-filtered list.

## Decision

**Bridge list reads gain a shared, explicit contract: bounded results, an
allow-listed sort, and server-side filtering — chosen per view by how its
underlying data grows, not applied identically everywhere.**

1. **Two pagination shapes, chosen by growth pattern, not one blanket
   scheme.** A repository's enrolment (the Fleet list) grows rarely and is
   bounded by organisation size; an accepted ledger stream (the timeline)
   grows continuously and without bound. Page-number pagination
   (`page`/`page_size`, translated to `LIMIT`/`OFFSET`) is the right fit for
   the former — simple, and "the list shifted by one row between two page
   loads" is a rare, low-stakes event for a slowly-changing enrolment list.
   Keyset/cursor pagination ordered by a stable monotonic column
   (`stream_position` for the timeline; a compound key for claims/knowledge)
   is the right fit for the latter, because `OFFSET` against a continuously
   appended table skips or repeats rows as new records land between two page
   requests — a real, not theoretical, drift for a live ledger.
   This ADR implements the first shape now, for the Fleet list. It commits
   the second shape as the *decided* pattern for `timeline`, `claims`, and
   `knowledge` adopting keyset pagination on their existing ordering columns
   when each is next touched — not a redesign to invent later, just not
   built in the same change as this one.
2. **`GET /api/v1/fleet` accepts `q`, `freshness`, `coordination`, `sort`,
   `page`, and `page_size`, all optional.** `q` substring-matches
   `repository_id` (case-insensitive, with literal `%`/`_` in the query
   escaped so a repository named `my_repo` is not treated as a one-character
   wildcard match). `freshness` and `coordination` are the same predicates
   the client-side buttons already apply today, moved server-side so they
   compose correctly with pagination. `sort` is `field:asc` or `field:desc`
   drawn from an allow-list (`repository_id`, `active_node_count`,
   `last_activated_at`); an unrecognised field, direction, or malformed
   pair is a `400`, never a silently-ignored value. `page_size` is clamped
   to `[1, 100]`; `page` below `1` is a `400`.
3. **The ORDER BY clause is chosen from a fixed, compiled-in set of literal
   SQL fragments, matched from the parsed, allow-listed sort field and
   direction — never built by interpolating the client's string into SQL.**
   `q`, `freshness`, and `coordination` remain real bound parameters compared
   against literal values inside the query (`$3 = 'lagging' AND ...`); no
   client-supplied text ever becomes part of the query's structure, matching
   this repository's existing parameterized-SQL discipline everywhere else.
4. **The response reports the true filtered total, not just the current
   page's length,** via a `COUNT(*) OVER()` window function evaluated after
   the same `GROUP BY`/`HAVING` filtering — one round trip, not two — so the
   UI can show "showing 21-40 of 214" and disable "next" correctly instead of
   guessing from a page that might be full by coincidence.
5. **The Fleet UI moves filtering and sorting from the browser to the
   server.** The freshness/coordination buttons and a new search box now
   trigger a re-fetch instead of re-filtering an in-memory array; clicking a
   sortable column header re-fetches with the new `sort` value; a page
   footer reports the true range and total and disables "Previous"/"Next" at
   the real boundaries.

## Consequences

- A tenant with a very large fleet gets a bounded, responsive Fleet page for
  the first time; today's unbounded query is the one thing standing between
  "works fine in every demo" and "the first real customer's page never
  finishes loading."
- `timeline`, `claims`, and `knowledge` remain at their existing fixed
  50-item cap with no way to page further, which is exactly the standing gap
  already named for `claims` specifically (the Bridge cannot list a stranded
  claim to recover, gaps.d, still pending on PR #573 at the time of writing).
  This ADR gives that follow-up work a named, decided pagination shape
  (keyset, not `page`/`page_size`) rather than leaving it to invent one from
  scratch.
- Any future Bridge list view chooses one of these two named shapes at
  design time based on its own data's growth pattern, rather than each view
  inventing its own bespoke limit-and-hope-nobody-asks-for-page-2 pattern the
  way `TIMELINE_LIMIT`/`ACTIVE_WORK_LIMIT`/`KNOWLEDGE_LIMIT` did.

## Rejected alternatives

**One universal cursor-based contract for every list, including the Fleet
list.** Rejected because keyset pagination against a `GROUP BY` aggregate
query (the Fleet list's `count(*)`/`max(...)` columns) needs the cursor
comparison to reference the same aggregate expressions the `ORDER BY` uses,
which is solvable but meaningfully more complex than `OFFSET` for a list that
does not have the append-only stream's drift problem in the first place.
Paying that complexity cost for a list bounded by "how many repositories a
tenant enrols" is optimizing for a growth pattern the Fleet list does not
have.

**A generic filter query language (e.g. a small DSL like `freshness=lagging
AND active_node_count>0`).** Rejected as solving a problem nobody has asked
for yet. Named, single-purpose filter parameters cover every case the
current UI needs and stay trivially safe to validate; a DSL needs its own
parser, its own injection surface to reason about, and its own UI to build a
query with, for filters this view does not yet need.

**Let the client choose the `ORDER BY` column and direction as raw SQL
fragments (with server-side validation only against a blocklist).**
Rejected outright — a blocklist is a promise to enumerate every dangerous
string forever and get it wrong once. The allow-listed, compiled-in
fragment set means an invalid `sort` value cannot reach the query at all,
not merely "is checked before it does."
