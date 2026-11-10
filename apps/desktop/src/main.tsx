import React from "react";
import { createRoot } from "react-dom/client";

import { App } from "./App";
import { ErrorBoundary } from "./components/ErrorBoundary";
import { applyTheme } from "./sdk/theme";
import "./styles.css";

const container = document.getElementById("root");
if (!container) {
  throw new Error("index.html is missing its root element");
}

// The theme is on <html> before React renders anything, so the first frame it
// paints is already in the right colours and nothing fades in from the wrong
// ones. See sdk/theme.ts.
applyTheme();

// The outer boundary catches what the shell's own cannot: a failure in the
// shell itself. Without it a single render error leaves the window blank.
createRoot(container).render(
  <React.StrictMode>
    <ErrorBoundary scope="window">
      <App />
    </ErrorBoundary>
  </React.StrictMode>,
);
