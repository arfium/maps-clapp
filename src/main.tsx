import React from "react";
import { createRoot } from "react-dom/client";
import App from "./App";
import "./styles.css";

// Browser preview (`npm run dev` with no Tauri around it): stub the core with a canned
// snapshot so the design can be looked at without a window server. `import.meta.env.DEV` is
// replaced by `false` in the production build, so this and ./preview leave the shipped
// bundle entirely.
if (import.meta.env.DEV && !("__TAURI_INTERNALS__" in window)) {
  const { installPreview } = await import("./preview");
  installPreview();
}

createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
