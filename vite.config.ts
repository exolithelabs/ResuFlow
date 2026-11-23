import { defineConfig } from "vite";

export default defineConfig({
  root: "web",
  clearScreen: false,
  server: { port: 5173, strictPort: true },
  build: { outDir: "../dist", emptyOutDir: true, target: "es2022" },
});
