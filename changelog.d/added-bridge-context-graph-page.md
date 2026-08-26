### Added

- **Bridge renders the Context Graph.** A new `/graph` page draws the
  repository's projected memory-plane neighbourhood — the last capability the
  VS Code extension had and the standalone Bridge did not. The traversal
  endpoint (`GET /api/v1/repositories/:id/graph`, `Projector::bounded_neighborhood`,
  ADR-0087) already shipped; nothing rendered it, so the projected graph was
  reachable only as JSON. Nodes are typed and colour-coded across the full
  `NodeType` vocabulary, and selecting one lists its edges strongest-first and
  can re-seed the traversal from it, so the page explores the graph rather than
  showing one fixed picture.

  Edge stroke width *and* opacity are both derived from `effective_weight`, so
  decay is something an operator sees at a glance rather than a number they have
  to go looking for — a graph whose edges all render identically hides the one
  property the graph exists to express. The projection's ledger position and
  rebuild time are shown beside it, and "never projected" is reported distinctly
  from "projected at position 0", so a stale or absent projection stays legible.

  The renderer is a dependency-free SVG force layout rather than a vendored
  graph library: every other Bridge page is vanilla JS compiled into the binary
  with `include_str!`, and the endpoint is bounded to at most 300 nodes by
  design, so a library would have added weight the page cannot use. The page is
  read-only — the graph is derived from the ledger, so a mutation from here
  could only ever contradict its own source.

- **The graph legend is also its filter.** Each node type is a real
  `aria-pressed` button carrying its own count in the current view, so a type
  a reader can see named is one they can switch off — and hiding a type drops
  its edges with it. The node/edge counts then report what is drawn alongside
  what was returned (`16 nodes, 15 edges of 18/19`), so a filtered view never
  reads as a smaller projection. Types absent from the current answer offer no
  control at all.

- **Work links its declared scope into the graph.** A claim or task's
  `declared_paths` are already `artifact:` node ids, so the Work page now shows
  a Scope column that opens exactly those nodes in the Context Graph —
  answering "what surrounds what this task claims to touch" without retyping a
  node id. Declared symbols come along when they already carry their path;
  bare names are left out rather than guessed at, and a task declaring nothing
  gets no link. Work was already fetching `declared_paths` for tasks and
  discarding them; this is the first surface that shows them.

- The graph accepts `depth`, `max_nodes`, and `max_fanout` as query parameters
  alongside `repository` and `seeds`, so a deep link can hand over a focused
  view rather than the page's defaults.
