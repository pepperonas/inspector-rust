import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import type { GestureLiveSnapshot, GestureLogEntry } from "../lib/ipc";
import type { GestureBinding, GestureBindingsView } from "../lib/gesture-bindings";

const mute: GestureBinding = {
  id: "mute",
  trigger: { kind: "tap", fingers: 3 },
  action: { type: "mute_toggle" },
  app: null,
  app_name: null,
  enabled: true,
  typing_guard: true,
};
const up: GestureBinding = { ...mute, id: "volume-up", trigger: { kind: "swipe_up", fingers: 3 }, action: { type: "volume_up" } };

let view: GestureBindingsView;
let log: GestureLogEntry[] = [];
const set = vi.fn(async (bindings: GestureBinding[]) => {
  view = { ...view, bindings: bindings.map((b, i) => ({ ...b, id: b.id || `b${i}` })), customised: true };
  return view;
});
const reset = vi.fn(async () => {
  view = { ...view, bindings: [mute, up], customised: false };
  return view;
});
const capture = vi.fn(async (_on: boolean) => undefined);

vi.mock("../lib/ipc", () => ({
  gestureBindingsGet: async () => view,
  gestureBindingsSet: (b: GestureBinding[]) => set(b),
  gestureBindingsReset: () => reset(),
  gestureCapture: (on: boolean) => capture(on),
  gestureLive: async () => ({ log }) as unknown as GestureLiveSnapshot,
  listActionHotkeys: async () => [
    { id: "ocr", label: "OCR region → text", shortcut: "", default: "", is_default: true },
    { id: "gestureunintended", label: "Mark unintended", shortcut: "", default: "", is_default: true },
  ],
  aiTasksState: async () => ({
    tasks: [
      { id: 4, name: "Downloads aufräumen", approved: true },
      { id: 5, name: "Entwurf", approved: false },
    ],
    running: [],
    paused: false,
    languages: [],
    next_due: {},
  }),
  listApps: async () => [{ name: "iTerm", path: "/Applications/iTerm.app", name_lower: "iterm" }],
  appBundleId: async () => "com.googlecode.iterm2",
}));

import { GestureBindingsSection } from "./GestureBindingsSection";

function entry(seq: number, kind: GestureLogEntry["kind"], fingers: number): GestureLogEntry {
  return { seq, at_ms: seq, device: 0, level: "config", verdict: "unmapped", accepted: false, kind, fingers, action: null, via: "frame", touch_id: null };
}

beforeEach(() => {
  view = { bindings: [mute, up], customised: false, min_fingers: 3, max_fingers: 5 };
  log = [];
  set.mockClear();
  reset.mockClear();
  capture.mockClear();
});
afterEach(() => {
  cleanup();
  vi.useRealTimers();
});

describe("GestureBindingsSection", () => {
  it("lists the bindings with readable labels", async () => {
    render(<GestureBindingsSection />);
    expect(await screen.findByText("3 Finger · Tippen")).toBeTruthy();
    expect(screen.getByText("Stumm an/aus")).toBeTruthy();
    expect(screen.getByText("3 Finger · Wischen nach oben")).toBeTruthy();
    // The built-in set has nothing to reset.
    expect(screen.queryByText("Standard wiederherstellen")).toBeNull();
  });

  it("switching a binding off saves the whole list with it disabled", async () => {
    render(<GestureBindingsSection />);
    fireEvent.click(await screen.findByLabelText("3 Finger · Tippen aktiv"));
    await waitFor(() => expect(set).toHaveBeenCalledTimes(1));
    const saved = set.mock.calls[0][0];
    expect(saved.find((b) => b.id === "mute")?.enabled).toBe(false);
    expect(saved).toHaveLength(2);
  });

  it("deletes only on the second click", async () => {
    render(<GestureBindingsSection />);
    await screen.findByText("3 Finger · Tippen");
    fireEvent.click(screen.getAllByLabelText("Löschen")[0]);
    expect(set).not.toHaveBeenCalled();
    fireEvent.click(screen.getByLabelText("Wirklich löschen"));
    await waitFor(() => expect(set).toHaveBeenCalledTimes(1));
    expect(set.mock.calls[0][0].map((b) => b.id)).toEqual(["volume-up"]);
  });

  it("adds a binding that opens a URL in one app only", async () => {
    render(<GestureBindingsSection />);
    fireEvent.click(await screen.findByText("Neue Zuordnung"));
    fireEvent.change(screen.getByLabelText("Finger"), { target: { value: "4" } });
    fireEvent.change(screen.getByLabelText("Aktion"), { target: { value: "open" } });
    const save = screen.getByText("Speichern").closest("button")!;
    expect(save.disabled).toBe(true); // target still missing
    fireEvent.change(screen.getByLabelText("Adresse oder Pfad"), { target: { value: "https://celox.io" } });
    fireEvent.click(screen.getByLabelText("Nur in einer App"));
    fireEvent.change(await screen.findByLabelText("App suchen"), { target: { value: "ite" } });
    fireEvent.click(await screen.findByText("iTerm"));
    await screen.findByText("(com.googlecode.iterm2)");
    fireEvent.click(screen.getByText("Speichern"));
    await waitFor(() => expect(set).toHaveBeenCalledTimes(1));
    const added = set.mock.calls[0][0][2];
    expect(added.trigger).toEqual({ kind: "swipe_left", fingers: 4 });
    expect(added.action).toEqual({ type: "open", target: "https://celox.io" });
    expect(added.app).toBe("com.googlecode.iterm2");
    expect(added.app_name).toBe("iTerm");
    expect(added.typing_guard).toBe(true);
  });

  it("offers approved tasks only and hides the misfire hotkey", async () => {
    render(<GestureBindingsSection />);
    fireEvent.click(await screen.findByText("Neue Zuordnung"));
    fireEvent.change(screen.getByLabelText("Aktion"), { target: { value: "task" } });
    const draft = (await screen.findByText("Entwurf (nicht freigegeben)")) as HTMLOptionElement;
    expect(draft.disabled).toBe(true);
    fireEvent.change(screen.getByLabelText("Aktion"), { target: { value: "hotkey" } });
    expect(screen.getByText("OCR region → text")).toBeTruthy();
    expect(screen.queryByText("Mark unintended")).toBeNull();
  });

  it("takes the trigger from a demonstrated gesture without firing it", async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    log = [entry(7, "tap", 3)];
    render(<GestureBindingsSection />);
    fireEvent.click(await screen.findByText("Neue Zuordnung"));
    fireEvent.click(screen.getByText("Geste vormachen"));
    await waitFor(() => expect(capture).toHaveBeenCalledWith(true));
    // The user performs a 4-finger swipe right.
    log = [entry(8, "swipe_right", 4), ...log];
    await act(async () => {
      vi.advanceTimersByTime(200);
    });
    await waitFor(() => expect((screen.getByLabelText("Geste") as HTMLSelectElement).value).toBe("swipe_right"));
    expect((screen.getByLabelText("Finger") as HTMLSelectElement).value).toBe("4");
    expect(capture).toHaveBeenLastCalledWith(false);
  });

  it("restores the built-in set only after confirming", async () => {
    view = { ...view, customised: true };
    render(<GestureBindingsSection />);
    fireEvent.click(await screen.findByText("Standard wiederherstellen"));
    expect(reset).not.toHaveBeenCalled();
    fireEvent.click(screen.getByText("Wirklich alle eigenen Zuordnungen verwerfen?"));
    await waitFor(() => expect(reset).toHaveBeenCalledTimes(1));
  });

  it("shows the backend's refusal", async () => {
    set.mockRejectedValueOnce("zwei aktive Zuordnungen für dieselbe Geste");
    render(<GestureBindingsSection />);
    fireEvent.click(await screen.findByLabelText("3 Finger · Tippen aktiv"));
    expect(await screen.findByText("zwei aktive Zuordnungen für dieselbe Geste")).toBeTruthy();
  });
});
