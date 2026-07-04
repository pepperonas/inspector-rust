import React, { Suspense } from "react";
import ReactDOM from "react-dom/client";
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
import "./styles.css";

// Inspector Rust runs in many Tauri windows depending on what the user is
// doing. The default `popup` window is the clipboard browser (`<App />`);
// everything else (screenshot preview/editor/pins, status toast, record
// overlay + stop bar, audio-swap, trim, colour loupe, alarm, snap overlay,
// window palette) is an auxiliary window routed by `window.label`.
//
// EVERY window component is `React.lazy` so Vite code-splits each into its
// own chunk (v0.84.228): with static imports every transient window — the
// ~1.6 s status toast, a screenshot pin, the record stop bar — parsed and
// evaluated the ENTIRE popup bundle (games, BPM audio graphs, stats charts,
// timesheet, …) just to render a handful of DOM nodes. App is lazy too:
// leaving it in the eager entry chunk would put the full popup bundle right
// back into every auxiliary window. The popup webview is created once at app
// start and reused, so its chunk loads once at startup — hotkey-to-visible
// latency is unaffected. Chunk loads are local asset fetches (no network).
const App = React.lazy(() => import("./App"));
const ScreenshotPreview = React.lazy(() =>
  import("./components/ScreenshotPreview").then((m) => ({ default: m.ScreenshotPreview })),
);
const ScreenshotEditor = React.lazy(() =>
  import("./components/ScreenshotEditor").then((m) => ({ default: m.ScreenshotEditor })),
);
const ScreenshotPin = React.lazy(() =>
  import("./components/ScreenshotPin").then((m) => ({ default: m.ScreenshotPin })),
);
const StatusToast = React.lazy(() =>
  import("./components/StatusToast").then((m) => ({ default: m.StatusToast })),
);
const SnapOverlay = React.lazy(() =>
  import("./components/SnapOverlay").then((m) => ({ default: m.SnapOverlay })),
);
const WindowPalette = React.lazy(() =>
  import("./components/WindowPalette").then((m) => ({ default: m.WindowPalette })),
);
const BrightnessOverlay = React.lazy(() =>
  import("./components/BrightnessOverlay").then((m) => ({ default: m.BrightnessOverlay })),
);
const RecordOverlay = React.lazy(() =>
  import("./components/RecordOverlay").then((m) => ({ default: m.RecordOverlay })),
);
const RecordStopBar = React.lazy(() =>
  import("./components/RecordStopBar").then((m) => ({ default: m.RecordStopBar })),
);
const AudioSwapOverlay = React.lazy(() =>
  import("./components/AudioSwapOverlay").then((m) => ({ default: m.AudioSwapOverlay })),
);
const TrimOverlay = React.lazy(() =>
  import("./components/TrimOverlay").then((m) => ({ default: m.TrimOverlay })),
);
const ColorLoupe = React.lazy(() =>
  import("./components/ColorLoupe").then((m) => ({ default: m.ColorLoupe })),
);
const AlarmOverlay = React.lazy(() =>
  import("./components/AlarmOverlay").then((m) => ({ default: m.AlarmOverlay })),
);

const label = getCurrentWebviewWindow().label;

function Mount() {
  if (label === "screenshot-preview") return <ScreenshotPreview />;
  if (label === "screenshot-editor") return <ScreenshotEditor />;
  if (label === "status-toast") return <StatusToast />;
  if (label === "brightness-overlay") return <BrightnessOverlay />;
  if (label === "record-overlay") return <RecordOverlay />;
  if (label === "record-stop") return <RecordStopBar />;
  if (label === "audio-swap") return <AudioSwapOverlay />;
  if (label === "trim-overlay") return <TrimOverlay />;
  if (label === "color-loupe") return <ColorLoupe />;
  if (label === "alarm-overlay") return <AlarmOverlay />;
  if (label.startsWith("screenshot-pin-")) return <ScreenshotPin />;
  if (label === "snap-overlay") return <SnapOverlay />;
  if (label === "window-palette") return <WindowPalette />;
  return <App />;
}

const root = ReactDOM.createRoot(document.getElementById("root") as HTMLElement);
root.render(
  <React.StrictMode>
    <Suspense fallback={null}>
      <Mount />
    </Suspense>
  </React.StrictMode>,
);
