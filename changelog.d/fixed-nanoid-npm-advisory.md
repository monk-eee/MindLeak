### Fixed

- Closed a high-severity npm advisory (GHSA-2v37-7h3g-55p8: `nanoid`
  custom generators can loop indefinitely when size is zero) in the VS
  Code extension's dependency tree via `npm audit fix`. `nanoid` is a
  transitive dev/test dependency (`vitest` -> `vite` -> `postcss` ->
  `nanoid`), never shipped in the built extension; bumped 3.3.17 ->
  3.3.18. `npm audit` now reports 0 vulnerabilities.
