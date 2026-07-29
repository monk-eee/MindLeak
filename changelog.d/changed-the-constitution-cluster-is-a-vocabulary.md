- The constitution and policy-pack tools collapse to a vocabulary (ADR-0059):
  `constitution_define`, `constitution_decide` and `constitution_query`, beside
  the already-collapsed `policy_pack_register`, `policy_pack_decide` and
  `policy_pack_query`. Each names its transition in an `action` argument rather
  than in a tool name.
  Every superseded name still answers for one minor version and its description
  names the call to make instead — a caller mid-task cannot read a changelog, so
  the deprecation has to teach rather than simply break. No guard was lost: each
  refusal a separate tool name used to encode is now an argument validation
  carrying the same message, including the attribution required to adopt policy.
