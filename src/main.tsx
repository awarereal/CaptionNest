import React from "react";
import ReactDOM from "react-dom/client";

import "@fontsource-variable/anek-latin/standard.css";
import "@fontsource-variable/anek-bangla";
import "@fontsource-variable/anek-devanagari";
import "@fontsource-variable/readex-pro";
import "@fontsource-variable/atkinson-hyperlegible-mono";

import "./styles/tokens.css";
import "./styles/base.css";
import "./styles/ui.css";
import "./styles/app.css";
import "./styles/screens.css";
import "./styles/workspace.css";

import App from "./App";
import { useStore } from "./state/store";

// Read-only handle for automated UI tests (scripts/dev). The page loads no remote content.
(window as unknown as { __CN_STORE__: typeof useStore }).__CN_STORE__ = useStore;

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
