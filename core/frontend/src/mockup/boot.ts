// Dev-only harness entry (mockup.html): installs a fake Tauri backend that answers with dummy data,
// then boots the real app. Used to render website mockups without any personal data.
import { mockIPC, mockWindows } from "@tauri-apps/api/mocks";
import { handle } from "./backend";

const label = new URLSearchParams(location.search).get("window") || "popup";
mockWindows(label);
mockIPC((cmd, args) => handle(cmd, (args ?? {}) as Record<string, unknown>), { shouldMockEvents: true });
(window as unknown as { __mockReady: boolean }).__mockReady = true;
void import("../main");
