import { defineConfig } from "vitest/config";

export default defineConfig({
  test: {
    environment: "node",
    include: ["src/**/*.test.ts", "scripts/**/*.test.mjs"],
    coverage: {
      provider: "v8",
      // Report coverage only for files the tests actually execute. The default
      // `all` baseline walk double-counts every file on Windows: the V8 provider
      // records executed modules under a lowercase drive letter (file:///c:/...)
      // while the include walk resolves them under the OS's uppercase drive
      // letter, so each file appears twice (the real entry plus a phantom 0%
      // one), roughly halving reported coverage and failing the line threshold
      // even when real coverage is well above it. Every file listed below is
      // exercised by the suite, so disabling the baseline walk loses no signal.
      all: false,
      include: ["src/util.ts", "src/designBoard.ts", "src/mcpClient.ts", "scripts/install.mjs"],
      reportsDirectory: "coverage/vitest",
      reporter: ["text", "json-summary", "lcov"],
    },
  },
});
