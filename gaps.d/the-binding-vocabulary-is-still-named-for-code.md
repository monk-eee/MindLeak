- **The binding vocabulary is still named for code below the verb.** —
  `link_goal_to_code` became `link_goal_to_artifact` (ADR-0060), but
  `CodeBindingMode` and the `code_bindings` table it writes to still say
  "code". — Low impact, cosmetic but misleading: the type name contradicts what
  the verb accepts, and the next reader will reasonably infer the store refuses
  non-code nodes when it does not. — **Left for later, deliberately:** renaming
  the enum is a public-API change and renaming the table is a data migration,
  neither of which belongs in a rename that had to migrate every caller
  atomically. Should be its own change, not bolted onto this one.
