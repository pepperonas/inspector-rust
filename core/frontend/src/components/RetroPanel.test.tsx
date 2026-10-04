import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, cleanup, fireEvent, waitFor, act } from "@testing-library/react";
import type { RetroConfig, RetroModeSettings, RetroPreset } from "../lib/ipc";

// Mirrors ModeSettings::defaults in retro/config.rs.
const EIGHT: RetroModeSettings = {
  palette: "nes", pixel_pt: 4, dither: "bayer4", dither_strength: 60,
  focus: true, focus_native: true, focus_pixel_pt: 1.5, focus_border: true,
  lens: false, lens_radius_pt: 80, lens_view: "original",
  scanlines: true, scanline_intensity: 40, crt: false, crt_strength: 40,
  opacity: 100, retro_frames: false, sprite_cursor: false,
};
const SIXTEEN: RetroModeSettings = {
  ...EIGHT, palette: "snes", pixel_pt: 2, dither_strength: 25, focus: true,
  focus_pixel_pt: 1, focus_border: false, scanlines: false, scanline_intensity: 30, crt_strength: 30,
};
const base = (): RetroConfig => ({ mode: "8bit", eight: { ...EIGHT }, sixteen: { ...SIXTEEN }, target: "current", fps: 60, retreat: true });
const BUILTIN: RetroPreset[] = [
  { name: "Show", mode: "8bit", settings: EIGHT, builtin: true },
  { name: "Alltag", mode: "16bit", settings: SIXTEEN, builtin: true },
];

let stored: RetroConfig = base();
let presets: RetroPreset[] = [...BUILTIN];
const retroSetConfig = vi.fn(async (c: RetroConfig) => (stored = c));
const retroRun = vi.fn(async (_a: string) => undefined);
const retroPresetSave = vi.fn(async (name: string) => {
  presets = [...presets, { name, mode: stored.mode, settings: stored.mode === "8bit" ? stored.eight : stored.sixteen, builtin: false }];
  return presets;
});
const retroPresetDelete = vi.fn(async (name: string) => (presets = presets.filter((p) => p.name !== name)));
const retroReset = vi.fn(async () => {
  stored = stored.mode === "8bit" ? { ...stored, eight: { ...EIGHT } } : { ...stored, sixteen: { ...SIXTEEN } };
  return stored;
});

vi.mock("../lib/ipc", () => ({
  retroGetConfig: async () => stored,
  retroSetConfig: (c: RetroConfig) => retroSetConfig(c),
  retroPresets: async () => presets,
  retroPresetSave: (n: string) => retroPresetSave(n),
  retroPresetDelete: (n: string) => retroPresetDelete(n),
  retroPresetApply: async () => stored,
  retroReset: () => retroReset(),
  retroRun: (a: string) => retroRun(a),
  retroStatus: async () => ({ running: false, supported: true, screen_permission: true, accessibility: true, note: null }),
  retroPreviewStart: async () => undefined,
  retroPreviewStop: async () => undefined,
  retroOpenPermission: async () => undefined,
}));
vi.mock("@tauri-apps/api/event", () => ({ listen: async () => () => undefined }));

import { RetroPanel } from "./RetroPanel";

const key = (k: string) => act(() => void window.dispatchEvent(new KeyboardEvent("keydown", { key: k, bubbles: true })));
const valueOf = (root: HTMLElement, label: string) => {
  const row = [...root.querySelectorAll("span")].find((s) => s.textContent === label)?.parentElement;
  return row?.querySelector("span.min-w-\\[88px\\]")?.textContent;
};

beforeEach(() => {
  stored = base();
  presets = [...BUILTIN];
  vi.clearAllMocks();
});
afterEach(cleanup);

describe("RetroPanel", () => {
  it("↓ moves the row, → changes its value and saves (debounced)", async () => {
    const { container } = render(<RetroPanel focused keyword="8bit" onExit={() => {}} />);
    await waitFor(() => expect(valueOf(container, "Pixelgröße")).toBe("4 pt"));
    await key("ArrowDown"); // Palette
    await key("ArrowDown"); // Pixelgröße
    await key("ArrowRight");
    expect(valueOf(container, "Pixelgröße")).toBe("4,5 pt");
    await waitFor(() => expect(retroSetConfig).toHaveBeenCalled());
    const calls = retroSetConfig.mock.calls;
    expect(calls[calls.length - 1][0].eight.pixel_pt).toBe(4.5);
  });

  it("switching the mode shows the other mode's defaults", async () => {
    const { container } = render(<RetroPanel focused keyword="8bit" onExit={() => {}} />);
    await waitFor(() => expect(valueOf(container, "Palette")).toBe("NES"));
    await key("ArrowRight"); // row 0 = Modus
    expect(valueOf(container, "Modus")).toBe("16-Bit");
    expect(valueOf(container, "Palette")).toBe("SNES");
    expect(valueOf(container, "Pixelgröße")).toBe("2 pt");
    expect(valueOf(container, "Aktives Fenster zeigt")).toBe("Original");
  });

  it("the 16bit keyword opens in 16-bit mode", async () => {
    const { container } = render(<RetroPanel focused keyword="16bit" onExit={() => {}} />);
    await waitFor(() => expect(valueOf(container, "Modus")).toBe("16-Bit"));
    expect(retroSetConfig.mock.calls[0][0].mode).toBe("16bit");
  });

  it("pixel size stops at the mode's limits", async () => {
    const { container } = render(<RetroPanel focused keyword="16bit" onExit={() => {}} />);
    await waitFor(() => expect(valueOf(container, "Modus")).toBe("16-Bit"));
    await key("ArrowDown");
    await key("ArrowDown");
    for (let i = 0; i < 12; i++) await key("ArrowRight");
    expect(valueOf(container, "Pixelgröße")).toBe("4 pt");
  });

  it("Enter toggles the overlay, Esc leaves", async () => {
    const onExit = vi.fn();
    render(<RetroPanel focused keyword="8bit" onExit={onExit} />);
    await waitFor(() => expect(retroRun).not.toHaveBeenCalled());
    await key("Enter");
    expect(retroRun).toHaveBeenCalledWith("toggle");
    await key("Escape");
    expect(onExit).toHaveBeenCalled();
  });

  it("keys are ignored while unfocused", async () => {
    const { container } = render(<RetroPanel focused={false} keyword="8bit" onExit={() => {}} />);
    await waitFor(() => expect(valueOf(container, "Modus")).toBe("8-Bit"));
    await key("ArrowRight");
    expect(valueOf(container, "Modus")).toBe("8-Bit");
  });

  it("saves and deletes a preset; built-ins have no delete button", async () => {
    const { getByPlaceholderText, getByText, findByText, queryByLabelText } = render(
      <RetroPanel focused keyword="8bit" onExit={() => {}} />,
    );
    await findByText("Show");
    expect(queryByLabelText("Preset Show löschen")).toBeNull();
    fireEvent.change(getByPlaceholderText("Name für neuen Preset"), { target: { value: "Mein Look" } });
    fireEvent.click(getByText("Speichern"));
    await findByText("Mein Look");
    expect(retroPresetSave).toHaveBeenCalledWith("Mein Look");
    fireEvent.click(await waitFor(() => queryByLabelText("Preset Mein Look löschen")!));
    await waitFor(() => expect(retroPresetDelete).toHaveBeenCalledWith("Mein Look"));
  });

  it("reset restores the active mode's defaults", async () => {
    stored = { ...base(), eight: { ...EIGHT, pixel_pt: 9 } };
    const { container, getByText } = render(<RetroPanel focused keyword="8bit" onExit={() => {}} />);
    await waitFor(() => expect(valueOf(container, "Pixelgröße")).toBe("9 pt"));
    fireEvent.click(getByText("Zurücksetzen"));
    await waitFor(() => expect(valueOf(container, "Pixelgröße")).toBe("4 pt"));
  });
});
