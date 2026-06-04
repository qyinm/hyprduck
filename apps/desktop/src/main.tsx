import { StrictMode } from "react";
import { createRoot } from "react-dom/client";

import { App } from "./App";
import { I18nProvider } from "./i18n/I18nProvider";
import "./styles.css";

const container = document.getElementById("app");

if (!container) {
  throw new Error("HyprDuck root container was not found.");
}

createRoot(container).render(
  <StrictMode>
    <I18nProvider>
      <App />
    </I18nProvider>
  </StrictMode>,
);
