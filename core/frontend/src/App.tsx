import { useEffect, useMemo, useRef, useState } from "react";
import { getVersion } from "@tauri-apps/api/app";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { Footer } from "./components/Footer";
import { HistoryList } from "./components/HistoryList";
import { NotesPanel } from "./components/NotesPanel";
import { PreviewPanel } from "./components/PreviewPanel";
import { SearchBar } from "./components/SearchBar";
import { SettingsPanel } from "./components/SettingsPanel";
import { SnippetsPanel } from "./components/SnippetsPanel";
import { useClipboardHistory } from "./hooks/useClipboardHistory";
import { useFuzzySearch } from "./hooks/useFuzzySearch";
import { useKeyboardNav } from "./hooks/useKeyboardNav";
import { useNotes } from "./hooks/useNotes";
import { useSnippets } from "./hooks/useSnippets";
import { tryEvaluate } from "./lib/calc";
import { tryParseColor } from "./lib/colors";
import {
  commandSuggestions,
  isGetShakyTrigger,
  isOpenerTrigger,
  rockTheBoxMode,
  parseCommand,
  parseKillArg,
  parseResizeArg,
  translateUrl,
  type ParsedCommand,
} from "./lib/commands";
import { TOP_OPENERS, pickOpenerIndex } from "./lib/openers";
import { PongGame } from "./components/PongGame";
import { SnakeGame } from "./components/SnakeGame";
import {
  clearHistory,
  deleteEntry,
  findSnippets,
  hidePopup,
  killProcess,
  listProcesses,
  optimizeClipboardImage,
  pasteEntry,
  pasteEntryFormatted,
  pasteSnippet,
  pasteText,
  removeVowelsToClipboard,
  resizeClipboardImage,
  saveClipAsNote,
  systemLock,
  adjustVolume,
  toggleMute,
  systemReboot,
  systemShutdown,
  getThemePreference,
  type ProcessInfo,
} from "./lib/ipc";
import { applyTheme, normaliseTheme } from "./lib/theme";
import { openUrl } from "@tauri-apps/plugin-opener";
import type { ListEntry, Snippet } from "./lib/types";

type Tab = "history" | "snippets" | "notes" | "settings";

function App() {
  const { entries, refresh: refreshHistory } = useClipboardHistory();
  const { snippets, refresh: refreshSnippets } = useSnippets();
  const { notes, categories: noteCategories, refresh: refreshNotes } = useNotes();
  const [query, setQuery] = useState("");
  const [selected, setSelected] = useState(0);
  const [activeTab, setActiveTab] = useState<Tab>("history");
  // Hidden game easter eggs — when non-null, the whole popup is replaced
  // by the matching game. `"pong"` ← typing `getshaky`; the two snake
  // modes ← `rockthebox` (walls kill) / `rockthabox` (wrap-around).
  // Exited only with Esc (handled inside the game).
  const [gameMode, setGameMode] = useState<
    "pong" | "snake-classic" | "snake-wrap" | null
  >(null);
  const [matchingSnippets, setMatchingSnippets] = useState<Snippet[]>([]);
  const [version, setVersion] = useState<string | undefined>(undefined);
  // Sticky banner shown when a paste fails. `"ax"` = macOS Accessibility
  // not granted (we surface it as a clear "fix this in Settings" CTA);
  // `"other"` = anything else (rare; shown as a generic "Paste failed").
  // Auto-dismisses after 8 s.
  const [pasteError, setPasteError] = useState<null | "ax" | "other">(null);
  // Same idea but for OCR — fired by the Rust hotkey handler when
  // Cmd+Shift+O fails because Screen Recording isn't granted.
  // The popup auto-shows + this flag flips → banner directs the
  // user into Settings → Permissions to fix the underlying TCC state.
  const [ocrPermissionMissing, setOcrPermissionMissing] = useState(false);
  // Same pattern again, for the text expander: the Rust hotkey handler
  // fires this when the expander hotkey is pressed but macOS Accessibility
  // isn't granted — otherwise the whole cycle silently no-ops and the
  // hotkey looks dead. We pop the popup + switch to Settings + show this
  // banner so the fix is one click away.
  const [expanderPermissionMissing, setExpanderPermissionMissing] = useState(false);
  const searchRef = useRef<HTMLInputElement>(null);

  // Pulled once from tauri.conf.json via the core:app permission set.
  // Failure (e.g. browser dev preview without Tauri context) is silent —
  // the footer just hides the version chip.
  useEffect(() => {
    getVersion().then(setVersion).catch(() => undefined);
  }, []);

  // Apply the persisted theme preference as early as possible. The
  // popup window is created hidden and only shown on the hotkey, so
  // this IPC round-trip finishes long before the user sees anything —
  // no flash-of-wrong-theme. If the IPC fails (dev preview), the CSS
  // baseline (no data-theme attr → follow OS) is a sane fallback.
  useEffect(() => {
    getThemePreference()
      .then((t) => applyTheme(normaliseTheme(t)))
      .catch(() => undefined);
  }, []);

  // Auto-dismiss the paste-failure banner.
  useEffect(() => {
    if (!pasteError) return;
    const id = window.setTimeout(() => setPasteError(null), 8000);
    return () => window.clearTimeout(id);
  }, [pasteError]);

  const filteredClips = useFuzzySearch(entries, query);

  // Game easter eggs: the instant the query is exactly a magic word,
  // transform the popup into that game. No Enter needed — finishing
  // the word IS the trigger (the words are unmistakable, no false
  // positives). Hidden from autocomplete entirely (see commands.ts).
  useEffect(() => {
    if (isGetShakyTrigger(query)) {
      setGameMode("pong");
      return;
    }
    const snake = rockTheBoxMode(query);
    if (snake) setGameMode(snake === "wrap" ? "snake-wrap" : "snake-classic");
  }, [query]);

  // Inline calculator: when the query parses as a math expression with at
  // least one operator/function/constant, surface the result as the top
  // list item (Alfred-style).
  const calcResult = useMemo(() => tryEvaluate(query), [query]);
  // Inline hex-color preview: same idea — when the query parses as a
  // hex color (#RGB, #RGBA, RRGGBB, RRGGBBAA, …) prepend a color row.
  // Calc and color are mutually exclusive in practice (a math expression
  // can't also be a valid hex literal because of operator characters).
  const colorResult = useMemo(() => tryParseColor(query), [query]);

  // Power-command palette: parse the query into either a complete
  // command (runnable on Enter) or autocomplete suggestions.
  const parsedCommand: ParsedCommand | null = useMemo(
    () => parseCommand(query),
    [query],
  );
  const commandSuggestionList = useMemo(
    () => commandSuggestions(query),
    [query],
  );

  // ── Kill-mode: live process picker ──────────────────────────────────
  // When the parsed command is `kill`, we override the whole combined
  // list with the process picker — the user is clearly in destructive
  // mode and showing clipboard history would just be noise. The picker
  // fetches the live process snapshot on every refresh; cached for the
  // current query.
  const isKillMode = parsedCommand?.spec.kind === "kill";
  const killArgs = useMemo(
    () => (isKillMode ? parseKillArg(parsedCommand!.arg) : null),
    [isKillMode, parsedCommand],
  );
  const [processSnapshot, setProcessSnapshot] = useState<ProcessInfo[]>([]);
  useEffect(() => {
    if (!isKillMode) {
      setProcessSnapshot([]);
      return;
    }
    let cancelled = false;
    listProcesses()
      .then((procs) => {
        if (!cancelled) setProcessSnapshot(procs);
      })
      .catch((e) => {
        console.error("list_processes failed", e);
        if (!cancelled) setProcessSnapshot([]);
      });
    return () => {
      cancelled = true;
    };
    // Re-fetch only when entering/leaving kill mode or when the pattern
    // changes meaningfully — the snapshot is small (~200 processes) and
    // cheap to refresh, but no point hammering it on every keypress.
  }, [isKillMode]);

  const killTargetEntries: ListEntry[] = useMemo(() => {
    if (!isKillMode || !killArgs) return [];
    const pattern = killArgs.pattern.toLowerCase();
    const filtered = pattern
      ? processSnapshot.filter(
          (p) =>
            p.name.toLowerCase().includes(pattern) ||
            p.exe.toLowerCase().includes(pattern),
        )
      : processSnapshot;
    // Cap at 50 visible — anything more is noise; the user should
    // refine the pattern.
    return filtered.slice(0, 50).map(
      (p): ListEntry => ({
        kind: "kill-target",
        data: {
          pid: p.pid,
          name: p.name,
          memory_mb: p.memory_mb,
          exe: p.exe,
          force: killArgs.force,
        },
      }),
    );
  }, [isKillMode, killArgs, processSnapshot]);

  const commandEntry: ListEntry | null = useMemo(() => {
    if (!parsedCommand) return null;
    // kill is rendered via killTargetEntries, not as a single command row.
    if (parsedCommand.spec.kind === "kill") return null;
    const { spec, arg } = parsedCommand;
    let label: string;
    let hint: string;
    switch (spec.kind) {
      case "translate-en":
        label = `Translate to German: "${arg}"`;
        hint = "Opens Google Translate (en → de) in your browser";
        break;
      case "translate-de":
        label = `Translate to English: "${arg}"`;
        hint = "Opens Google Translate (de → en) in your browser";
        break;
      case "translate-auto":
        label = `Translate to German: "${arg}"`;
        hint = "Opens Google Translate (auto-detect → de) in your browser";
        break;
      case "resize": {
        const dims = parseResizeArg(arg);
        label = dims
          ? `Resize clipboard image → ${dims.width}×${dims.height}`
          : `rz: invalid dimensions ("${arg}" — expected W×H, e.g. 1200x800)`;
        hint = dims ? "Lanczos3 sampling · also pushed to History" : "Use format like 1200x800";
        break;
      }
      case "optim":
        label = "Optimise clipboard PNG → ~/Downloads";
        hint = "Lossless oxipng (zopfli + filter selection)";
        break;
      case "rmvvls": {
        const preview = arg.replace(/[aeiouAEIOUäöüÄÖÜ]/g, "");
        label = `Remove vowels: "${arg}" → "${preview}"`;
        hint = "Stripped string lands on your clipboard";
        break;
      }
      case "reboot":
        label = "Restart the system";
        hint = "macOS — confirms before executing (osascript → loginwindow)";
        break;
      case "shutdown":
        label = "Power off the system";
        hint = "macOS — confirms before executing (osascript → loginwindow)";
        break;
      case "lock":
        label = "Lock the screen";
        hint = "macOS — instant, no confirmation (pmset displaysleepnow)";
        break;
      case "mute":
        label = "Toggle system mute";
        hint = "macOS — mutes if unmuted, unmutes if muted";
        break;
      default:
        // kill is handled above; this guards against future additions.
        return null;
    }
    return {
      kind: "command",
      data: {
        commandKind: spec.kind,
        rawInput: query,
        arg,
        label,
        hint,
      },
    };
  }, [parsedCommand, query]);

  // Hidden `opener` easter egg — typing the word surfaces a random
  // German pickup-line from the embedded top-100 list (curated from the
  // nice-to-be-nice VPS DB). The current pick lives in `openerIndex`;
  // `null` means the trigger is inactive. On *first* activation we seed
  // deterministically from the query (re-typing "opener" lands on the
  // same line, predictable). The user then walks the list with ← / →
  // (see the keydown effect below) — subsequent query changes while
  // the trigger still matches do NOT re-seed, so cycling state is
  // preserved as the user adds extra characters.
  const [openerIndex, setOpenerIndex] = useState<number | null>(null);
  const openerActiveRef = useRef(false);
  useEffect(() => {
    const isActive = isOpenerTrigger(query);
    if (isActive && !openerActiveRef.current) {
      const seeded = pickOpenerIndex(query);
      setOpenerIndex(seeded >= 0 ? seeded : 0);
    } else if (!isActive && openerActiveRef.current) {
      setOpenerIndex(null);
    }
    openerActiveRef.current = isActive;
  }, [query]);

  const openerEntry: ListEntry | null = useMemo(() => {
    if (openerIndex === null) return null;
    return { kind: "opener", data: { text: TOP_OPENERS[openerIndex] } };
  }, [openerIndex]);

  const suggestionEntries: ListEntry[] = useMemo(
    () =>
      commandSuggestionList.map((spec) => ({
        kind: "command-suggestion",
        data: {
          keyword: spec.keyword,
          syntax: spec.syntax,
          description: spec.description,
          completion: spec.requiresArg ? `${spec.keyword} ` : spec.keyword,
        },
      })),
    [commandSuggestionList],
  );

  // Combine: in kill mode, the process picker takes over the entire
  // list (no point mixing clipboard history with process rows — they
  // can't be activated the same way and would just confuse selection).
  // Otherwise: command/suggestion first, then calc / color, then
  // snippets, then history clips.
  const combined: ListEntry[] = isKillMode
    ? killTargetEntries
    : [
        ...(openerEntry ? [openerEntry] : []),
        ...(commandEntry ? [commandEntry] : []),
        ...suggestionEntries,
        ...(calcResult ? [{ kind: "calc", data: calcResult } as ListEntry] : []),
        ...(colorResult ? [{ kind: "color", data: colorResult } as ListEntry] : []),
        ...matchingSnippets.map((s): ListEntry => ({ kind: "snippet", data: s })),
        ...filteredClips.map((c): ListEntry => ({ kind: "clip", data: c })),
      ];

  // ← / → cycle through openers while the opener row is selected. Only
  // wired when that's actually true so the search-bar input's normal
  // cursor-movement on Left/Right still works for every other row.
  // Boolean dep (not `combined`) keeps the listener stable across the
  // 60×/sec re-renders that happen while the user types.
  const selectedIsOpener = combined[selected]?.kind === "opener";
  useEffect(() => {
    if (!selectedIsOpener) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== "ArrowRight" && e.key !== "ArrowLeft") return;
      if (e.shiftKey || e.metaKey || e.ctrlKey || e.altKey) return;
      e.preventDefault();
      e.stopPropagation();
      const delta = e.key === "ArrowRight" ? 1 : -1;
      const n = TOP_OPENERS.length;
      setOpenerIndex((cur) => (cur === null ? 0 : ((cur + delta) % n + n) % n));
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [selectedIsOpener]);

  // Find matching snippets whenever query changes.
  useEffect(() => {
    if (!query.trim()) {
      setMatchingSnippets([]);
      return;
    }
    findSnippets(query)
      .then(setMatchingSnippets)
      .catch(() => setMatchingSnippets([]));
  }, [query]);

  useEffect(() => {
    setSelected(0);
  }, [query, entries.length]);

  // Handle window-shown (hotkey): reset to history tab.
  useEffect(() => {
    let unlisten: UnlistenFn | undefined;
    (async () => {
      unlisten = await listen("window-shown", () => {
        setActiveTab("history");
        setQuery("");
        setSelected(0);
        requestAnimationFrame(() => {
          searchRef.current?.focus();
          searchRef.current?.select();
        });
      });
    })();
    return () => unlisten?.();
  }, []);

  // Handle tray "Manage Snippets": switch to snippets tab.
  useEffect(() => {
    let unlisten: UnlistenFn | undefined;
    (async () => {
      unlisten = await listen("open-snippets-tab", () => {
        setActiveTab("snippets");
        void refreshSnippets();
      });
    })();
    return () => unlisten?.();
  }, [refreshSnippets]);

  // Handle tray "Manage Notes": switch to notes tab.
  useEffect(() => {
    let unlisten: UnlistenFn | undefined;
    (async () => {
      unlisten = await listen("open-notes-tab", () => {
        setActiveTab("notes");
        void refreshNotes();
      });
    })();
    return () => unlisten?.();
  }, [refreshNotes]);

  // Backend fires this when the OCR shortcut is pressed but the
  // Screen Recording TCC grant is missing. Switch to Settings (which
  // shows the Permissions overview) and surface a banner so the
  // failure isn't silent.
  useEffect(() => {
    let unlisten: UnlistenFn | undefined;
    (async () => {
      unlisten = await listen("ocr-permission-needed", () => {
        setOcrPermissionMissing(true);
        setActiveTab("settings");
      });
    })();
    return () => unlisten?.();
  }, []);

  // Auto-dismiss the OCR-permission banner after a longer window —
  // 15 s gives the user time to read + click into System Settings.
  useEffect(() => {
    if (!ocrPermissionMissing) return;
    const id = window.setTimeout(() => setOcrPermissionMissing(false), 15000);
    return () => window.clearTimeout(id);
  }, [ocrPermissionMissing]);

  // Backend fires this when the text-expander hotkey is pressed but the
  // Accessibility grant is missing. Switch to Settings (where the
  // Accessibility banner + "Force re-grant" button live) and surface a
  // banner so the failed expansion isn't silent.
  useEffect(() => {
    let unlisten: UnlistenFn | undefined;
    (async () => {
      unlisten = await listen("expander-permission-needed", () => {
        setExpanderPermissionMissing(true);
        setActiveTab("settings");
      });
    })();
    return () => unlisten?.();
  }, []);

  useEffect(() => {
    if (!expanderPermissionMissing) return;
    const id = window.setTimeout(() => setExpanderPermissionMissing(false), 15000);
    return () => window.clearTimeout(id);
  }, [expanderPermissionMissing]);

  const activate = async (i: number, shiftKey = false) => {
    const target = combined[i];
    if (!target) return;
    try {
      if (target.kind === "snippet") {
        await pasteSnippet(target.data.id);
      } else if (target.kind === "calc") {
        await pasteText(target.data.display);
      } else if (target.kind === "color") {
        await pasteText(target.data.pasteValue);
      } else if (target.kind === "opener") {
        // Easter-egg paste — drop the German opener into the focused app.
        await pasteText(target.data.text);
      } else if (target.kind === "command-suggestion") {
        // Autocomplete: don't run anything; just populate the search
        // bar with the command prefix so the user can fill in the
        // argument. Keep the popup open + refocus the input.
        setQuery(target.data.completion);
        requestAnimationFrame(() => {
          searchRef.current?.focus();
          const len = target.data.completion.length;
          searchRef.current?.setSelectionRange(len, len);
        });
        return;
      } else if (target.kind === "command") {
        // Runnable power command. Dispatch by kind.
        const { commandKind, arg } = target.data;
        if (commandKind === "translate-en" || commandKind === "translate-de"
            || commandKind === "translate-auto") {
          const url = translateUrl(commandKind, arg);
          await openUrl(url);
          await hidePopup();
        } else if (commandKind === "resize") {
          const dims = parseResizeArg(arg);
          if (!dims) {
            setPasteError("other");
            return;
          }
          await resizeClipboardImage(dims.width, dims.height);
          await hidePopup();
        } else if (commandKind === "optim") {
          await optimizeClipboardImage();
          await hidePopup();
        } else if (commandKind === "rmvvls") {
          await removeVowelsToClipboard(arg);
          await hidePopup();
        } else if (commandKind === "reboot") {
          // Destructive: native confirmation before firing osascript.
          if (!window.confirm("Restart the system now?\n\nAll unsaved app data may be lost. macOS will show its own confirmation for apps with unsaved changes.")) {
            return;
          }
          await systemReboot();
          await hidePopup();
        } else if (commandKind === "shutdown") {
          if (!window.confirm("Power off the system now?\n\nAll unsaved app data may be lost. macOS will show its own confirmation for apps with unsaved changes.")) {
            return;
          }
          await systemShutdown();
          await hidePopup();
        } else if (commandKind === "lock") {
          // No confirmation: locking is cheap to undo (just type password).
          await systemLock();
          await hidePopup();
        } else if (commandKind === "mute") {
          // Toggle — no confirmation, trivially reversible.
          await toggleMute();
          await hidePopup();
        }
        return;
      } else if (target.kind === "kill-target") {
        // Destructive: confirm before killing. The dialog shows the
        // exact PID + name so the user can't mistake which process
        // they're terminating.
        const { pid, name, force } = target.data;
        const sig = force ? "SIGKILL (force quit)" : "SIGTERM (graceful)";
        if (!window.confirm(`Kill process?\n\n${name}\nPID ${pid}\nSignal: ${sig}`)) {
          return;
        }
        await killProcess(pid, force);
        // Stay open — the user might want to kill another one. Just
        // refresh the snapshot by triggering a re-fetch.
        setProcessSnapshot((cur) => cur.filter((p) => p.pid !== pid));
        return;
      } else {
        // Clipboard entry. Shift+Enter overrides the plain-text setting
        // and forces the original content type (HTML/RTF formatted paste).
        if (shiftKey) {
          await pasteEntryFormatted(target.data.id);
        } else {
          await pasteEntry(target.data.id);
        }
      }
    } catch (e) {
      console.error("activate failed", e);
      // The backend returns the sentinel "ax.permission_denied" when
      // Accessibility isn't granted, so we can show a tailored prompt
      // pointing the user at the Settings tab.
      const msg = String(e);
      if (msg.includes("ax.permission_denied")) {
        setPasteError("ax");
      } else {
        setPasteError("other");
      }
    }
  };

  const onSaveAsNote = async (i: number) => {
    const target = combined[i];
    if (!target || target.kind !== "clip") return;
    try {
      await saveClipAsNote(target.data.id, "", "");
      await refreshNotes();
    } catch (e) {
      console.error("save as note failed", e);
    }
  };

  const onDeleteClip = async (i: number) => {
    const target = combined[i];
    if (!target || target.kind !== "clip") return;
    try {
      await deleteEntry(target.data.id);
      await refreshHistory();
    } catch (e) {
      console.error("delete entry failed", e);
    }
  };

  const onClearAllHistory = async () => {
    try {
      await clearHistory();
      await refreshHistory();
    } catch (e) {
      console.error("clear history failed", e);
    }
  };

  useKeyboardNav({
    length: combined.length,
    selected,
    setSelected,
    onEnter: (shiftKey) => void activate(selected, shiftKey),
    onEscape: () => {
      void hidePopup();
    },
    // Shift+↑ / Shift+↓ adjust the system output volume by ±6 points
    // (≈ macOS's own 1/16 hardware-key step) instead of moving the list
    // selection. Fire-and-forget — macOS plays its volume feedback.
    onShiftArrow: (direction) => {
      void adjustVolume(direction === "up" ? 6 : -6).catch((e) =>
        console.error("adjust_volume failed", e),
      );
    },
    // In game mode the game owns the keyboard — disable the popup nav
    // handler so Esc / arrows don't double-fire.
    enabled: !gameMode,
  });

  const current = combined[selected] ?? null;

  // Game mode — a hidden easter egg fully takes over the app-shell. The
  // game owns all input (mouse + keys); Esc inside it calls onExit,
  // which drops us back to the normal popup with a cleared search field.
  if (gameMode) {
    const exitGame = () => {
      setGameMode(null);
      setQuery("");
      setSelected(0);
      requestAnimationFrame(() => searchRef.current?.focus());
    };
    return (
      <div className="flex h-screen w-screen p-2">
        <div className="app-shell fade-in flex h-full w-full flex-col">
          {gameMode === "pong" ? (
            <PongGame onExit={exitGame} />
          ) : (
            <SnakeGame onExit={exitGame} wrap={gameMode === "snake-wrap"} />
          )}
        </div>
      </div>
    );
  }

  return (
    <div className="flex h-screen w-screen p-2">
      <div className="app-shell fade-in flex h-full w-full flex-col">

        {/* Paste-failure banner — sticky at the top, click-to-dismiss. */}
        {pasteError && (
          <div
            className={
              "flex items-start gap-2 border-b px-4 py-2 text-[12px] " +
              (pasteError === "ax"
                ? "border-amber-500/40 bg-amber-500/10"
                : "border-red-500/40 bg-red-500/10")
            }
          >
            <span className="flex-1">
              {pasteError === "ax" ? (
                <>
                  <b>Paste failed — macOS Accessibility access not granted.</b>{" "}
                  Open the <b>Settings</b> tab and click <b>Force re-grant</b>{" "}
                  in the amber banner. After granting in System Settings, click{" "}
                  <b>Restart now</b>.
                </>
              ) : (
                <b>Paste failed.</b>
              )}
            </span>
            {pasteError === "ax" && (
              <button
                onClick={() => {
                  setActiveTab("settings");
                  setPasteError(null);
                }}
                className="rounded bg-amber-500/30 px-2 py-0.5 text-[11px] font-medium hover:bg-amber-500/40"
              >
                Open Settings
              </button>
            )}
            <button
              onClick={() => setPasteError(null)}
              className="rounded px-1.5 text-[var(--color-muted)] hover:bg-[var(--color-surface)]"
              title="Dismiss"
            >
              ×
            </button>
          </div>
        )}

        {ocrPermissionMissing && (
          <div className="flex items-start gap-2 border-b border-amber-500/40 bg-amber-500/10 px-4 py-2 text-[12px]">
            <span className="flex-1">
              <b>OCR failed — macOS Screen Recording access not granted.</b>{" "}
              Without it, <code>screencapture</code> is denied and the
              region marquee never appears. Grant it in <b>System Settings → Privacy &amp; Security → Screen Recording</b>{" "}
              for Inspector Rust, then relaunch.
            </span>
            <button
              onClick={() => setOcrPermissionMissing(false)}
              className="rounded px-1.5 text-[var(--color-muted)] hover:bg-[var(--color-surface)]"
              title="Dismiss"
            >
              ×
            </button>
          </div>
        )}

        {expanderPermissionMissing && (
          <div className="flex items-start gap-2 border-b border-amber-500/40 bg-amber-500/10 px-4 py-2 text-[12px]">
            <span className="flex-1">
              <b>Text expansion failed — macOS Accessibility access not granted.</b>{" "}
              Inspector Rust can&apos;t read the focused field or type the snippet
              without it. Use <b>Force re-grant</b> in the amber banner below,
              then click <b>Restart now</b>.
            </span>
            <button
              onClick={() => setExpanderPermissionMissing(false)}
              className="rounded px-1.5 text-[var(--color-muted)] hover:bg-[var(--color-surface)]"
              title="Dismiss"
            >
              ×
            </button>
          </div>
        )}

        {/* Header — fixed height, tab buttons anchored top-right */}
        <div className="relative shrink-0">
          {activeTab === "history" ? (
            <SearchBar
              ref={searchRef}
              value={query}
              onChange={setQuery}
              calcMode={calcResult !== null}
            />
          ) : (
            <div className="flex h-14 items-center border-b border-[var(--color-border)] pl-4 pr-[260px]">
              <span className="text-[15px] font-semibold">
                {activeTab === "snippets"
                  ? "Snippets"
                  : activeTab === "notes"
                    ? "Notes"
                    : "Settings"}
              </span>
            </div>
          )}
          <div className="absolute right-3 top-1/2 flex -translate-y-1/2 gap-1">
            <TabButton active={activeTab === "history"} onClick={() => setActiveTab("history")}>
              History
            </TabButton>
            <TabButton active={activeTab === "snippets"} onClick={() => {
              setActiveTab("snippets");
              void refreshSnippets();
            }}>
              Snippets
            </TabButton>
            <TabButton active={activeTab === "notes"} onClick={() => {
              setActiveTab("notes");
              void refreshNotes();
            }}>
              Notes
            </TabButton>
            <TabButton active={activeTab === "settings"} onClick={() => setActiveTab("settings")}>
              Settings
            </TabButton>
          </div>
        </div>

        {/* Content */}
        {activeTab === "history" ? (
          <div className="flex min-h-0 flex-1">
            <div className="w-2/5 border-r border-[var(--color-border)]">
              <HistoryList
                entries={combined}
                selectedIndex={selected}
                onSelect={setSelected}
                onActivate={activate}
                onSaveAsNote={onSaveAsNote}
                onDeleteClip={onDeleteClip}
                onClearAll={onClearAllHistory}
              />
            </div>
            <div className="w-3/5 min-w-0">
              <PreviewPanel entry={current} />
            </div>
          </div>
        ) : activeTab === "snippets" ? (
          <SnippetsPanel snippets={snippets} onRefresh={refreshSnippets} />
        ) : activeTab === "notes" ? (
          <NotesPanel notes={notes} categories={noteCategories} onRefresh={refreshNotes} />
        ) : (
          <SettingsPanel
            onBackupImported={async () => {
              // After a Backup → Import, refresh every list that might
              // have new rows. History reloads itself via the
              // `clipboard-changed` event the watcher emits, but Notes
              // and Snippets need an explicit nudge.
              await Promise.all([refreshHistory(), refreshSnippets(), refreshNotes()]);
            }}
          />
        )}

        <Footer
          index={selected}
          total={
            activeTab === "history"
              ? combined.length
              : activeTab === "snippets"
                ? snippets.length
                : activeTab === "notes"
                  ? notes.length
                  : 0
          }
          version={version}
        />
      </div>
    </div>
  );
}

function TabButton({
  active,
  onClick,
  children,
}: {
  active: boolean;
  onClick: () => void;
  children: React.ReactNode;
}) {
  return (
    <button
      onClick={onClick}
      className={
        "rounded px-2 py-1 text-[11px] font-medium transition-colors whitespace-nowrap " +
        (active
          ? "bg-[var(--color-accent)] text-[var(--color-accent-fg)]"
          : "text-[var(--color-muted)] hover:bg-[var(--color-surface)] hover:text-[var(--color-fg)]")
      }
    >
      {children}
    </button>
  );
}

export default App;
