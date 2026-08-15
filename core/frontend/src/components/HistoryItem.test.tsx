import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen, fireEvent, cleanup, act } from "@testing-library/react";
import type { ClipEntry, ListEntry } from "../lib/types";

// The `app` row lazily fetches its icon; keep it off the network and never
// resolving so the lucide fallback is what renders (asserted below).
const { getAppIcon } = vi.hoisted(() => ({ getAppIcon: vi.fn(() => new Promise<string>(() => {})) }));
vi.mock("../lib/ipc", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../lib/ipc")>()),
  getAppIcon,
}));

import { HistoryItem } from "./HistoryItem";

function clip(over: Partial<ClipEntry> = {}): ClipEntry {
  return {
    id: 1,
    content_type: "text",
    content_text: "hello world",
    content_data: "hello world",
    hash: "h1",
    byte_size: 11,
    created_at: 1_700_000_000,
    last_used_at: 1_700_000_000,
    pinned: false,
    note: null,
    derived_from: null,
    derived_kind: null,
    ...over,
  };
}

/**
 * One fixture per `ListEntry` kind. `command`-keyword kinds are separated from
 * the neutral ones because the accent split between them is a documented
 * invariant (see CLAUDE.md → "Frontend data flow and `ListEntry` union").
 */
const KEYWORD_COMMAND_ROWS: Record<string, ListEntry> = {
  command: {
    kind: "command",
    data: { commandKind: "optim", rawInput: "optim", arg: "", label: "Optimise", hint: "PNG/JPEG" },
  },
  "command-suggestion": {
    kind: "command-suggestion",
    data: { keyword: "clean", syntax: "clean", description: "Free disk space", completion: "clean" },
  },
  help: { kind: "help", data: { command: "kill", tagline: "End a process", category: "System" } },
  "totp-manage": { kind: "totp-manage", data: { label: "2FA manager" } },
  totp: {
    kind: "totp",
    data: {
      id: 7, issuer: "Hostinger", account: "me@example.com",
      digits: 6, period: 30, code: "123456", seconds_remaining: 21,
    },
  },
  pwgen: { kind: "pwgen", data: { length: 12, mode: "all", password: "aB3!xQ7#pLm2" } },
  bruno: {
    kind: "bruno",
    data: {
      yearlyGross: 60000, period: "monthly", netYear: 36000, netMonth: 3000,
      totalDeductions: 24000, deductionRate: 0.4, marginalRate: 0.42,
      social: { health: 1, care: 1, pension: 1, unemployment: 1 },
      incomeTax: 1, soli: 0, churchTax: 0,
      taxClass: 1, state: "NW", children: 0, isChurchMember: false,
    },
  },
  bpm: { kind: "bpm", data: { label: "BPM detector" } },
  equalizer: { kind: "equalizer", data: { label: "Equalizer" } },
  "kill-target": {
    kind: "kill-target",
    data: { pid: 4242, name: "Slack", memory_mb: 812.34, exe: "/Applications/Slack.app", force: false },
  },
  meme: { kind: "meme", data: { name: "facepalm", category: "reactions", path: "/m/reactions/facepalm.gif" } },
  // figlet's font gallery is the same whole-list takeover as kill/meme. It was
  // left out of `isCustomCommand` when figlet landed (v0.85.0) — fixed
  // 2026-08-15; this row is the regression guard.
  "figlet-font": {
    kind: "figlet-font",
    data: { name: "slant", category: "Classic", popular: true, sample: "  _/_/\n _/\n", pinned: false },
  },
  social: { kind: "social", data: { platform: "youtube", url: "https://youtu.be/abc" } },
  // Not a keyword, but deliberately given the command treatment in v0.84.27 —
  // typing an expression should feel as "active" as typing a keyword.
  calc: { kind: "calc", data: { expression: "2+2", value: 4, display: "4" } },
};

const NEUTRAL_ROWS: Record<string, ListEntry> = {
  clip: { kind: "clip", data: clip() },
  snippet: {
    kind: "snippet",
    data: { id: 3, abbreviation: "mfg", title: "Sign-off", body: "Mit freundlichen Grüßen", created_at: 0, updated_at: 0 },
  },
  color: {
    kind: "color",
    data: {
      hex: "#ff0000", pasteValue: "#FF0000", r: 255, g: 0, b: 0, a: 1,
      hsl: { h: 0, s: 100, l: 50 }, rgbString: "rgb(255, 0, 0)", hslString: "hsl(0, 100%, 50%)",
    },
  },
  opener: { kind: "opener", data: { text: "Bist du ein Magnet?" } },
  app: { kind: "app", data: { name: "Safari", path: "/Applications/Safari.app" } },
  "finder-file": {
    kind: "finder-file",
    data: { path: "/tmp/a.png", name: "a.png", size_bytes: 2048, is_image: true },
  },
};

function renderRow(entry: ListEntry, selected = false) {
  const { container } = render(
    <HistoryItem entry={entry} selected={selected} onClick={() => {}} onDoubleClick={() => {}} />,
  );
  return container.firstElementChild as HTMLElement;
}

const isRose = (el: HTMLElement) => /\brose-/.test(el.className);

afterEach(cleanup);
// NB: block body — an expression body would RETURN the mock, and vitest treats
// a hook's return value as a teardown callback (it would then invoke
// `getAppIcon()` and await its never-resolving promise → hook timeout).
beforeEach(() => {
  getAppIcon.mockClear();
});

describe("HistoryItem — the custom-command accent invariant", () => {
  // CLAUDE.md: "Every keyword-triggered command row is rendered with a reddish
  // (rose) accent so it's visually obvious you're about to trigger a command
  // rather than paste a clip / launch an app." A kind silently dropping out of
  // `isCustomCommand` is a real (and easily unnoticed) regression.
  for (const [kind, entry] of Object.entries(KEYWORD_COMMAND_ROWS)) {
    it(`renders "${kind}" with the rose command accent`, () => {
      expect(isRose(renderRow(entry))).toBe(true);
    });

    it(`renders "${kind}" as a SOLID rose row when selected`, () => {
      const row = renderRow(entry, true);
      expect(row.className).toContain("bg-rose-600");
      expect(row.className).not.toContain("bg-[var(--color-accent)]");
    });
  }

  for (const [kind, entry] of Object.entries(NEUTRAL_ROWS)) {
    it(`keeps "${kind}" on the neutral accent`, () => {
      expect(isRose(renderRow(entry))).toBe(false);
    });
  }

  it("uses the neutral accent — not rose — for a SELECTED clip", () => {
    const row = renderRow(NEUTRAL_ROWS.clip, true);
    expect(row.className).toContain("bg-[var(--color-accent)]");
    expect(row.className).not.toContain("bg-rose-600");
  });

  it("gives command rows the entrance animation, and clips none", () => {
    expect(renderRow(KEYWORD_COMMAND_ROWS.command).className).toContain("md3-cmd-enter");
    expect(renderRow(NEUTRAL_ROWS.clip).className).not.toContain("md3-cmd-enter");
  });

  it("keeps the destructive kill row on the more alarming red-500 chip", () => {
    // Deliberately NOT rose: `kill` is the one destructive picker.
    render(
      <HistoryItem
        entry={KEYWORD_COMMAND_ROWS["kill-target"]}
        selected={false}
        onClick={() => {}}
        onDoubleClick={() => {}}
      />,
    );
    expect(screen.getByText("kill").className).toContain("red-500");
  });
});

describe("HistoryItem — row chips + labels", () => {
  it("labels each kind with its own chip", () => {
    const expected: [string, string][] = [
      ["command", "cmd"],
      ["command-suggestion", "hint"],
      ["totp-manage", "2fa"],
      ["totp", "otp"],
      ["pwgen", "pwgen"],
      ["bruno", "bruno"],
      ["bpm", "bpm"],
      ["calc", "calc"],
    ];
    for (const [kind, chip] of expected) {
      const { unmount } = render(
        <HistoryItem
          entry={KEYWORD_COMMAND_ROWS[kind]}
          selected={false}
          onClick={() => {}}
          onDoubleClick={() => {}}
        />,
      );
      expect(screen.getByText(chip), `${kind} → "${chip}" chip`).toBeTruthy();
      unmount();
    }
  });

  it("shows the meme's folder as its chip, falling back to 'meme' at top level", () => {
    renderRow(KEYWORD_COMMAND_ROWS.meme);
    expect(screen.getByText("reactions")).toBeTruthy();
    cleanup();
    renderRow({ kind: "meme", data: { name: "x", category: "", path: "/m/x.gif" } });
    expect(screen.getByText("meme")).toBeTruthy();
  });

  it("marks a force-kill row as `kill -9`", () => {
    renderRow({
      kind: "kill-target",
      data: { pid: 1, name: "Stuck", memory_mb: 1, exe: "", force: true },
    });
    expect(screen.getByText("kill -9")).toBeTruthy();
  });

  it("shows the ? help affordance only on a SELECTED command row", () => {
    renderRow(KEYWORD_COMMAND_ROWS.command, true);
    expect(screen.getByLabelText("Press ? for help")).toBeTruthy();
    cleanup();
    renderRow(KEYWORD_COMMAND_ROWS.command, false);
    expect(screen.queryByLabelText("Press ? for help")).toBeNull();
  });

  it("tags styled clips with their format, and leaves plain text untagged", () => {
    renderRow({ kind: "clip", data: clip({ content_type: "html" }) });
    expect(screen.getByTitle("Styled HTML content")).toBeTruthy();
    cleanup();
    renderRow({ kind: "clip", data: clip({ content_type: "rtf" }) });
    expect(screen.getByTitle("Styled RTF content")).toBeTruthy();
    cleanup();
    renderRow({ kind: "clip", data: clip({ content_type: "text" }) });
    expect(screen.queryByTitle(/^Styled /)).toBeNull();
  });

  it("highlights a noted clip in amber and surfaces the note as a tooltip", () => {
    const row = renderRow({ kind: "clip", data: clip({ note: "check this later" }) });
    expect(row.className).toContain("amber");
    expect(screen.getByTitle("check this later")).toBeTruthy();
  });
});

describe("HistoryItem — clip row actions", () => {
  it("pin / delete / save-as-note fire WITHOUT also selecting the row", () => {
    // Each action lives inside the row, whose onClick selects it — a missing
    // stopPropagation would make every icon click also move the selection.
    const onClick = vi.fn();
    const onTogglePin = vi.fn();
    const onDelete = vi.fn();
    const onSaveAsNote = vi.fn();
    render(
      <HistoryItem
        entry={{ kind: "clip", data: clip({ pinned: false }) }}
        selected={false}
        onClick={onClick}
        onDoubleClick={() => {}}
        onTogglePin={onTogglePin}
        onDelete={onDelete}
        onSaveAsNote={onSaveAsNote}
      />,
    );

    fireEvent.click(screen.getByTitle("Pin to top"));
    expect(onTogglePin).toHaveBeenCalledWith(true); // toggles to the opposite

    fireEvent.click(screen.getByTitle("Save as note"));
    expect(onSaveAsNote).toHaveBeenCalledTimes(1);

    fireEvent.click(screen.getByTitle("Delete entry from history"));
    expect(onDelete).toHaveBeenCalledTimes(1);

    expect(onClick).not.toHaveBeenCalled();
  });

  it("un-pins an already-pinned clip", () => {
    const onTogglePin = vi.fn();
    render(
      <HistoryItem
        entry={{ kind: "clip", data: clip({ pinned: true }) }}
        selected={false}
        onClick={() => {}}
        onDoubleClick={() => {}}
        onTogglePin={onTogglePin}
      />,
    );
    fireEvent.click(screen.getByTitle("Unpin"));
    expect(onTogglePin).toHaveBeenCalledWith(false);
  });

  it("confirms a save-as-note in place, then reverts", () => {
    vi.useFakeTimers();
    try {
      render(
        <HistoryItem
          entry={{ kind: "clip", data: clip() }}
          selected={false}
          onClick={() => {}}
          onDoubleClick={() => {}}
          onSaveAsNote={() => {}}
        />,
      );
      fireEvent.click(screen.getByTitle("Save as note"));
      expect(screen.getByTitle("Saved!")).toBeTruthy();

      // The feedback is transient — it must not stick on a recycled row.
      act(() => vi.advanceTimersByTime(1500));
      expect(screen.getByTitle("Save as note")).toBeTruthy();
    } finally {
      vi.useRealTimers();
    }
  });

  it("offers no clip actions on a non-clip row even when the handlers are passed", () => {
    render(
      <HistoryItem
        entry={KEYWORD_COMMAND_ROWS.command}
        selected={false}
        onClick={() => {}}
        onDoubleClick={() => {}}
        onTogglePin={vi.fn()}
        onDelete={vi.fn()}
        onSaveAsNote={vi.fn()}
      />,
    );
    expect(screen.queryByTitle("Pin to top")).toBeNull();
    expect(screen.queryByTitle("Delete entry from history")).toBeNull();
    expect(screen.queryByTitle("Save as note")).toBeNull();
  });

  it("toggles the time chip between relative and absolute without selecting the row", () => {
    const onClick = vi.fn();
    render(
      <HistoryItem
        entry={{ kind: "clip", data: clip() }}
        selected={false}
        onClick={onClick}
        onDoubleClick={() => {}}
      />,
    );
    const chip = screen.getByTitle(/^Captured:/);
    const relative = chip.textContent;

    fireEvent.click(chip);
    expect(chip.textContent).not.toBe(relative); // now the absolute date
    expect(onClick).not.toHaveBeenCalled();

    fireEvent.click(chip);
    expect(chip.textContent).toBe(relative); // and back
  });

  it("tells a never-reused clip apart from a re-used one in the time tooltip", () => {
    renderRow({ kind: "clip", data: clip({ created_at: 1000, last_used_at: 1000 }) });
    expect(screen.getByTitle(/never re-used since/)).toBeTruthy();
    cleanup();
    renderRow({ kind: "clip", data: clip({ created_at: 1000, last_used_at: 2000 }) });
    expect(screen.getByTitle(/Last used:/)).toBeTruthy();
  });
});

describe("HistoryItem — lineage rails", () => {
  const rails = [{ lane: 0, color: "#f00", node: true }];

  it("reserves the gutter the list asked for", () => {
    const { container } = render(
      <HistoryItem
        entry={{ kind: "clip", data: clip({ derived_from: 9, derived_kind: "base64" }) }}
        selected={false}
        onClick={() => {}}
        onDoubleClick={() => {}}
        rails={rails}
        railGutter={10}
      />,
    );
    expect((container.firstElementChild as HTMLElement).style.paddingLeft).toBe("22px"); // 12 + 10
  });

  it("names the transform that produced the clip in the rail tooltip", () => {
    render(
      <HistoryItem
        entry={{ kind: "clip", data: clip({ derived_from: 9, derived_kind: "base64-encode" }) }}
        selected={false}
        onClick={() => {}}
        onDoubleClick={() => {}}
        rails={rails}
        railGutter={5}
      />,
    );
    expect(screen.getByTitle("Base64 encode")).toBeTruthy();
  });

  it("falls back to a generic tooltip for an organically captured clip", () => {
    render(
      <HistoryItem
        entry={{ kind: "clip", data: clip() }}
        selected={false}
        onClick={() => {}}
        onDoubleClick={() => {}}
        rails={rails}
        railGutter={5}
      />,
    );
    expect(screen.getByTitle("Copied from another entry")).toBeTruthy();
  });

  it("draws nothing (and adds no padding) when the rails are off", () => {
    const { container } = render(
      <HistoryItem
        entry={{ kind: "clip", data: clip() }}
        selected={false}
        onClick={() => {}}
        onDoubleClick={() => {}}
      />,
    );
    const row = container.firstElementChild as HTMLElement;
    expect(row.style.paddingLeft).toBe("");
    expect(screen.queryByTitle("Copied from another entry")).toBeNull();
  });
});
