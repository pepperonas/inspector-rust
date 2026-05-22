/**
 * IPC wrapper contract tests.
 *
 * Each wrapper in `ipc.ts` calls `invoke("<rust_command_name>", {…})`
 * to reach a Rust `#[tauri::command]`. The two halves are wired by
 * exact string + the snake_case argument keys Tauri expects — a typo
 * on either side silently breaks the IPC. These tests pin the
 * contract: command name + argument shape + default values + result
 * pass-through + error propagation.
 *
 * Sample is representative, not exhaustive — one test per IPC
 * "namespace" (history / snippets / notes / settings / expander /
 * permissions / lifecycle) is enough to catch the typo class.
 */
import { describe, it, expect, vi, beforeEach } from "vitest";

// Mock the Tauri core BEFORE importing ipc.ts so the wrappers close
// over the mocked invoke. The factory runs at hoisted-mock time.
vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
}));

import { invoke } from "@tauri-apps/api/core";
import * as ipc from "./ipc";

const mockInvoke = vi.mocked(invoke);

beforeEach(() => {
  mockInvoke.mockReset();
  // Default: every wrapper that doesn't have its own mockResolvedValue
  // in a test resolves to undefined, mirroring void-returning commands.
  mockInvoke.mockResolvedValue(undefined);
});

describe("ipc — history wrappers", () => {
  it("getHistory passes explicit limit + offset", async () => {
    mockInvoke.mockResolvedValue([]);
    await ipc.getHistory(100, 50);
    expect(mockInvoke).toHaveBeenCalledTimes(1);
    expect(mockInvoke).toHaveBeenCalledWith("get_history", {
      limit: 100,
      offset: 50,
    });
  });

  it("getHistory has sensible defaults (limit=500, offset=0)", async () => {
    mockInvoke.mockResolvedValue([]);
    await ipc.getHistory();
    expect(mockInvoke).toHaveBeenCalledWith("get_history", {
      limit: 500,
      offset: 0,
    });
  });

  it("searchHistory passes the query string + limit", async () => {
    mockInvoke.mockResolvedValue([]);
    await ipc.searchHistory("hello", 200);
    expect(mockInvoke).toHaveBeenCalledWith("search_history", {
      query: "hello",
      limit: 200,
    });
  });

  it("deleteEntry passes the id", async () => {
    await ipc.deleteEntry(42);
    expect(mockInvoke).toHaveBeenCalledWith("delete_entry", { id: 42 });
  });

  it("clearHistory takes no args", async () => {
    await ipc.clearHistory();
    expect(mockInvoke).toHaveBeenCalledWith("clear_history");
  });

  it("pasteEntry / pasteEntryFormatted hit distinct commands", async () => {
    await ipc.pasteEntry(7);
    await ipc.pasteEntryFormatted(7);
    expect(mockInvoke).toHaveBeenNthCalledWith(1, "paste_entry", { id: 7 });
    expect(mockInvoke).toHaveBeenNthCalledWith(2, "paste_entry_formatted", { id: 7 });
  });
});

describe("ipc — snippets wrappers", () => {
  it("listSnippets takes no args", async () => {
    mockInvoke.mockResolvedValue([]);
    await ipc.listSnippets();
    expect(mockInvoke).toHaveBeenCalledWith("list_snippets");
  });

  it("findSnippets passes the query", async () => {
    mockInvoke.mockResolvedValue([]);
    await ipc.findSnippets("sig");
    expect(mockInvoke).toHaveBeenCalledWith("find_snippets", { query: "sig" });
  });

  it("upsertSnippet passes all four fields", async () => {
    mockInvoke.mockResolvedValue(1);
    await ipc.upsertSnippet(null, "hi", "Greeting", "Hello there!");
    expect(mockInvoke).toHaveBeenCalledWith("upsert_snippet", {
      id: null,
      abbreviation: "hi",
      title: "Greeting",
      body: "Hello there!",
    });
  });

  it("deleteSnippet passes the id", async () => {
    await ipc.deleteSnippet(9);
    expect(mockInvoke).toHaveBeenCalledWith("delete_snippet", { id: 9 });
  });
});

describe("ipc — notes wrappers", () => {
  it("listNotes takes no args", async () => {
    mockInvoke.mockResolvedValue([]);
    await ipc.listNotes();
    expect(mockInvoke).toHaveBeenCalledWith("list_notes");
  });

  it("createNote passes title/body/category as snake_case-safe payload", async () => {
    mockInvoke.mockResolvedValue(11);
    await ipc.createNote("My note", "Body text", "ideas");
    expect(mockInvoke).toHaveBeenCalledWith("create_note", {
      title: "My note",
      body: "Body text",
      category: "ideas",
    });
  });

  it("saveClipAsNote maps the JS clipId arg to snake_case clipId field", async () => {
    // Tauri's auto-conversion expects the JS payload key to match the
    // Rust parameter name. The wrapper sends `clipId` as the key —
    // this test pins the spelling so a future rename breaks loudly.
    mockInvoke.mockResolvedValue(22);
    await ipc.saveClipAsNote(5, "From clip", "saved");
    expect(mockInvoke).toHaveBeenCalledWith("save_clip_as_note", {
      clipId: 5,
      title: "From clip",
      category: "saved",
    });
  });

  it("deleteNote / pasteNote both pass id", async () => {
    await ipc.deleteNote(3);
    await ipc.pasteNote(4);
    expect(mockInvoke).toHaveBeenNthCalledWith(1, "delete_note", { id: 3 });
    expect(mockInvoke).toHaveBeenNthCalledWith(2, "paste_note", { id: 4 });
  });
});

describe("ipc — settings + capture wrappers", () => {
  it("toggleCapture passes the paused flag", async () => {
    await ipc.toggleCapture(true);
    expect(mockInvoke).toHaveBeenCalledWith("toggle_capture", { paused: true });
  });

  it("setPastePlainTextOnly passes the boolean value", async () => {
    await ipc.setPastePlainTextOnly(false);
    expect(mockInvoke).toHaveBeenCalledWith("set_paste_plain_text_only", {
      value: false,
    });
  });

  it("getThemePreference / setThemePreference round-trip the theme key", async () => {
    mockInvoke.mockResolvedValue("dark");
    const t = await ipc.getThemePreference();
    expect(t).toBe("dark");
    expect(mockInvoke).toHaveBeenCalledWith("get_theme_preference");

    mockInvoke.mockReset();
    mockInvoke.mockResolvedValue(undefined);
    await ipc.setThemePreference("light");
    expect(mockInvoke).toHaveBeenCalledWith("set_theme_preference", {
      theme: "light",
    });
  });
});

describe("ipc — expander wrappers", () => {
  it("getExpanderConfig takes no args", async () => {
    mockInvoke.mockResolvedValue({ enabled: true, hotkey: "Alt+Digit1", accessibility_granted: true });
    await ipc.getExpanderConfig();
    expect(mockInvoke).toHaveBeenCalledWith("get_expander_config");
  });

  it("setExpanderConfig passes enabled + hotkey", async () => {
    mockInvoke.mockResolvedValue({ enabled: false, hotkey: "Alt+Digit2", accessibility_granted: true });
    await ipc.setExpanderConfig(false, "Alt+Digit2");
    expect(mockInvoke).toHaveBeenCalledWith("set_expander_config", {
      enabled: false,
      hotkey: "Alt+Digit2",
    });
  });
});

describe("ipc — permissions + lifecycle wrappers", () => {
  it("getAccessibilityStatus / forceResetAndRequestGrant hit distinct commands", async () => {
    mockInvoke.mockResolvedValue(true);
    await ipc.getAccessibilityStatus();
    expect(mockInvoke).toHaveBeenNthCalledWith(1, "get_accessibility_status");

    mockInvoke.mockReset();
    mockInvoke.mockResolvedValue(true);
    await ipc.forceResetAndRequestGrant();
    expect(mockInvoke).toHaveBeenCalledWith("force_reset_and_request_grant");
  });

  it("openAccessibilitySettings / openScreenRecordingSettings are distinct, both args-free", async () => {
    await ipc.openAccessibilitySettings();
    await ipc.openScreenRecordingSettings();
    expect(mockInvoke).toHaveBeenNthCalledWith(1, "open_accessibility_settings");
    expect(mockInvoke).toHaveBeenNthCalledWith(2, "open_screen_recording_settings");
  });

  it("setAutostartEnabled passes the enabled bool", async () => {
    mockInvoke.mockResolvedValue(true);
    await ipc.setAutostartEnabled(true);
    expect(mockInvoke).toHaveBeenCalledWith("set_autostart_enabled", {
      enabled: true,
    });
  });

  it("relaunchApp + quitApp are args-free and named correctly", async () => {
    await ipc.relaunchApp();
    await ipc.quitApp();
    expect(mockInvoke).toHaveBeenNthCalledWith(1, "relaunch_app");
    expect(mockInvoke).toHaveBeenNthCalledWith(2, "quit_app");
  });
});

describe("ipc — return values + errors pass through unchanged", () => {
  it("the wrapper resolves to whatever invoke resolves to", async () => {
    const fakeRows = [{ id: 1, content_type: "text" as const }];
    mockInvoke.mockResolvedValue(fakeRows);
    await expect(ipc.getHistory()).resolves.toBe(fakeRows);
  });

  it("an invoke rejection propagates from the wrapper", async () => {
    mockInvoke.mockRejectedValue(new Error("tauri ipc died"));
    await expect(ipc.deleteEntry(1)).rejects.toThrow("tauri ipc died");
  });
});
