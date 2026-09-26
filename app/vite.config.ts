import { defineConfig } from "vite";
import { svelte } from "@sveltejs/vite-plugin-svelte";
import { resolve } from "node:path";

// Two windows, two entry points: the settings window (index.html) and the floating
// listening pill (pill.html).
export default defineConfig({
  plugins: [svelte()],
  clearScreen: false,
  server: { port: 1420, strictPort: true },
  build: {
    target: "es2022",
    rollupOptions: {
      input: {
        main: resolve(__dirname, "index.html"),
        pill: resolve(__dirname, "pill.html"),
      },
    },
  },
  test: {
    include: ["src/**/*.test.ts"],
  },
});
