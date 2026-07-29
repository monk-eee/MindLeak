- **The extension toolchain has one low-severity development advisory.** —
  Vitest resolves `esbuild` 0.27.7, affected by GHSA-g7r4-m6w7-qqqr when its
  development server runs on Windows. `npm audit --omit=dev` is clean and the
  package is not shipped with the extension; a normal `npm audit fix` finds no
  compatible update. — Low impact. — Left open until Vitest accepts a fixed
  `esbuild`; do not use `--force` to hide the compatibility decision.
