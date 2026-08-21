### Added

- Bridge exposes a fleet-wide Agents view (ADR-0105 decision 5's Agents/Work
  control room, first slice): `GET /api/v1/agents` returns one page of every
  live delegated claim across ALL of a tenant's enrolled repositories
  (`FleetStore::fleet_work`), not just one repository a caller already knows
  to ask about. Accepts `repository_id`/`owner_id` (substring search, `%`/`_`
  escaped), `sort` (`field:asc|desc`, allow-listed: `lease_expires_at`,
  `repository_id`, `owner_id`; defaults to soonest-expiring lease first),
  `page`, and `page_size` (clamped 1-100), returning the true filtered total
  alongside each page. The Fleet page gained an "Agents" section listing
  repository, agent, task, branch, lease expiry, and declared scope for
  every active claim fleet-wide, with its own search, sortable columns, and
  pager — an operator no longer has to open each repository's detail panel
  in turn to see who is working on what right now.
