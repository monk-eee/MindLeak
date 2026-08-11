- **Repairing a design no longer builds twins of the tasks it already created.**
  A repair re-states the drafts it is repairing, and materialization deletes
  `design_task_links` before creating, so a revision in create mode produced a
  second copy of every task the previous revision had made and left the originals
  live but unreachable from the design. Two agents could then claim a task and
  its twin, with only one of them being the work the design points at. A create
  draft now resolves to live work of the same goal and title, so a repair
  re-links what it already made; terminal work never stands in for work that
  still has to happen. `promote_design` itself was already safe — plan equality
  and the compare-and-swap on `promotion_status` make a straight replay
  idempotent — and the link and no-work repair paths are unchanged.
