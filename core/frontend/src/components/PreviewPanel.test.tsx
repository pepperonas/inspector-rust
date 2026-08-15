import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen, fireEvent, cleanup, waitFor } from "@testing-library/react";
import type { ClipEntry, ListEntry } from "../lib/types";

const { commitTransformedText, socialYtdlpAvailable, imageChromaticity } = vi.hoisted(() => ({
  // Typed so `.mock.calls[n][2]` (the transform kind) stays checkable.
  commitTransformedText:
    vi.fn<(text: string, sourceId?: number | null, kind?: string | null) => Promise<void>>(
      async () => undefined,
    ),
  socialYtdlpAvailable: vi.fn(async () => true),
  imageChromaticity: vi.fn(async () => 0.5),
}));
vi.mock("../lib/ipc", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../lib/ipc")>()),
  commitTransformedText,
  socialYtdlpAvailable,
  imageChromaticity,
  getClip: vi.fn(async () => null),
}));
vi.mock("@tauri-apps/plugin-opener", () => ({ openUrl: vi.fn(async () => undefined) }));
vi.mock("@tauri-apps/plugin-clipboard-manager", () => ({ writeText: vi.fn(async () => undefined) }));
vi.mock("@tauri-apps/api/core", () => ({ convertFileSrc: (p: string) => p }));

import { PreviewPanel } from "./PreviewPanel";
import { IS_MAC } from "../lib/platform";

function textClip(text: string, over: Partial<ClipEntry> = {}): ListEntry {
  return {
    kind: "clip",
    data: {
      id: 1,
      content_type: "text",
      content_text: text,
      content_data: text,
      hash: "h1",
      byte_size: text.length,
      created_at: 0,
      last_used_at: 0,
      pinned: false,
      note: null,
      derived_from: null,
      derived_kind: null,
      ...over,
    },
  };
}

/** The mono `<pre>` the clip body renders into. */
const body = () => document.querySelector("pre") as HTMLPreElement;

/** Cmd on macOS, Ctrl elsewhere — the component keys off the same constant. */
const MOD = IS_MAC ? { metaKey: true } : { ctrlKey: true };

afterEach(cleanup);
beforeEach(() => {
  commitTransformedText.mockClear();
  commitTransformedText.mockResolvedValue(undefined);
});

describe("PreviewPanel — long-text cap", () => {
  const CAP = 200_000;

  it("renders a short clip in full, with no opt-in button", () => {
    render(<PreviewPanel entry={textClip("hello world")} />);
    expect(body().textContent).toBe("hello world");
    expect(screen.queryByText(/Show all/)).toBeNull();
  });

  it("renders exactly the cap without truncating a clip that just fits", () => {
    // Boundary: `text.length <= CAP` must render whole — an off-by-one here
    // would put a "Show all" button on a clip that is already complete.
    render(<PreviewPanel entry={textClip("x".repeat(CAP))} />);
    expect(body().textContent?.length).toBe(CAP);
    expect(screen.queryByText(/Show all/)).toBeNull();
  });

  it("caps one character past the limit and offers the opt-in", () => {
    render(<PreviewPanel entry={textClip("x".repeat(CAP + 1))} />);
    expect(body().textContent?.length).toBe(CAP);
    expect(screen.getByText(/Show all/)).toBeTruthy();
  });

  it("names the real size in the opt-in, so the cost is visible before clicking", () => {
    // WebKit lays out the WHOLE wrapped text node — this click can freeze the
    // popup for hundreds of ms, which is exactly why it is opt-in.
    render(<PreviewPanel entry={textClip("x".repeat(250_000))} />);
    const btn = screen.getByText(/Show all/);
    expect(btn.textContent).toContain((250_000).toLocaleString());
    expect(btn.textContent).toMatch(/may take a moment/);
  });

  it("expands to the full text on the opt-in, and drops the button", () => {
    render(<PreviewPanel entry={textClip("x".repeat(250_000))} />);
    fireEvent.click(screen.getByText(/Show all/));

    expect(body().textContent?.length).toBe(250_000);
    expect(screen.queryByText(/Show all/)).toBeNull();
  });

  it("RESETS the opt-in when a different clip is selected", () => {
    // The cap is keyed on the clip id — arrowing from an expanded clip onto
    // another huge one must not inherit the expansion (that would re-freeze).
    const { rerender } = render(<PreviewPanel entry={textClip("x".repeat(250_000))} />);
    fireEvent.click(screen.getByText(/Show all/));
    expect(screen.queryByText(/Show all/)).toBeNull();

    rerender(<PreviewPanel entry={textClip("y".repeat(250_000), { id: 2 })} />);
    expect(screen.getByText(/Show all/)).toBeTruthy();
    expect(body().textContent?.length).toBe(CAP);
  });
});

describe("PreviewPanel — transform bar reveal", () => {
  it("shows a discoverable hint instead of nothing while the modifier is up", () => {
    // Pre-v0.93.2 this slot rendered `null`, so the whole feature was invisible
    // until you already knew about it.
    render(<PreviewPanel entry={textClip("hello")} />);

    const hint = screen.getByTitle(/Copy this entry in another shape/);
    expect(hint.textContent).toMatch(/for formatting options/);
    expect(screen.queryByText("UPPERCASE")).toBeNull();
  });

  it("reveals the chips while the modifier is held, and hides them on release", () => {
    render(<PreviewPanel entry={textClip("hello")} />);

    fireEvent.keyDown(window, { key: "Meta", ...MOD });
    expect(screen.getByText("UPPERCASE")).toBeTruthy();
    expect(screen.getByText("Base64 encode")).toBeTruthy();

    fireEvent.keyUp(window, { key: "Meta", metaKey: false, ctrlKey: false });
    expect(screen.queryByText("UPPERCASE")).toBeNull();
    expect(screen.getByTitle(/Copy this entry in another shape/)).toBeTruthy();
  });

  it("clicking the hint PINS the chips open for mouse users", () => {
    render(<PreviewPanel entry={textClip("hello")} />);
    fireEvent.click(screen.getByTitle(/Copy this entry in another shape/));

    expect(screen.getByText("UPPERCASE")).toBeTruthy();

    // …and they stay open without any modifier held.
    fireEvent.keyUp(window, { key: "Meta", metaKey: false, ctrlKey: false });
    expect(screen.getByText("UPPERCASE")).toBeTruthy();
  });

  it("the pinned bar can be collapsed again", () => {
    render(<PreviewPanel entry={textClip("hello")} />);
    fireEvent.click(screen.getByTitle(/Copy this entry in another shape/));

    fireEvent.click(screen.getByLabelText("Hide the formatting options"));

    expect(screen.queryByText("UPPERCASE")).toBeNull();
    expect(screen.getByTitle(/Copy this entry in another shape/)).toBeTruthy();
  });

  it("offers the same transforms on an RTF clip's plain-text representation", () => {
    render(
      <PreviewPanel
        entry={textClip("Mit freundlichen Grüßen", { content_type: "rtf", content_data: "{\\rtf1}" })}
      />,
    );
    expect(screen.getByText(/RTF formatting will be preserved/)).toBeTruthy();
    fireEvent.click(screen.getByTitle(/Copy this entry in another shape/));
    expect(screen.getByText("UPPERCASE")).toBeTruthy();
  });

  it("withholds the transform bar from an RTF clip with no text representation", () => {
    render(
      <PreviewPanel
        entry={textClip("", { content_type: "rtf", content_text: "", content_data: "{\\rtf1}" })}
      />,
    );
    expect(screen.queryByTitle(/Copy this entry in another shape/)).toBeNull();
  });
});

describe("PreviewPanel — transforms commit a NEW entry with lineage", () => {
  it("records the source id and the transform kind, not just the result", async () => {
    // The lineage rail in the list is drawn from exactly these two arguments.
    render(<PreviewPanel entry={textClip("hello", { id: 77 })} />);
    fireEvent.click(screen.getByTitle(/Copy this entry in another shape/));

    fireEvent.click(screen.getByText("UPPERCASE"));

    await waitFor(() => expect(commitTransformedText).toHaveBeenCalledTimes(1));
    expect(commitTransformedText).toHaveBeenCalledWith("HELLO", 77, "upper");
  });

  it("runs the digit-bound transforms via Cmd/Ctrl+1…9 without opening the bar", async () => {
    render(<PreviewPanel entry={textClip("hello world", { id: 5 })} />);

    fireEvent.keyDown(window, { key: "2", ...MOD }); // 2 = UPPERCASE
    await waitFor(() => expect(commitTransformedText).toHaveBeenCalledTimes(1));
    expect(commitTransformedText).toHaveBeenCalledWith("HELLO WORLD", 5, "upper");

    fireEvent.keyDown(window, { key: "8", ...MOD }); // 8 = Base64 encode
    await waitFor(() => expect(commitTransformedText).toHaveBeenCalledTimes(2));
    expect(commitTransformedText.mock.calls[1][2]).toBe("base64-encode");
  });

  it("binds Cmd/Ctrl+^ to plain-text regardless of the Shift state", async () => {
    // `^` needs Shift on US layouts but is a bare key on German ISO.
    render(<PreviewPanel entry={textClip("hello", { id: 5 })} />);

    fireEvent.keyDown(window, { key: "^", ...MOD });
    await waitFor(() => expect(commitTransformedText).toHaveBeenCalledTimes(1));
    expect(commitTransformedText.mock.calls[0][2]).toBe("plain-text");

    fireEvent.keyDown(window, { key: "^", shiftKey: true, ...MOD });
    await waitFor(() => expect(commitTransformedText).toHaveBeenCalledTimes(2));
  });

  it("ignores Shift+digit, which types punctuation on US layouts", async () => {
    render(<PreviewPanel entry={textClip("hello")} />);
    fireEvent.keyDown(window, { key: "2", shiftKey: true, ...MOD });
    await Promise.resolve();
    expect(commitTransformedText).not.toHaveBeenCalled();
  });

  it("ignores a bare digit — it belongs in the search bar", async () => {
    render(<PreviewPanel entry={textClip("hello")} />);
    fireEvent.keyDown(window, { key: "2" });
    await Promise.resolve();
    expect(commitTransformedText).not.toHaveBeenCalled();
  });

  it("leaves Alt+Cmd/Ctrl+digit alone", async () => {
    render(<PreviewPanel entry={textClip("hello")} />);
    fireEvent.keyDown(window, { key: "2", altKey: true, ...MOD });
    await Promise.resolve();
    expect(commitTransformedText).not.toHaveBeenCalled();
  });

  it("survives a failing commit without taking the preview down", async () => {
    const err = vi.spyOn(console, "error").mockImplementation(() => {});
    commitTransformedText.mockRejectedValueOnce(new Error("db locked"));
    try {
      render(<PreviewPanel entry={textClip("hello")} />);
      fireEvent.keyDown(window, { key: "2", ...MOD });

      await waitFor(() => expect(err).toHaveBeenCalled());
      expect(body().textContent).toBe("hello"); // still rendered
    } finally {
      err.mockRestore();
    }
  });
});
