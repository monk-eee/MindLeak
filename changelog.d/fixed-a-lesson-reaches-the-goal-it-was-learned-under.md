- **A lesson that names no code now reaches the goal it was learned under.** The
  conformance advisory matched recorded knowledge on referenced nodes and
  nothing else, so a record whose evidence carried no `nodes` array was stored,
  counted, decayed, and structurally incapable of reaching any agent. Measured
  over the whole ledger: 191 records, 124 carrying nodes, **67 silent**. They
  were not marginal notes — several were among the most expensive lessons the
  repository had, and were then re-learned from scratch, at length, by agents
  with no way to know they existed.

  The reach is recovered by reading the provenance those records already carry,
  not by rewriting them: 55 of the 67 still name the goal they were learned
  under, or a task from which the goal is reachable. The advisory gained a
  second, narrower matching dimension for exactly that case. Nothing is copied
  forward and no possibly-stale claim is restated, which is what made rewriting
  the records the wrong repair.

  Three details decide whether this helps or merely adds noise:

  - It is **capped** at three lessons per check, ranked by effective weight.
    ADR-0072 established that an advisory firing on almost every task carries no
    information, and a goal accumulates everything ever learned serving it — 20
    and 18 silent records sit under the two busiest goals here.
  - Goal identity compares by **slug**, so a constitution amendment does not
    sever a lesson from the intent it belongs to.
  - Task provenance is read in every spelling it was written in — a JSON field,
    a nested array, or the bare string that is not JSON at all — because a
    reader that understood only one shape would silence the records written in
    the others.

  The advisory still only informs. It adds findings and can never harden a
  verdict, emit a violation, or downgrade an aligned one. No LLM joins the read
  path. Twelve records that name nothing at all remain undeliverable and are
  recorded as still open.
