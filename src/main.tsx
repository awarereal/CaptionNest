import React from "react";
import ReactDOM from "react-dom/client";

import "@fontsource-variable/anek-latin/standard.css";
import "@fontsource-variable/anek-bangla";
import "@fontsource-variable/anek-devanagari";
import "@fontsource-variable/readex-pro";
import "@fontsource-variable/atkinson-hyperlegible-mono";
// Caption fonts (all with Bengali + Latin), offered in the caption style panel.
import "@fontsource/hind-siliguri/400.css";
import "@fontsource/hind-siliguri/500.css";
import "@fontsource/hind-siliguri/600.css";
import "@fontsource/hind-siliguri/700.css";
import "@fontsource-variable/noto-sans";
import "@fontsource-variable/noto-sans-bengali";
import "@fontsource-variable/baloo-da-2";
import "@fontsource/atma/400.css";
import "@fontsource/atma/500.css";
import "@fontsource/atma/600.css";
import "@fontsource/atma/700.css";
import "@fontsource/tiro-bangla/400.css";

import "./styles/tokens.css";
import "./styles/base.css";
import "./styles/ui.css";
import "./styles/app.css";
import "./styles/screens.css";
import "./styles/workspace.css";

import App from "./App";

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
