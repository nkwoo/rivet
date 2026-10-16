import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import "@fontsource/archivo/800.css";
import "@fontsource/ibm-plex-sans/400.css";
import "@fontsource/ibm-plex-sans/500.css";
import "@fontsource/jetbrains-mono/400.css";
import "./styles.css";
import App from "./App.tsx";

const root = document.getElementById("root");
if (!root) {
  throw new Error("#root element not found");
}

createRoot(root).render(
  <StrictMode>
    <App />
  </StrictMode>,
);
