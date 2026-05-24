import React from "react";
import ReactDOM from "react-dom/client";
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
import App from "./App";
import { ScreenshotPreview } from "./components/ScreenshotPreview";
import { ScreenshotEditor } from "./components/ScreenshotEditor";
import "./styles.css";

// Inspector Rust runs in three Tauri windows depending on what the
// user is doing. The default `popup` window is the clipboard browser
// (`<App />`). The `screenshot-preview` window is the small floating
// CleanShot-X-style preview after a region capture. The
// `screenshot-editor` window is the annotation editor (arrows / text
// / rect / highlight / blur). Route by `window.label` so each window
// mounts only the React tree it needs — keeps the auxiliary windows
// lightweight (no clipboard poll, no fuzzy index, no expander
// listeners).
const label = getCurrentWebviewWindow().label;

function Mount() {
  if (label === "screenshot-preview") return <ScreenshotPreview />;
  if (label === "screenshot-editor") return <ScreenshotEditor />;
  return <App />;
}

const root = ReactDOM.createRoot(document.getElementById("root") as HTMLElement);
root.render(
  <React.StrictMode>
    <Mount />
  </React.StrictMode>,
);
