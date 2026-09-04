import { defineConfig } from "vite";

// Pure static hosting: relative asset paths so the build works from any
// sub-path (GitHub Pages, a CDN folder) without configuration.
export default defineConfig({
  base: "./",
  build: { target: "es2022", sourcemap: true },
  server: { port: 5173, strictPort: true },
});
