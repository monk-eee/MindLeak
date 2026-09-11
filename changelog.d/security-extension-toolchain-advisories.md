- **Resolve the recorded extension development-toolchain advisories.** Pin
  Vitest and its coverage plugin to 4.1.11 and update the affected transitive
  packages to fast-uri 3.1.7, js-yaml 4.3.2, and qs 6.16.0. The matching Vitest
  modules advance together; Vite, Rolldown, and unrelated dependencies retain
  their previous locked versions. A clean locked install reports no npm audit
  findings; extension coverage, lint, compile, formatting, native-server smoke,
  and a platform-targeted VSIX content check pass. These were development
  dependencies, not a claim of demonstrated exploitation in the shipped
  extension.
