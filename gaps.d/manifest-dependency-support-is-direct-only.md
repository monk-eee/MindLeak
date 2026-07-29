- **Manifest dependency support is direct-only.** — `Cargo.toml`, `package.json`,
  `go.mod`, and named PEP 508 lines in `requirements*.txt` emit `depends_on`.
  Lockfiles, transitive dependencies, npm overrides, Cargo workspace catalogs,
  Go replacements, requirement includes/options, and unnamed VCS/local Python
  requirements do not. — Low impact on direct impact analysis; intentional to
  avoid turning catalogs and resolver output into false direct edges.
