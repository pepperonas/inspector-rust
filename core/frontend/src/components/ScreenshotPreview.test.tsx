import { describe, it, expect, afterEach, beforeEach, vi } from "vitest";
import { render, cleanup, act } from "@testing-library/react";

const over = { value: false };

vi.mock("@tauri-apps/api/core", () => ({ convertFileSrc: (p: string) => `asset://${p}` }));
vi.mock("@tauri-apps/api/event", () => ({ listen: () => Promise.resolve(() => {}) }));
vi.mock("../lib/ipc", () => ({
  getPendingScreenshotInfo: () => Promise.resolve({ path: "/tmp/shot.png", pinned: false, width: 10, height: 10 }),
  repositionPreviewToCursor: () => Promise.resolve(over.value),
  screenshotPreviewCopy: () => Promise.resolve(),
  screenshotPreviewDiscard: () => Promise.resolve(),
  screenshotPreviewEdit: () => Promise.resolve(),
  screenshotPreviewSave: () => Promise.resolve(),
  setScreenshotPinned: () => Promise.resolve(),
  pinCurrentScreenshot: () => Promise.resolve(""),
}));

import { ScreenshotPreview } from "./ScreenshotPreview";

beforeEach(() => {
  vi.useFakeTimers();
  over.value = false;
});
afterEach(() => {
  cleanup();
  vi.useRealTimers();
});

/** The layer that holds every control. */
function controls(container: HTMLElement): HTMLElement {
  const el = container.querySelector(".transition-opacity");
  if (!el) throw new Error("controls layer missing");
  return el as HTMLElement;
}

async function tick(ms: number) {
  await act(async () => {
    await vi.advanceTimersByTimeAsync(ms);
  });
}

describe("ScreenshotPreview hover", () => {
  it("shows the controls when the pointer poll reports the pointer over the window, without any click", async () => {
    const { container } = render(<ScreenshotPreview />);
    await tick(0);
    expect(controls(container).className).toContain("opacity-0");

    over.value = true;
    await tick(250);
    expect(controls(container).className).toContain("opacity-100");
    expect(controls(container).className).not.toContain("pointer-events-none");
  });

  it("hides them again once the poll reports the pointer gone", async () => {
    const { container } = render(<ScreenshotPreview />);
    await tick(0);
    over.value = true;
    await tick(250);
    over.value = false;
    await tick(250);
    expect(controls(container).className).toContain("opacity-0");
  });
});
