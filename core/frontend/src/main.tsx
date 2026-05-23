import React from "react";
import ReactDOM from "react-dom/client";
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
import App from "./App";
import { ScreenshotPreview } from "./components/ScreenshotPreview";
import "./styles.css";

// Inspector Rust runs in two-or-three Tauri windows depending on what
// the user is doing. The default `popup` window is the clipboard
// browser (`<App />`). The `screenshot-preview` window is the small
// floating CleanShot-X-style preview that appears after a region
// capture. Route by `window.label` so each window mounts only the React
// tree it needs — keeps the preview window lightweight (no clipboard
// poll, no fuzzy index, no expander listeners).
const label = getCurrentWebviewWindow().label;

const root = ReactDOM.createRoot(document.getElementById("root") as HTMLElement);
root.render(
  <React.StrictMode>
    {label === "screenshot-preview" ? <ScreenshotPreview /> : <App />}
  </React.StrictMode>,
);
