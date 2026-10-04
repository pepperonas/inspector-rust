import { describe, it, expect, vi, afterEach, beforeEach } from "vitest";
import { render, screen, cleanup, fireEvent, act, waitFor } from "@testing-library/react";
import type { AiTask, AiTasksState } from "../lib/tasks";

vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn(async () => () => {}) }));

const task = (p: Partial<AiTask> = {}): AiTask => ({
  id: 7,
  name: "Downloads aufräumen",
  prompt: "Räume auf",
  language: "zsh",
  script: "echo hi\n",
  explanation: "Sagt hallo.",
  provider: "anthropic",
  trigger: { type: "interval", minutes: 60 },
  enabled: true,
  timeout_s: 120,
  script_hash: "HASH-1",
  approved: false,
  created_ms: 0,
  updated_ms: 0,
  last_fire_ms: null,
  last_run: null,
  ...p,
});

const ipc = vi.hoisted(() => ({
  aiTasksState: vi.fn(),
  aiProviderStatus: vi.fn(),
  aiGetConfig: vi.fn(),
  aiTaskGenerate: vi.fn(),
  aiTaskSave: vi.fn(),
  aiTaskApprove: vi.fn(),
  aiTaskRevoke: vi.fn(),
  aiTaskRunNow: vi.fn(),
  aiTaskRuns: vi.fn(),
  aiTaskSetEnabled: vi.fn(),
  aiTaskDelete: vi.fn(),
  aiTasksSetPaused: vi.fn(),
}));
vi.mock("../lib/ipc", () => ipc);

import { TasksPanel, resetTasksSession } from "./TasksPanel";

function state(tasks: AiTask[], over: Partial<AiTasksState> = {}): AiTasksState {
  return { tasks, running: [], paused: false, languages: ["zsh", "bash", "python"], next_due: {}, ...over };
}

beforeEach(() => {
  resetTasksSession();
  for (const f of Object.values(ipc)) f.mockReset();
  ipc.aiTasksState.mockResolvedValue(state([]));
  ipc.aiProviderStatus.mockResolvedValue([
    { id: "anthropic", label: "Claude (API)", configured: true, hint: "••••abcd", model: "m", default_model: "m" },
  ]);
  ipc.aiGetConfig.mockResolvedValue({ provider: "anthropic", models: {} });
  ipc.aiTaskRuns.mockResolvedValue([]);
});
afterEach(cleanup);

const mount = (arg = "", focused = false) =>
  render(<TasksPanel arg={arg} focused={focused} onExit={vi.fn()} onOpenSettings={vi.fn()} />);

describe("TasksPanel", () => {
  it("shows the empty state and a hint when no provider is set up", async () => {
    ipc.aiProviderStatus.mockResolvedValue([
      { id: "anthropic", label: "Claude (API)", configured: false, hint: null, model: "m", default_model: "m" },
    ]);
    mount();
    expect(await screen.findByText(/Ersten Task anlegen/)).toBeTruthy();
    expect(await screen.findByText(/Noch kein KI-Anbieter eingerichtet/)).toBeTruthy();
  });

  it("generates a script, then saves AND approves exactly the saved version", async () => {
    ipc.aiTaskGenerate.mockResolvedValue({ name: "Hallo", language: "bash", script: "echo hallo\n", explanation: "x" });
    ipc.aiTaskSave.mockResolvedValue(task({ id: 9, script_hash: "SAVED-HASH" }));
    ipc.aiTaskApprove.mockResolvedValue(task({ id: 9, approved: true }));
    mount();
    fireEvent.click(await screen.findByText(/Ersten Task anlegen/));
    fireEvent.change(screen.getByPlaceholderText(/Lösche im Downloads-Ordner/), { target: { value: "Sag hallo" } });
    fireEvent.click(screen.getByText("Skript erzeugen"));
    expect(await screen.findByDisplayValue(/echo hallo/)).toBeTruthy();
    expect(ipc.aiTaskGenerate).toHaveBeenCalledWith(expect.objectContaining({ description: "Sag hallo", provider: "anthropic" }));

    fireEvent.click(screen.getByText(/Speichern & freigeben/));
    await waitFor(() => expect(ipc.aiTaskApprove).toHaveBeenCalled());
    // The hash comes from what the backend saved — never computed here.
    expect(ipc.aiTaskApprove).toHaveBeenCalledWith(9, "SAVED-HASH");
    expect(ipc.aiTaskSave.mock.calls[0][0]).toMatchObject({ name: "Hallo", language: "bash", script: "echo hallo\n" });
  });

  it("'Nur speichern' never approves", async () => {
    ipc.aiTaskGenerate.mockResolvedValue({ name: "X", language: "zsh", script: "echo\n", explanation: "" });
    ipc.aiTaskSave.mockResolvedValue(task({ id: 3 }));
    mount();
    fireEvent.click(await screen.findByText(/Ersten Task anlegen/));
    fireEvent.change(screen.getByPlaceholderText(/Lösche im Downloads-Ordner/), { target: { value: "x" } });
    fireEvent.click(screen.getByText("Skript erzeugen"));
    await screen.findByLabelText("Skript");
    fireEvent.click(screen.getByText(/Nur speichern/));
    await waitFor(() => expect(ipc.aiTaskSave).toHaveBeenCalled());
    expect(ipc.aiTaskApprove).not.toHaveBeenCalled();
  });

  it("an unapproved task offers approval of the hash it shows and can't be run", async () => {
    ipc.aiTasksState.mockResolvedValue(state([task()]));
    ipc.aiTaskApprove.mockResolvedValue(task({ approved: true }));
    mount();
    fireEvent.click(await screen.findByText("Downloads aufräumen"));
    expect((screen.getByText(/Jetzt ausführen/).closest("button") as HTMLButtonElement).disabled).toBe(true);
    fireEvent.click(screen.getByText(/Diese Fassung freigeben/));
    await waitFor(() => expect(ipc.aiTaskApprove).toHaveBeenCalledWith(7, "HASH-1"));
  });

  it("flags risky scripts before approval", async () => {
    ipc.aiTasksState.mockResolvedValue(state([task({ script: 'rm -rf "$HOME/tmp/x"\n' })]));
    mount();
    fireEvent.click(await screen.findByText("Downloads aufräumen"));
    expect(screen.getByText(/löscht Dateien/)).toBeTruthy();
  });

  it("an argument fills the editor only once focused (not while typing)", async () => {
    const r = mount("räum den Desktop auf", false);
    await screen.findByText(/Ersten Task anlegen/);
    expect(screen.queryByDisplayValue("räum den Desktop auf")).toBeNull();
    r.rerender(<TasksPanel arg="räum den Desktop auf" focused={true} onExit={vi.fn()} onOpenSettings={vi.fn()} />);
    expect(await screen.findByDisplayValue("räum den Desktop auf")).toBeTruthy();
  });

  it("a running generation survives the panel closing", async () => {
    let resolve!: (v: unknown) => void;
    ipc.aiTaskGenerate.mockReturnValue(new Promise((r) => (resolve = r)));
    const r = mount();
    fireEvent.click(await screen.findByText(/Ersten Task anlegen/));
    fireEvent.change(screen.getByPlaceholderText(/Lösche im Downloads-Ordner/), { target: { value: "x" } });
    fireEvent.click(screen.getByText("Skript erzeugen"));
    r.unmount();
    await act(async () => resolve({ name: "Spät", language: "zsh", script: "echo spät\n", explanation: "" }));
    mount();
    expect(await screen.findByDisplayValue(/echo spät/)).toBeTruthy();
  });

  it("Esc steps back from the detail to the list before exiting", async () => {
    ipc.aiTasksState.mockResolvedValue(state([task()]));
    const onExit = vi.fn();
    render(<TasksPanel arg="" focused onExit={onExit} onOpenSettings={vi.fn()} />);
    fireEvent.click(await screen.findByText("Downloads aufräumen"));
    expect(screen.getByText(/Diese Fassung freigeben/)).toBeTruthy();
    fireEvent.keyDown(window, { key: "Escape" });
    expect(await screen.findByText("Neuer Task")).toBeTruthy();
    expect(onExit).not.toHaveBeenCalled();
    fireEvent.keyDown(window, { key: "Escape" });
    expect(onExit).toHaveBeenCalledTimes(1);
  });
});
