import React from "react";
import { createRoot } from "react-dom/client";

import { App } from "./App";
import { ErrorBoundary } from "./components/ErrorBoundary";
import "./styles.css";

const container = document.getElementById("root");
if (!container) {
  throw new Error("index.html is missing its root element");
}

// The outer boundary catches what the shell's own cannot: a failure in the
// shell itself. Without it a single render error leaves the window blank.
createRoot(container).render(
  <React.StrictMode>
    <ErrorBoundary scope="window">
      <App />
    </ErrorBoundary>
  </React.StrictMode>,
);
