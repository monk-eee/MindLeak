- **A required tool argument must now be reachable — declared in the schema or
  injected by the session — and a guard fails if one is neither.** Two ways an
  argument gets to a handler: the caller reads it in the schema and sends it, or
  `bind_session` resolves the session and injects it. `agent` is the second kind
  — it is deliberately stripped from every session-bound tool and replaced by
  `session_id`, so attribution on the verbs that amend the constitution, grant
  waivers and accept ratchet baselines is *resolved from the session*, never
  asserted by the caller. A tool that declared `agent` would let a caller name
  itself anything it liked. But a handler reading `agent` beside a schema that
  never mentions it looks exactly like the `lease_secs` typo incident, and the
  symptom if it ever were one is identical: `missing required string arg`
  naming a field the tool does not advertise. The rule is now pinned across the
  constitution, amendment, control, waiver, executive and design tools, with a
  floor on how many arguments the check inspects — a source scan that quietly
  stops matching anything is a green tick over an empty set.
