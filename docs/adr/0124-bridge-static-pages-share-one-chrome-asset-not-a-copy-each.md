# ADR-0124: Bridge static pages share one chrome asset, not a copy each

- Status: Accepted
- Date: 2026-08-24
- Deciders: MindLeak maintainers
- Accepted: 2026-08-24 by the repository owner, authorized directly in session — attributed human adoption after review.
- Depends on: [ADR-0105](0105-bridge-is-the-server-version-of-the-vsix.md) decision 5
  (the Bridge navigation surface's full capability set)
- Related: `crates/ackplane-bridge/tests/nav_consistency.rs` (the structural
  guard this ADR's decision replaces the enforcement strategy of)

## Context

Bridge has no server-side templating and no frontend build step: every route
in `crates/ackplane-bridge/src/main.rs` serves one static `.html` file
through `include_str!`, and each file is a fully self-contained document with
its own inline `<style>` and `<script>`. That is a reasonable choice for a
handful of pages. It stopped being one once Bridge grew to twelve pages that
all needed the identical brand mark and site navigation: each page carried
its own copy of the nav's HTML, its own copy of the nav's CSS, and its own
copy of the disclosure `<script>` that opens/closes a nav group and closes it
on outside click or Escape.

That duplication had already caused two different classes of drift before
this ADR. First, `administration.html` and `supervisors.html` shipped with
their own, already-diverging nav before a structural test
(`nav_consistency.rs`) existed to catch it — the reason that test was written
as a byte-identical-inline-text comparison across every discovered page in
the first place. Second, once the test existed and kept the nav's *content*
identical, the pages still diverged on presentation: five pages had no real
brand mark at all (an empty `<span class="mark">`), and three separate CSS
variable dialects (`--line`, `--border`, `--ml-border`) meant the same
nav/brand rule had to be hand-translated three times to look the same. A
byte-identical-text test can prove twelve copies currently agree; it cannot
stop a thirteenth page, or the next edit to any of the twelve, from being the
one copy someone forgets.

The nav itself also outgrew a flat link list. At twelve-plus capabilities, a
single-row nav either wraps unreadably or needs horizontal scrolling; the
fix — grouping related capabilities (Work, Evidence, Authority) behind a
disclosure control with iconography — makes the "one copy per page" problem
worse, not better, since a `<details class="nav-group">` block is far more
markup to keep byte-identical than a flat `<a>` list ever was.

## Decision

**The Bridge nav and brand mark are declared exactly once, in
`crates/ackplane-bridge/static/shared/chrome.js` and
`crates/ackplane-bridge/static/shared/chrome.css`, served as real static
assets and rendered into small mount points on every page. A page can no
longer drift from the shared nav, because it no longer has its own copy of
the nav to drift.**

1. **`chrome.js` is the single source of truth for what the nav contains.**
   A `NAV_ITEMS` array lists every capability from ADR-0105 decision 5 once:
   its id, label, href (or `null` for a capability with no page yet), and
   icon. Adding, renaming, reordering, or regrouping a capability means
   editing this one array — never a thirteenth (or first) per-page copy.
   `renderNav`/`renderGroup`/`renderLink` render it into any element marked
   `[data-bridge-nav]`, reading that mount's `data-current` attribute to
   mark the active entry and auto-open the group that contains it.
   `renderBrand` does the same for `[data-bridge-brand]`, reading an
   optional `data-subtitle`. `wireDisclosure` is the one copy of the
   open/close/outside-click/Escape behavior every page previously inlined.

2. **`chrome.css` is the single source of truth for how the nav and brand
   look**, styled entirely through six neutral custom properties
   (`--chrome-surface`, `--chrome-line`, `--chrome-ink`, `--chrome-muted`,
   `--chrome-accent`, `--chrome-focus`) so it never needs to know which of
   the three existing palette dialects a consuming page uses.

3. **A consuming page bridges its own palette onto the neutral tokens
   instead of renaming its variables.** Each page keeps its existing
   `:root` block (`--line` or `--border` or `--ml-*`) and adds six
   one-line mappings — `--chrome-line: var(--line);` and so on — so
   `chrome.css` renders in that page's existing colors without a
   repo-wide variable rename. This is a deliberate seam: it lets three
   dialects converge on shared markup today without forcing them to a
   fourth, unified palette in the same change.

4. **A page declares two mount points and loads two `<link>`/`<script>`
   tags; it declares no nav or brand markup of its own.**
   `<a class="brand" href="/" data-bridge-brand data-subtitle="...">MindLeak
   Bridge</a>` and `<nav class="nav" data-bridge-nav
   data-current="{page-stem}"></nav>` are the entire brand/nav footprint
   left in a page's HTML. `data-current` is always the page's own static
   file stem (`index` for the Fleet page, which is also the id `chrome.js`
   uses for its Fleet entry), so a page can never declare itself under the
   wrong capability. The static text inside the mount points
   (`MindLeak Bridge`, an empty `<nav>`) is a no-JS fallback: the brand
   link still works if `chrome.js` fails to load, though the nav is empty.

5. **`shared_assets.rs` serves both files as real static routes**
   (`GET /static/shared/chrome.css`, `GET /static/shared/chrome.js`) with
   the correct `Content-Type`, merged into the same router every other
   Bridge route is registered on — not templated, inlined, or duplicated
   into the served HTML at request time.

6. **The structural guard moves from comparing inlined text to verifying the
   wiring.** `nav_consistency.rs` no longer extracts and diffs each page's
   `<nav>` HTML (there is no longer any nav HTML in a page to diff); it
   asserts every discovered page links `chrome.css`, loads `chrome.js`,
   declares both mount points with the correct `data-current`, and bridges
   all six neutral tokens, and it asserts `chrome.js`'s `NAV_ITEMS` itself
   covers every known capability id and gives a real (non-`null`) href to
   every id with a page shipped under `static/`.

7. **A page may still keep genuinely page-specific chrome beside the shared
   mount points.** The Fleet page's `.scope-pill` tenant-scope indicator and
   the `.masthead-right` wrapper that lays it out next to the nav are not
   duplicated anywhere else and stay page-owned; sharing stops at the nav
   and brand, not at everything in a page's header. The Fleet page's
   previous bespoke brand treatment (an animated pulse dot and a two-part
   "mind leak / bridge" wordmark, present on no other page) is retired in
   favor of the shared brand — that inconsistency was the underlying
   problem, not a feature worth preserving through the mount point.

## Consequences

- A new Bridge capability, or a rename/reorder of an existing one, is a
  one-file edit (`chrome.js`) instead of a twelve-file edit — and it is
  structurally impossible to update eleven of twelve copies and miss the
  twelfth, because there is only one copy.
- The three CSS variable dialects still exist; this ADR does not unify them.
  A future page or refactor can still choose to converge on one dialect
  without touching `chrome.css`, which only ever reads the neutral tokens.
- `chrome.css`/`chrome.js` are plain static files with no build step, cache
  bust, or version query string. A browser that has cached an old copy sees
  stale nav/brand until the cache expires or is bypassed; this is an
  accepted tradeoff consistent with how every other Bridge static asset
  (each page's own inline CSS/JS) is already served today.
- `nav_consistency.rs`'s guard is now stronger, not just relocated: it was
  previously possible (and had already happened twice) for a page to ship
  with an inline nav that quietly differed from the others. That page no
  longer has an inline nav to differ.

## Rejected alternatives

**Keep per-page inline nav/brand markup and rely solely on the structural
test to catch drift.** Rejected because the test can only prove twelve
existing copies currently agree; it does nothing for a thirteenth page and
adds one more place every future nav change must be applied identically.
This is also the status quo that already produced two rounds of visible,
shipped drift before either round was caught.

**Introduce a server-side templating engine so pages are composed instead of
copy-pasted.** Rejected as disproportionate to the problem: Bridge's twelve
pages are otherwise independent, fully self-contained documents, and a
templating engine would add a new dependency, a new render path, and a new
class of template-injection surface to guard against for the sake of two
small, genuinely shared fragments (a nav and a brand mark).

**Unify all three CSS variable dialects into one palette as part of this
change.** Rejected as a second, separable decision: a palette-consolidation
scope change would touch every rule in every page's `<style>` block, not
just the nav/brand rules this ADR is about, and would make an already large,
mechanical refactor materially riskier without changing what a user sees.

**Keep the Fleet page's bespoke pulse-dot brand treatment and give it its own
mount-point variant.** Rejected because a second brand rendering path
defeats the purpose of a single shared source of truth, and a purely
decorative, non-functional animation on exactly one of twelve pages is the
same inconsistency this ADR exists to remove, not a justified exception to
it.
