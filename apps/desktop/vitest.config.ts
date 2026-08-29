import { defineConfig } from "vitest/config";

// Separate from vite.config.ts on purpose. That file exists to build the
// window and carries the Svelte plugin and a fixed dev-server port; loading it
// here would start reserving port 5183 to run a unit test, which fails on a
// machine already running the thing being tested.
export default defineConfig({
  test: {
    include: ["src/**/*.test.ts"],
    environment: "node",
  },
});
