- Fixed `ackplane-mcp` serving without proving node trust when its enrolled
  identity was only *partially* declared, or when the signing-key seed was set
  but malformed. Both cases took the same path as "nothing declared at all",
  which is the documented no-op, so an operator who set four of the five
  `MINDLEAK_ACKPLANE_*` variables — or mistyped the seed — got silently weaker
  trust than they had asked for and was told nothing. Only a completely
  undeclared identity is now treated as the no-op; anything half-configured is
  refused and names the variable at fault.
- Changed `ackplane-supervisor` and `ackplane-mcp` to resolve the enrolled
  node identity through the shared `ackplane_client::node_identity` module
  instead of private copies of the same environment-variable reading, signer
  selection, credential-facility account naming and seed decoding. A change to
  any of those now reaches both processes at once rather than one of them.
  `resolve_node_identity` reports which variables are missing (or that the
  seed is malformed) rather than a bare `None`, so the supervisor's
  "name every missing variable" refusal survives the move, and `ackplane-mcp`
  now names the variables actually absent instead of listing all of them.
