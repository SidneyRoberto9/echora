import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import { api } from "./lib/api";
import "./styles.css";

window.addEventListener("error", (event) => {
  api.reportFrontendCrash(event.message, event.error?.stack).catch(() => {});
});
window.addEventListener("unhandledrejection", (event) => {
  const reason = event.reason;
  const message = reason instanceof Error ? reason.message : String(reason);
  const stack = reason instanceof Error ? reason.stack : undefined;
  api.reportFrontendCrash(message, stack).catch(() => {});
});

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
