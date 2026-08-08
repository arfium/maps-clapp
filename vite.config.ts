import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import path from "node:path";

// Tauri v2: fixed dev port; target the WebView engine (Safari-class) like Clatch's GUI.
//
// `dist-web/` is the FRONTEND bundle, deliberately not `dist/`: the Clatch depot is
// `pkg/` (scripts/package.sh), so one name means one thing.
//
// `@clappkit` is plain .ts source in the clappkit submodule — an alias, not an npm
// package, so nothing is added to package.json. It imports `react` and `@tauri-apps/api`
// itself, and resolution would otherwise start from clappkit's own directory, which has no
// node_modules: `dedupe` pins those to THIS app's copy, the only one that should ship.
//
// maplibre-gl is a real dependency, bundled — NOT a CDN script. A packaged clapp loads its
// frontend from disk through Tauri's custom protocol, so a `<script src="https://…">` is a
// map that only works while unpkg does.
//
// The port is 1426 — one per clapp, so two windows can be in dev at once (chess 1420,
// clock 1421, telegram 1422, whatsapp 1423, higgsfield 1424, jlcpcb 1425).
export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  resolve: {
    alias: { "@clappkit": path.resolve(__dirname, "clappkit/web") },
    dedupe: ["react", "react-dom", "@tauri-apps/api"],
  },
  server: {
    port: 1426,
    strictPort: true,
    fs: { allow: ["."] },
  },
  build: { target: "safari15", outDir: "dist-web" },
});
