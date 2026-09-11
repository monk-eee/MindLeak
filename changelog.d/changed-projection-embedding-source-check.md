- **Projection embedding source checks:** the server storage API now accepts the
  projected label snapshot and refuses writes after that source changes or
  disappears. Stale uploads cannot replace a current vector. This prepares the
  enrolled-node indexing path; authenticated producer RPCs and the node-side
  embedding loop are still unfinished, and Ackplane computes no embeddings.
