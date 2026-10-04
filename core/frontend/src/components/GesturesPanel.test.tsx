import { StrictMode } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { act, cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import type { GestureConfig, GestureGuardConfig, GestureLiveSnapshot, GestureTraceFile } from "../lib/ipc";

const zones = { left: 0.03, right: 0.03, top: 0.05, bottom: 0.05 };
const guard: GestureGuardConfig = {
  settle_ms: 30, palm_size: 2, palm_major: 0, thumb_ratio: 1.7, thumb_min_size: 1.3, thumb_zone: 0.6,
  edges_builtin: zones, edges_external: zones, palm_blocks_all: false, typing_single_ms: 250,
  typing_burst_ms: 600, burst_gap_ms: 500, release_by_center: false, center_size: 0.5,
  constant_count: true, coherence_min: 0.6, swipe_min_move: 0.06, early_min_move: 0.12, min_speed: 0,
  evenness_min: 0, tap_window_ms: 700, tap_hold_max_ms: 350, tap_max_move: 0.12, cooldown_ms: 150,
};
const config: GestureConfig = {
  enabled: true, fingers: 3, volume_step: 5, tiptap: false, typing_guard: true, volume: true, mute: true, guard,
};

let snapshot: GestureLiveSnapshot;
const gestureSetGuard = vi.fn(async (g: GestureGuardConfig) => ({ ...config, guard: g }));
const setGestureConfig = vi.fn(async (c: GestureConfig) => c);
const calibrateStart = vi.fn(async () => undefined);
const calibrateCancel = vi.fn(async () => undefined);
const calibrateApply = vi.fn(async () => ({ ...config, guard: { ...guard, palm_size: 1.9 } }));
const recordStart = vi.fn(async (_secs: number) => undefined);
const traceDelete = vi.fn(async (_name: string) => undefined);
const traceDeleteAll = vi.fn(async () => 0);
let traces: GestureTraceFile[] = [];
vi.mock("../lib/ipc", () => ({
  getGestureConfig: async () => config,
  gestureDefaultGuard: async () => guard,
  gestureLive: async () => snapshot,
  gestureLiveClear: async () => undefined,
  gestureSetGuard: (g: GestureGuardConfig) => gestureSetGuard(g),
  setGestureConfig: (c: GestureConfig) => setGestureConfig(c),
  gestureCalibrateStart: () => calibrateStart(),
  gestureCalibrateCancel: () => calibrateCancel(),
  gestureCalibrateApply: () => calibrateApply(),
  gestureRecordStart: (secs: number) => recordStart(secs),
  gestureTraceList: async () => traces,
  gestureTraceReplay: async () => [],
  gestureTraceDelete: (name: string) => traceDelete(name),
  gestureTraceDeleteAll: () => traceDeleteAll(),
  listActionHotkeys: async () => [
    { id: "gestureunintended", label: "", shortcut: "Ctrl+Shift+Alt+KeyG", default: "", is_default: true },
  ],
}));
vi.mock("../hooks/useTauriEvent", () => ({ useTauriEvent: () => undefined }));

import { GesturesPanel } from "./GesturesPanel";

beforeEach(() => {
  snapshot = {
    contacts_supported: true,
    running: true,
    now_ms: 1000,
    frame: {
      at_ms: 990,
      device: 0,
      touches: [
        { id: 1, x: 0.4, y: 0.5, major: 9, minor: 8, angle: 0, size: 0.9, class: "finger", in_edge: false },
        { id: 2, x: 0.8, y: 0.8, major: 30, minor: 20, angle: 0, size: 2.6, class: "palm", in_edge: false },
        { id: 3, x: 0.5, y: 0.01, major: 9, minor: 8, angle: 0, size: 0.9, class: "finger", in_edge: true },
      ],
    },
    typing_block: false,
    await_center: false,
    devices: [{ builtin: true, width_mm: 157.8, height_mm: 97.8 }],
    log: [
      {
        seq: 2, at_ms: 900, device: 0, level: "typing", verdict: "typing_guard", accepted: false,
        kind: "tap", fingers: 3, action: "mute_toggle", via: "tick", touch_id: null,
      },
    ],
    recording: { recording: false, remaining_ms: 0, frames: 0 },
    calibration: { state: "idle" },
  };
  traces = [];
  for (const m of [gestureSetGuard, setGestureConfig, calibrateStart, calibrateCancel, calibrateApply, recordStart, traceDelete, traceDeleteAll]) {
    m.mockClear();
  }
});
afterEach(() => {
  cleanup();
  vi.useRealTimers();
});

describe("GesturesPanel", () => {
  it("draws every contact, coloured by class, edge contacts dashed", async () => {
    const { container } = render(<GesturesPanel />);
    await waitFor(() => expect(container.querySelectorAll("ellipse")).toHaveLength(3));
    const [finger, palm, edge] = Array.from(container.querySelectorAll("ellipse"));
    expect(finger.getAttribute("stroke")).toBe("#38bdf8");
    expect(palm.getAttribute("stroke")).toBe("#f43f5e");
    expect(edge.getAttribute("stroke-dasharray")).toBe("3 3");
    expect(finger.getAttribute("stroke-dasharray")).toBeNull();
  });

  it("drops a stale frame instead of freezing the last contacts", async () => {
    snapshot = { ...snapshot, now_ms: 5000 };
    const { container } = render(<GesturesPanel />);
    await screen.findByText("Letzte Entscheidungen");
    await waitFor(() => expect(screen.getByRole("img").getAttribute("aria-label")).toContain("0 Kontakte"));
    expect(container.querySelectorAll("ellipse")).toHaveLength(0);
  });

  it("lists the decisions with their verdict", async () => {
    render(<GesturesPanel />);
    expect(await screen.findByText("blocked (typing)")).toBeTruthy();
    expect(screen.getByText("Tippen ×3")).toBeTruthy();
  });

  it("shows the typing badge only while a key press blocks", async () => {
    render(<GesturesPanel />);
    await screen.findByText("blocked (typing)");
    expect(screen.queryByText(/Lautstärke und Stumm gesperrt/)).toBeNull();
    snapshot = { ...snapshot, typing_block: true };
    expect(await screen.findByText(/Lautstärke und Stumm gesperrt/)).toBeTruthy();
  });

  it("explains the missing live view outside macOS", async () => {
    snapshot = { ...snapshot, contacts_supported: false, frame: null };
    render(<GesturesPanel />);
    expect(await screen.findByText(/Live-Ansicht der Kontakte gibt es bisher nur unter macOS/)).toBeTruthy();
    expect(screen.getByText(/die liefert bisher nur macOS/)).toBeTruthy();
    expect(screen.queryByRole("button", { name: "Kalibrieren" })).toBeNull();
  });

  it("saves a slider drag once, after the debounce, with the new value", async () => {
    render(<GesturesPanel />);
    const slider = (await screen.findByText("Pause nach Geste")).closest("label")!.querySelector("input")!;
    vi.useFakeTimers();
    fireEvent.change(slider, { target: { value: "300" } });
    fireEvent.change(slider, { target: { value: "400" } });
    expect(gestureSetGuard).not.toHaveBeenCalled();
    await act(async () => {
      vi.advanceTimersByTime(300);
    });
    expect(gestureSetGuard).toHaveBeenCalledTimes(1);
    expect(gestureSetGuard.mock.calls[0][0].cooldown_ms).toBe(400);
  });

  it("edits the edge zones of the selected profile only", async () => {
    render(<GesturesPanel />);
    fireEvent.click(await screen.findByRole("button", { name: /Extern/ }));
    const top = screen.getByText("oben").closest("label")!.querySelector("input")!;
    vi.useFakeTimers();
    fireEvent.change(top, { target: { value: "12" } });
    await act(async () => {
      vi.advanceTimersByTime(300);
    });
    const saved = gestureSetGuard.mock.calls[0][0];
    expect(saved.edges_external.top).toBeCloseTo(0.12);
    expect(saved.edges_builtin.top).toBe(0.05);
  });

  it("offers the reset only once something differs from the defaults", async () => {
    render(<GesturesPanel />);
    const slider = (await screen.findByText("Pause nach Geste")).closest("label")!.querySelector("input")!;
    expect(screen.queryByText("Standardwerte")).toBeNull();
    fireEvent.change(slider, { target: { value: "300" } });
    expect(screen.getByText("Standardwerte")).toBeTruthy();
  });

  it("switches the gestures off from the header switch", async () => {
    render(<GesturesPanel />);
    const sw = await screen.findByRole("switch");
    await waitFor(() => expect(sw.getAttribute("aria-checked")).toBe("true"));
    fireEvent.click(sw);
    await waitFor(() => expect(setGestureConfig).toHaveBeenCalledTimes(1));
    expect(setGestureConfig.mock.calls[0][0].enabled).toBe(false);
  });

  describe("calibration", () => {
    it("starts on the button and walks through the steps from the live status", async () => {
      render(<GesturesPanel />);
      fireEvent.click(await screen.findByRole("button", { name: "Kalibrieren" }));
      await waitFor(() => expect(calibrateStart).toHaveBeenCalledTimes(1));
      snapshot = {
        ...snapshot,
        calibration: { state: "running", phase: { phase: "step", step: 2, steps: 3, remaining_ms: 1500, step_ms: 6000 } },
      };
      expect(await screen.findByText("Schritt 2 von 3 · Daumen")).toBeTruthy();
      expect(screen.getByRole("progressbar").getAttribute("aria-valuenow")).toBe("75");
      fireEvent.click(screen.getByRole("button", { name: "Abbrechen" }));
      expect(calibrateCancel).toHaveBeenCalledTimes(1);
    });

    it("starts by itself once for `gestures calibrate`", async () => {
      // StrictMode runs effects twice (the app is wrapped in it).
      render(
        <StrictMode>
          <GesturesPanel start="calibrate" />
        </StrictMode>,
      );
      await waitFor(() => expect(calibrateStart).toHaveBeenCalledTimes(1));
      await screen.findByText("Letzte Entscheidungen");
      await act(async () => {
        await new Promise((r) => setTimeout(r, 80)); // several polls later
      });
      expect(calibrateStart).toHaveBeenCalledTimes(1);
      expect(recordStart).not.toHaveBeenCalled();
    });

    it("shows the proposal and saves only on confirmation", async () => {
      snapshot = {
        ...snapshot,
        calibration: {
          state: "done",
          proposal: {
            device: { builtin: true, width_mm: null, height_mm: null },
            palm: null,
            thumb: null,
            finger: null,
            changes: [{ key: "palm_size", current: 2, proposed: 1.9 }],
            warnings: ["Schritt 2: kein ruhender Daumen erkannt — Daumen-Werte bleiben."],
          },
        },
      };
      render(<GesturesPanel />);
      const card = within(await screen.findByRole("region", { name: "Kalibrierung" }));
      expect(await card.findByText("Handballen ab Größe")).toBeTruthy();
      expect(card.getByText("1,9")).toBeTruthy();
      expect(card.getByText(/kein ruhender Daumen/)).toBeTruthy();
      expect(calibrateApply).not.toHaveBeenCalled();
      fireEvent.click(screen.getByRole("button", { name: /Übernehmen/ }));
      await waitFor(() => expect(calibrateApply).toHaveBeenCalledTimes(1));
    });

    it("names a failed calibration and offers another run", async () => {
      snapshot = { ...snapshot, calibration: { state: "failed", error: "Schritt 3 hat nur 2 Fingerkontakte erkannt" } };
      render(<GesturesPanel />);
      expect(await screen.findByText(/nur 2 Fingerkontakte/)).toBeTruthy();
      fireEvent.click(screen.getByRole("button", { name: "Nochmal" }));
      await waitFor(() => expect(calibrateStart).toHaveBeenCalledTimes(1));
    });
  });

  describe("recordings", () => {
    const rec = (name: string, unintended = false): GestureTraceFile => ({ name, bytes: 40_000, unintended });

    it("lists recordings with their kind and time, misfire reports marked", async () => {
      traces = [rec("trace-unintended-20261004-141516.json", true), rec("trace-20261003-090000.json")];
      render(<GesturesPanel />);
      expect(await screen.findByText("04.10. 14:15:16")).toBeTruthy();
      expect(screen.getByText("Ungewollt")).toBeTruthy();
      expect(screen.getByText("Aufnahme")).toBeTruthy();
      expect(screen.getByText(/⌃⇧⌥G|Ctrl\+Shift\+Alt\+G/)).toBeTruthy();
    });

    it("deletes a recording only after the second click", async () => {
      traces = [rec("trace-20261003-090000.json")];
      render(<GesturesPanel />);
      fireEvent.click(await screen.findByRole("button", { name: "03.10. 09:00:00 löschen" }));
      expect(traceDelete).not.toHaveBeenCalled();
      fireEvent.click(screen.getByRole("button", { name: "Ja, löschen" }));
      await waitFor(() => expect(traceDelete).toHaveBeenCalledWith("trace-20261003-090000.json"));
    });

    it("deletes all only after confirming", async () => {
      traces = [rec("trace-20261003-090000.json"), rec("trace-20261003-100000.json")];
      render(<GesturesPanel />);
      fireEvent.click(await screen.findByRole("button", { name: "Alle löschen" }));
      fireEvent.click(screen.getByRole("button", { name: "Nein" }));
      expect(traceDeleteAll).not.toHaveBeenCalled();
      fireEvent.click(screen.getByRole("button", { name: "Alle löschen" }));
      fireEvent.click(screen.getByRole("button", { name: "Ja, löschen" }));
      await waitFor(() => expect(traceDeleteAll).toHaveBeenCalledTimes(1));
    });

    it("starts a 30-s recording for `gestures record` and shows the countdown", async () => {
      render(
        <StrictMode>
          <GesturesPanel start="record" />
        </StrictMode>,
      );
      await waitFor(() => expect(recordStart).toHaveBeenCalledWith(30));
      await act(async () => {
        await new Promise((r) => setTimeout(r, 50));
      });
      expect(recordStart).toHaveBeenCalledTimes(1);
      snapshot = { ...snapshot, recording: { recording: true, remaining_ms: 12_300, frames: 512 } };
      expect(await screen.findByText(/Nimmt auf … 13 s · 512 Frames/)).toBeTruthy();
      expect(calibrateStart).not.toHaveBeenCalled();
    });
  });
});
