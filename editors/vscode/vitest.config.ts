import { defineConfig } from "vitest/config";

export default defineConfig({
  test: {
    environment: "node",
    include: ["src/**/*.test.ts", "scripts/**/*.test.mjs"],
    coverage: {
      provider: "v8",
      include: ["src/util.ts", "src/designBoard.ts", "src/mcpClient.ts", "scripts/install.mjs"],
      reportsDirectory: "coverage/vitest",
      reporter: ["text", "json-summary", "lcov"],
    },
  },
});
