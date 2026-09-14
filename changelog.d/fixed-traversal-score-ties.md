- Graph traversal now breaks equal-score node ties by depth, original query
  seed order, and stable node ID. Repeated reads no longer shuffle equally
  weighted evidence or push query seeds behind tied descendants in a bounded
  context prefix. Score priority, decay filtering, and traversal direction are
  unchanged.
