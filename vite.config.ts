/// <reference types="vitest/config" />
import { defineConfig } from "vite";
import { svelte } from "@sveltejs/vite-plugin-svelte";

export default defineConfig({
  plugins: [svelte()],
  clearScreen: false,
  server: { port: 1420, strictPort: true },
  // svelte 5 runes must resolve to browser builds under vitest
  resolve: process.env.VITEST ? { conditions: ["browser"] } : undefined,
  test: { include: ["src/**/*.test.ts"] },
});
