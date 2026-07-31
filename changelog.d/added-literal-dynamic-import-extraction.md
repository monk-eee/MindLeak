- **Literal dynamic JavaScript and TypeScript imports now contribute structural
  dependency edges.** Calls such as `import("./feature")` and
  `import("@scope/package")` use the same conservative artifact-candidate and
  package resolution as static imports. Computed, template-literal, malformed,
  and multi-argument forms remain ignored rather than guessed.
