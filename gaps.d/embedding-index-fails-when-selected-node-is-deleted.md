- **`MindLeak::index_nodes` selected candidates before the embedding-model
  round trip and wrote them without rechecking that the node still existed --
  VERIFIED 2026-08-27, repair in progress.** A concurrent `forget_file`,
  reconcile, or prune can delete a selected node while the optional model call
  is in flight. The embeddings foreign key correctly rejects the orphaned
  vector, but that error aborts the entire index batch instead of skipping the
  stale candidate and indexing the remaining live nodes.
