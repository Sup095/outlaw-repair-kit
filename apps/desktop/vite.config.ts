import { defineConfig } from "vite";
import { svelte } from "@sveltejs/vite-plugin-svelte";

// Tauri serves the front-end from a fixed port in development and from the
// built files in a release, so the port must not wander.
export default defineConfig({
  plugins: [svelte()],
  clearScreen: false,
  server: { port: 5183, strictPort: true },
  build: { target: "es2021", sourcemap: false },
});
