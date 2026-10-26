import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";

window.addEventListener("error", (event) => {
  void import("./lib/shell").then(({ logUi }) =>
    logUi(`window error: ${event.message} @ ${event.filename}:${event.lineno}`),
  );
});
window.addEventListener("unhandledrejection", (event) => {
  void import("./lib/shell").then(({ logUi }) =>
    logUi(`unhandled rejection: ${String(event.reason)}`),
  );
});

// Chromium's default context menu (and its devtools entries) never fits a
// taskbar shell; the app provides its own right-click menus. In dev, keep it
// available for inspecting pages through the webview's devtools.
if (import.meta.env.PROD) {
  window.addEventListener("contextmenu", (event) => event.preventDefault());
}

// Devtools stay available in dev builds; production can't open them at all
// (no tauri devtools feature), and the shortcuts are blocked for safety.
if (import.meta.env.PROD) {
  window.addEventListener("keydown", (event) => {
    const key = event.key.toUpperCase();
    if (
      event.key === "F12" ||
      (event.ctrlKey && event.shiftKey && ["I", "J", "C"].includes(key))
    ) {
      event.preventDefault();
      event.stopPropagation();
    }
  });
}

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
