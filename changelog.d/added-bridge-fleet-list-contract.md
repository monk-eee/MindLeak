### Added

- Bridge's Fleet list is paginated, sortable, and filterable server-side
  (ADR-0112): `GET /api/v1/fleet` accepts `q` (substring search on
  repository id, with literal `%`/`_` escaped), `freshness`, `coordination`,
  `sort` (`field:asc|desc`, allow-listed), `page`, and `page_size` (clamped
  1-100), returning the true filtered total alongside each page. Previously
  the endpoint returned every enrolled repository unbounded, and the
  freshness/coordination filters only ever filtered an array already fully
  loaded into the browser. The Fleet UI gained a search box, sortable
  column headers, and a working pager.
