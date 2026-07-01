import { useEffect, useRef, useState } from "react";
import { getVersion } from "@tauri-apps/api/app";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { open as openDialog, save as saveDialog } from "@tauri-apps/plugin-dialog";
import {
  AlertTriangle,
  Archive,
  Camera,
  Clock,
  Lock,
  CheckCircle2,
  ClipboardType,
  Download,
  Euro,
  Info,
  Keyboard,
  Monitor,
  Moon,
  PlayCircle,
  Power,
  Smile,
  Sun,
  SunMoon,
  Trash2,
  AlarmClock,
  Hand,
  LayoutGrid,
  Upload,
  Volume2,
  Wand2,
  Zap,
} from "lucide-react";
import { AboutContent } from "./AboutContent";
import { IS_LINUX, IS_MAC } from "../lib/platform";
import { LinuxShortcutsSettings } from "./LinuxShortcutsSettings";
import {
  diagnoseExpandAtCursor,
  forceResetAndRequestGrant,
  brunoGetDefaults,
  brunoSetDefaults,
  getMemeDir,
  setMemeDir,
  getTimesheetConfig,
  setTimesheetConfig,
  trackCategoryRules,
  trackDeleteCategoryRule,
  forceResetFinderAutomationGrant,
  forceResetScreenRecordingGrant,
  getAccessibilityStatus,
  getFinderAutomationStatus,
  type BrunoDefaults,
  getAutoExpandConfig,
  getGestureConfig,
  setGestureConfig,
  type GestureConfig,
  getKeepaliveEnabled,
  setKeepaliveEnabled,
  getWindowSnapConfig,
  setWindowSnapConfig,
  type WindowSnapConfig,
  getWindowPaletteConfig,
  setWindowPaletteConfig,
  type WindowPaletteConfig,
  getAutostartEnabled,
  getCleanerConfig,
  cleanerCategories,
  getDirectSlots,
  getExpanderConfig,
  getInputLockChord,
  getOcrSaveSourceImage,
  getPastePlainTextOnly,
  getPopupHotkey,
  getPopupHotkeyDefault,
  getHistoryHotkey,
  getHistoryHotkeyDefault,
  setHistoryHotkey,
  getScreenRecordingStatus,
  getSoundEnabled,
  setSoundEnabled,
  getAlarmStyle,
  setAlarmStyle,
  type AlarmStyle,
  getThemePreference,
  getClipboardPrivacy,
  setClipboardPrivacy,
  type ClipboardPrivacy,
  importBackup,
  listSnippets,
  openAccessibilitySettings,
  openFinderAutomationSettings,
  openScreenRecordingSettings,
  quitApp,
  relaunchApp,
  saveBackupToFile,
  setAutoExpandConfig,
  setCleanerConfig,
  setAutostartEnabled,
  setDirectSlots,
  setExpanderConfig,
  setInputLockChord as ipcSetInputLockChord,
  setOcrSaveSourceImage,
  getWindowSizePreference,
  setPastePlainTextOnly,
  setPopupHotkey,
  setSuppressHide,
  setThemePreference,
  setWindowSizePreference,
  type AutoExpandConfig,
  type CleanerCategory,
  type CleanerConfig,
  type CleanerLevel,
  type DiagnoseResult,
  type DirectSlot,
  type ExpanderConfig,
} from "../lib/ipc";
import { applyTheme, normaliseTheme, type ThemePreference } from "../lib/theme";
import { MEME_ENABLED } from "../lib/meme";
import type { BackupImportResult, Snippet } from "../lib/types";
import { formatBytes } from "../lib/format";
import { HotkeyCapture } from "./HotkeyCapture";
import { GlobalShortcutsSection } from "./GlobalShortcutsSection";

// Must match `expander::DEFAULT_HOTKEY` in the Rust core. `Digit1` is the
// `1`-row key (not the numpad) — layout-stable everywhere, no dead-key /
// reserved-combo surprises. Shown to the user as "Alt+1".
const DEFAULT_HOTKEY = "Alt+Digit1";

// One-click presets so the user doesn't have to fight the capture widget
// for the common case. `value` is the W3C `KeyboardEvent.code` string the
// backend stores; `label` is what we render.
const QUICK_HOTKEYS: ReadonlyArray<{ label: string; value: string }> = [
  { label: "Alt+1", value: "Alt+Digit1" },
  { label: "Alt+2", value: "Alt+Digit2" },
  { label: "Alt+3", value: "Alt+Digit3" },
];

/** Render a stored hotkey code-string ("Alt+Digit1") in the friendly form
 *  ("Alt+1") used in tooltips / status text. */
function prettyHotkey(code: string): string {
  return code.replace(/\bDigit(\d)\b/g, "$1").replace(/\bKey([A-Z])\b/g, "$1");
}

interface Props {
  /** Notes-tab refresh — used to reflect imported notes immediately. */
  onBackupImported?: () => Promise<void> | void;
}

export function SettingsPanel({ onBackupImported }: Props = {}) {
  const [cfg, setCfg] = useState<ExpanderConfig | null>(null);
  const [hotkey, setHotkey] = useState<string>(DEFAULT_HOTKEY);
  const [enabled, setEnabled] = useState(false);
  const [accessibility, setAccessibility] = useState<boolean | null>(null);
  // Passive auto-expansion (aText-style, v0.56.0). Null until loaded.
  const [aeCfg, setAeCfg] = useState<AutoExpandConfig | null>(null);
  const [aeSaving, setAeSaving] = useState(false);
  // Cleaning workflow (v0.60.0).
  const [cleanCfg, setCleanCfg] = useState<CleanerConfig | null>(null);
  const [cleanCats, setCleanCats] = useState<CleanerCategory[]>([]);
  // Independent of Accessibility: macOS gates `screencapture -i`
  // (the OCR region picker) behind the Screen Recording TCC policy.
  // Granting Accessibility doesn't unlock this — they're separate
  // grants, polled independently.
  const [screenRec, setScreenRec] = useState<boolean | null>(null);
  // Automation → Finder (TCC AppleEvents). Gates Ctrl+Shift+F (read
  // the current Finder selection via osascript). Probing this fires
  // the macOS Automation prompt the first time after install — there's
  // no separate "not determined" state in the AppleEvents policy.
  // We let the probe fire on mount; the prompt copy
  // (NSAppleEventsUsageDescription) explains why.
  const [finderAutomation, setFinderAutomation] = useState<boolean | null>(null);
  // `chaining` is true while the "Set up permissions" button walks the
  // user through all grants: when one flips to granted, the chaining
  // effect auto-opens the next still-missing System Settings pane, so
  // one click guides through everything.
  const [chaining, setChaining] = useState(false);
  const chainOpenedScreenRec = useRef(false);
  const chainOpenedFinder = useRef(false);
  // Set to true when polling detects a false→true transition. Drives the
  // "Access detected — restart Inspector Rust to activate?" prompt: macOS caches
  // AXIsProcessTrusted per-process, so the running Inspector Rust can't actually
  // *use* the just-granted permission until it's relaunched.
  const [justGranted, setJustGranted] = useState(false);
  const [restarting, setRestarting] = useState(false);
  const [busy, setBusy] = useState(false);
  const [diagnose, setDiagnose] = useState<
    | { kind: "running" }
    | { kind: "result"; data: DiagnoseResult }
    | { kind: "err"; message: string }
    | null
  >(null);
  const [status, setStatus] = useState<
    | { kind: "ok"; message: string }
    | { kind: "err"; message: string }
    | null
  >(null);
  const pollRef = useRef<number | null>(null);

  // ── Paste section state ─────────────────────────────────────────────────
  const [plainTextOnly, setPlainTextOnly] = useState<boolean | null>(null);
  const [plainTextSaving, setPlainTextSaving] = useState(false);

  // ── Capture section state ───────────────────────────────────────────────
  // Default OFF (since v0.26.3): OCR persists only the recognised text
  // to history. The PNG is captured for recognition then discarded —
  // keeps the History list focused on the text the user actually wanted.
  const [ocrSaveSource, setOcrSaveSource] = useState<boolean | null>(null);
  const [ocrSaveSourceSaving, setOcrSaveSourceSaving] = useState(false);

  // ── Input-lock section state ───────────────────────────────────────────
  // The chord that releases the input lock. Persisted server-side (the
  // Rust IPC rejects empty / unparseable chords so the user can never
  // save an unusable one and lock themselves out).
  const [inputLockChord, setInputLockChord] = useState<string[] | null>(null);
  // `null` = not capturing; otherwise the set of keys pressed so far
  // during the live chord-capture mode. Committed when the user
  // releases all keys (mirrors the macOS-lock SettingsDialog UX).
  const [capturedChord, setCapturedChord] = useState<Set<string> | null>(null);
  const [inputLockSaving, setInputLockSaving] = useState(false);
  const [inputLockStatus, setInputLockStatus] = useState<
    { kind: "ok" | "err"; message: string } | null
  >(null);

  // ── Direct hotkey → snippet slots ───────────────────────────────────────
  const [snippetList, setSnippetList] = useState<Snippet[]>([]);
  const [slots, setSlots] = useState<DirectSlot[]>([]);
  const [savedSlots, setSavedSlots] = useState<DirectSlot[]>([]);
  const [slotsBusy, setSlotsBusy] = useState(false);
  const [slotsStatus, setSlotsStatus] = useState<
    { kind: "ok" | "err"; message: string } | null
  >(null);
  useEffect(() => {
    listSnippets().then(setSnippetList).catch(() => undefined);
    getDirectSlots()
      .then((s) => {
        setSlots(s);
        setSavedSlots(s);
      })
      .catch(() => undefined);
  }, []);
  const slotKey = (s: { hotkey: string; snippet_id: number }) =>
    `${s.hotkey} ${s.snippet_id}`;
  const slotsDirty =
    slots.map(slotKey).join("|") !== savedSlots.map(slotKey).join("|");
  const updateSlot = (i: number, patch: Partial<DirectSlot>) =>
    setSlots((cur) => cur.map((s, j) => (j === i ? { ...s, ...patch } : s)));
  const addSlot = () =>
    setSlots((cur) => [
      ...cur,
      {
        hotkey: "",
        snippet_id: snippetList[0]?.id ?? 0,
        abbreviation: snippetList[0]?.abbreviation ?? null,
        title: snippetList[0]?.title ?? null,
      },
    ]);
  const removeSlot = (i: number) =>
    setSlots((cur) => cur.filter((_, j) => j !== i));
  const saveSlots = async () => {
    setSlotsBusy(true);
    setSlotsStatus(null);
    try {
      const payload = slots
        .filter((s) => s.hotkey.trim() !== "")
        .map((s) => ({ hotkey: s.hotkey, snippet_id: s.snippet_id }));
      const applied = await setDirectSlots(payload);
      setSlots(applied);
      setSavedSlots(applied);
      setSlotsStatus({
        kind: "ok",
        message:
          applied.length === 0
            ? "No direct slots."
            : `${applied.length} direct slot${applied.length === 1 ? "" : "s"} registered.`,
      });
    } catch (e) {
      setSlotsStatus({ kind: "err", message: String(e) });
    } finally {
      setSlotsBusy(false);
    }
  };

  // ── Autostart (login item / LaunchAgent) ─────────────────────────────────
  const [autostart, setAutostart] = useState<boolean | null>(null);
  const [autostartBusy, setAutostartBusy] = useState(false);
  useEffect(() => {
    getAutostartEnabled()
      .then(setAutostart)
      .catch(() => setAutostart(false));
  }, []);
  // Stay in sync when the tray menu toggles autostart — backend emits
  // `autostart-changed` with the now-effective boolean.
  useEffect(() => {
    let unlisten: UnlistenFn | undefined;
    (async () => {
      unlisten = await listen<boolean>("autostart-changed", (e) => {
        setAutostart(e.payload);
      });
    })();
    return () => unlisten?.();
  }, []);
  const toggleAutostart = async (next: boolean) => {
    setAutostartBusy(true);
    try {
      const applied = await setAutostartEnabled(next);
      setAutostart(applied);
    } catch (e) {
      console.error("autostart toggle failed", e);
    } finally {
      setAutostartBusy(false);
    }
  };

  // ── Window snapping (macOS, v0.84.132) ───────────────────────────────────
  const [snapCfg, setSnapCfg] = useState<WindowSnapConfig | null>(null);
  const [snapBusy, setSnapBusy] = useState(false);
  useEffect(() => {
    if (!IS_MAC) return;
    getWindowSnapConfig()
      .then(setSnapCfg)
      .catch(() => setSnapCfg(null));
  }, []);
  const toggleSnap = async (next: boolean) => {
    if (!snapCfg) return;
    setSnapBusy(true);
    const optimistic = { ...snapCfg, enabled: next };
    setSnapCfg(optimistic);
    try {
      setSnapCfg(await setWindowSnapConfig(optimistic));
    } catch (e) {
      console.error("window-snap toggle failed", e);
      setSnapCfg({ ...snapCfg, enabled: !next });
    } finally {
      setSnapBusy(false);
    }
  };

  // ── Window palette (Moom-style, macOS, v0.84.138) ────────────────────────
  const [paletteCfg, setPaletteCfg] = useState<WindowPaletteConfig | null>(null);
  const [paletteBusy, setPaletteBusy] = useState(false);
  useEffect(() => {
    if (!IS_MAC) return;
    getWindowPaletteConfig()
      .then(setPaletteCfg)
      .catch(() => setPaletteCfg(null));
  }, []);
  const updatePalette = async (patch: Partial<WindowPaletteConfig>) => {
    if (!paletteCfg) return;
    setPaletteBusy(true);
    const next = { ...paletteCfg, ...patch };
    setPaletteCfg(next);
    try {
      setPaletteCfg(await setWindowPaletteConfig(next));
    } catch (e) {
      console.error("window-palette config failed", e);
      setPaletteCfg(paletteCfg);
    } finally {
      setPaletteBusy(false);
    }
  };

  // ── Keep-alive: always running / auto-relaunch (v0.84.126) ───────────────
  const [keepalive, setKeepalive] = useState<boolean | null>(null);
  const [keepaliveBusy, setKeepaliveBusy] = useState(false);
  useEffect(() => {
    getKeepaliveEnabled()
      .then(setKeepalive)
      .catch(() => setKeepalive(false));
  }, []);
  const toggleKeepalive = async (next: boolean) => {
    setKeepaliveBusy(true);
    setKeepalive(next); // optimistic
    try {
      const applied = await setKeepaliveEnabled(next);
      setKeepalive(applied);
    } catch (e) {
      console.error("keepalive toggle failed", e);
      setKeepalive(!next);
    } finally {
      setKeepaliveBusy(false);
    }
  };

  // ── Feedback sounds (v0.84.57) ───────────────────────────────────────────
  const [soundEnabled, setSoundEnabledState] = useState<boolean | null>(null);
  const [soundBusy, setSoundBusy] = useState(false);
  useEffect(() => {
    getSoundEnabled()
      .then(setSoundEnabledState)
      .catch(() => setSoundEnabledState(true));
  }, []);
  const toggleSound = async (next: boolean) => {
    setSoundBusy(true);
    setSoundEnabledState(next); // optimistic — the IPC applies instantly
    try {
      await setSoundEnabled(next);
    } catch (e) {
      console.error("sound toggle failed", e);
      setSoundEnabledState(!next);
    } finally {
      setSoundBusy(false);
    }
  };

  // ── Touchpad gestures (v0.84.113) ────────────────────────────────────────
  const [gestureCfg, setGestureCfg] = useState<GestureConfig | null>(null);
  const [gestureBusy, setGestureBusy] = useState(false);
  useEffect(() => {
    getGestureConfig()
      .then(setGestureCfg)
      .catch(() => setGestureCfg(null));
  }, []);
  const toggleTipTap = async (next: boolean) => {
    if (!gestureCfg) return;
    setGestureBusy(true);
    const optimistic = { ...gestureCfg, tiptap: next };
    setGestureCfg(optimistic);
    try {
      const applied = await setGestureConfig(optimistic);
      setGestureCfg(applied);
    } catch (e) {
      console.error("tip-tap toggle failed", e);
      setGestureCfg({ ...gestureCfg, tiptap: !next });
    } finally {
      setGestureBusy(false);
    }
  };

  const toggleGestures = async (next: boolean) => {
    if (!gestureCfg) return;
    setGestureBusy(true);
    const optimistic = { ...gestureCfg, enabled: next };
    setGestureCfg(optimistic); // optimistic — the IPC re-applies the source
    try {
      const applied = await setGestureConfig(optimistic);
      setGestureCfg(applied);
    } catch (e) {
      console.error("gesture toggle failed", e);
      setGestureCfg({ ...gestureCfg, enabled: !next });
    } finally {
      setGestureBusy(false);
    }
  };

  // ── Timer alarm style (v0.84.76) ─────────────────────────────────────────
  const [alarmStyle, setAlarmStyleState] = useState<AlarmStyle | null>(null);
  useEffect(() => {
    getAlarmStyle()
      .then(setAlarmStyleState)
      .catch(() => setAlarmStyleState("overlay"));
  }, []);
  const chooseAlarmStyle = async (next: AlarmStyle) => {
    const prev = alarmStyle;
    setAlarmStyleState(next); // optimistic
    try {
      await setAlarmStyle(next);
    } catch (e) {
      console.error("alarm style failed", e);
      setAlarmStyleState(prev);
    }
  };

  // ── Clipboard privacy (v0.76.0) ──────────────────────────────────────────
  const [privacy, setPrivacy] = useState<ClipboardPrivacy | null>(null);
  const [privacySaved, setPrivacySaved] = useState(false);
  useEffect(() => {
    getClipboardPrivacy()
      .then(setPrivacy)
      .catch(() => setPrivacy({ exclude_apps: "", auto_clear_seconds: 0 }));
  }, []);
  const savePrivacy = async (next: ClipboardPrivacy) => {
    setPrivacy(next);
    try {
      await setClipboardPrivacy(next);
      setPrivacySaved(true);
      setTimeout(() => setPrivacySaved(false), 1500);
    } catch (e) {
      console.error("save clipboard privacy failed", e);
    }
  };

  // ── Appearance / theme ───────────────────────────────────────────────────
  // `theme` is null until the persisted preference loads; the segmented
  // control disables itself in that window. App.tsx also applies the
  // theme on its own mount — this panel just keeps the picker in sync
  // and re-applies on change.
  const [theme, setTheme] = useState<ThemePreference | null>(null);
  const [themeBusy, setThemeBusy] = useState(false);
  useEffect(() => {
    getThemePreference()
      .then((t) => setTheme(normaliseTheme(t)))
      .catch(() => setTheme("system"));
  }, []);
  const changeTheme = async (next: ThemePreference) => {
    // Apply instantly for a snappy feel, then persist. If the persist
    // fails we keep the applied theme — it's purely cosmetic and the
    // next launch falls back to the last successfully-stored value.
    applyTheme(next);
    setTheme(next);
    setThemeBusy(true);
    try {
      await setThemePreference(next);
    } catch (e) {
      console.error("theme persist failed", e);
    } finally {
      setThemeBusy(false);
    }
  };

  // ── Popup overlay size ───────────────────────────────────────────────────
  // `winSize` is null until the persisted preset loads; the segmented control
  // disables itself in that window. Changing it resizes the live window on the
  // backend (next show recentres it).
  const [winSize, setWinSize] = useState<string | null>(null);
  const [winSizeBusy, setWinSizeBusy] = useState(false);
  useEffect(() => {
    getWindowSizePreference()
      .then((s) => setWinSize(s))
      .catch(() => setWinSize("medium"));
  }, []);
  const changeWinSize = async (next: string) => {
    setWinSize(next);
    setWinSizeBusy(true);
    try {
      await setWindowSizePreference(next);
    } catch (e) {
      console.error("window size persist failed", e);
    } finally {
      setWinSizeBusy(false);
    }
  };

  // ── About content (inline, bottom of Settings) ──────────────────────────
  // Version is pulled once on mount; the catch keeps the render path
  // independent of the Tauri context (matters for component tests).
  const [appVersion, setAppVersion] = useState<string | undefined>(undefined);
  useEffect(() => {
    getVersion().then(setAppVersion).catch(() => undefined);
  }, []);

  useEffect(() => {
    let alive = true;
    getPastePlainTextOnly()
      .then((v) => {
        if (alive) setPlainTextOnly(v);
      })
      .catch(() => {
        if (alive) setPlainTextOnly(true);
      });
    return () => {
      alive = false;
    };
  }, []);

  const togglePlainText = async (next: boolean) => {
    setPlainTextOnly(next); // optimistic
    setPlainTextSaving(true);
    try {
      await setPastePlainTextOnly(next);
    } catch (e) {
      // Revert on failure.
      setPlainTextOnly(!next);
      setStatus({ kind: "err", message: String(e) });
    } finally {
      setPlainTextSaving(false);
    }
  };

  useEffect(() => {
    let alive = true;
    getOcrSaveSourceImage()
      .then((v) => {
        if (alive) setOcrSaveSource(v);
      })
      .catch(() => {
        if (alive) setOcrSaveSource(false);
      });
    return () => {
      alive = false;
    };
  }, []);

  const toggleOcrSaveSource = async (next: boolean) => {
    setOcrSaveSource(next); // optimistic
    setOcrSaveSourceSaving(true);
    try {
      await setOcrSaveSourceImage(next);
    } catch (e) {
      setOcrSaveSource(!next);
      setStatus({ kind: "err", message: String(e) });
    } finally {
      setOcrSaveSourceSaving(false);
    }
  };

  // ── Input lock: load + chord-capture handler ───────────────────────────
  useEffect(() => {
    let alive = true;
    getInputLockChord()
      .then((c) => {
        if (alive) setInputLockChord(c);
      })
      .catch(() => {
        if (alive) setInputLockChord(["i", "r"]);
      });
    return () => {
      alive = false;
    };
  }, []);

  // Listen for keydowns / keyups while in capture mode. The chord is
  // committed on the *first keyup* (after at least one key was
  // pressed) — mirrors macOS-lock's SettingsDialog: press the keys you
  // want, release one to confirm. Cancel via Escape.
  useEffect(() => {
    if (capturedChord === null) return;
    const onKeyDown = (e: KeyboardEvent) => {
      e.preventDefault();
      e.stopPropagation();
      if (e.key === "Escape") {
        setCapturedChord(null);
        return;
      }
      const name = chordKeyName(e.key);
      if (!name) return;
      setCapturedChord((cur) => {
        if (cur === null) return cur;
        const next = new Set(cur);
        next.add(name);
        return next;
      });
    };
    const onKeyUp = (e: KeyboardEvent) => {
      e.preventDefault();
      e.stopPropagation();
      if (e.key === "Escape") return;
      // Commit only if the user actually pressed something. Snapshot
      // first so a half-released chord still saves the full set.
      setCapturedChord((cur) => {
        if (cur === null) return cur;
        if (cur.size === 0) return cur;
        void commitChord(Array.from(cur));
        return null;
      });
    };
    window.addEventListener("keydown", onKeyDown, true);
    window.addEventListener("keyup", onKeyUp, true);
    return () => {
      window.removeEventListener("keydown", onKeyDown, true);
      window.removeEventListener("keyup", onKeyUp, true);
    };
  }, [capturedChord]);

  const commitChord = async (keys: string[]) => {
    setInputLockSaving(true);
    setInputLockStatus(null);
    try {
      await ipcSetInputLockChord(keys);
      setInputLockChord(keys);
      setInputLockStatus({
        kind: "ok",
        message: `Saved chord: ${keys.join(" + ")}`,
      });
    } catch (e) {
      setInputLockStatus({ kind: "err", message: String(e) });
    } finally {
      setInputLockSaving(false);
    }
  };

  // ── Backup section state ────────────────────────────────────────────────
  const [includeHistory, setIncludeHistory] = useState(true);
  const [includeSnippets, setIncludeSnippets] = useState(true);
  const [includeNotes, setIncludeNotes] = useState(true);
  const [backupBusy, setBackupBusy] = useState<"export" | "import" | null>(null);
  const [backupStatus, setBackupStatus] = useState<
    | { kind: "ok"; message: string }
    | { kind: "import-ok"; result: BackupImportResult }
    | { kind: "err"; message: string }
    | null
  >(null);

  const onExport = async () => {
    if (!includeHistory && !includeSnippets && !includeNotes) {
      setBackupStatus({
        kind: "err",
        message: "Select at least one section to export.",
      });
      return;
    }
    setBackupStatus(null);
    setBackupBusy("export");
    await setSuppressHide(true).catch(() => {});
    try {
      const stamp = new Date().toISOString().replace(/[:.]/g, "-").slice(0, 19);
      const path = await saveDialog({
        title: "Save Inspector Rust backup",
        defaultPath: `inspector-rust-backup-${stamp}.json`,
        filters: [{ name: "JSON", extensions: ["json"] }],
      });
      if (!path) return;
      const bytes = await saveBackupToFile(path, {
        includeHistory,
        includeSnippets,
        includeNotes,
      });
      const filename = path.split("/").pop() ?? path;
      setBackupStatus({
        kind: "ok",
        message: `Exported ${formatBytes(bytes)} to ${filename}`,
      });
    } catch (e) {
      setBackupStatus({ kind: "err", message: String(e) });
    } finally {
      await setSuppressHide(false).catch(() => {});
      setBackupBusy(null);
    }
  };

  const onImport = async () => {
    setBackupStatus(null);
    setBackupBusy("import");
    await setSuppressHide(true).catch(() => {});
    try {
      const path = await openDialog({
        multiple: false,
        directory: false,
        filters: [{ name: "JSON", extensions: ["json"] }],
        title: "Select Inspector Rust backup file",
      });
      if (!path) return;
      const result = await importBackup(path);
      setBackupStatus({ kind: "import-ok", result });
      if (onBackupImported) await onBackupImported();
    } catch (e) {
      setBackupStatus({ kind: "err", message: String(e) });
    } finally {
      await setSuppressHide(false).catch(() => {});
      setBackupBusy(null);
    }
  };

  // Initial load.
  useEffect(() => {
    let alive = true;
    getExpanderConfig()
      .then((c) => {
        if (!alive) return;
        setCfg(c);
        setHotkey(c.hotkey);
        setEnabled(c.enabled);
        setAccessibility(c.accessibility_granted);
      })
      .catch((e) => setStatus({ kind: "err", message: String(e) }));
    getAutoExpandConfig()
      .then((c) => {
        if (alive) setAeCfg(c);
      })
      .catch((e) => setStatus({ kind: "err", message: String(e) }));
    Promise.all([getCleanerConfig(), cleanerCategories()])
      .then(([c, cats]) => {
        if (!alive) return;
        setCleanCfg(c);
        setCleanCats(cats);
      })
      .catch((e) => setStatus({ kind: "err", message: String(e) }));
    return () => {
      alive = false;
    };
  }, []);

  // Persist + apply a cleaner config change (optimistic + reconcile).
  const updateCleaner = async (patch: Partial<CleanerConfig>) => {
    if (!cleanCfg) return;
    const next = { ...cleanCfg, ...patch };
    setCleanCfg(next);
    try {
      const applied = await setCleanerConfig(next);
      setCleanCfg(applied);
    } catch (e) {
      setStatus({ kind: "err", message: String(e) });
    }
  };

  // Persist + apply an auto-expansion config change. Optimistically updates
  // local state, then reconciles with whatever the backend reports back.
  const updateAutoExpand = async (patch: Partial<AutoExpandConfig>) => {
    if (!aeCfg) return;
    const next = { ...aeCfg, ...patch };
    setAeCfg(next);
    setAeSaving(true);
    try {
      const applied = await setAutoExpandConfig(next);
      setAeCfg(applied);
    } catch (e) {
      setStatus({ kind: "err", message: String(e) });
    } finally {
      setAeSaving(false);
    }
  };

  // While Accessibility is *not* granted, poll once a second so the badge
  // flips to green within a moment of the user toggling it on in System
  // Settings — instead of needing a panel reload. Crucially: when the
  // status flips false→true, surface a one-time restart prompt because
  // the running process can't actually *use* the new grant until it's
  // re-launched (macOS caches AXIsProcessTrusted per-process).
  useEffect(() => {
    if (accessibility === null || accessibility === true) return;
    const id = window.setInterval(async () => {
      try {
        const ok = await getAccessibilityStatus();
        if (ok) {
          setAccessibility(true);
          setJustGranted(true);
        }
      } catch {
        /* ignore — keep polling */
      }
    }, 1000);
    pollRef.current = id;
    return () => {
      if (pollRef.current !== null) window.clearInterval(pollRef.current);
      pollRef.current = null;
    };
  }, [accessibility]);

  // Initial load + same poll-while-not-granted pattern for Screen
  // Recording. Independent state so a missing AX grant doesn't hide
  // a missing Screen Recording grant or vice versa.
  useEffect(() => {
    let alive = true;
    getScreenRecordingStatus()
      .then((ok) => {
        if (alive) setScreenRec(ok);
      })
      .catch(() => {
        /* leave null — UI shows neutral state */
      });
    return () => {
      alive = false;
    };
  }, []);
  useEffect(() => {
    if (screenRec === null || screenRec === true) return;
    const id = window.setInterval(async () => {
      try {
        const ok = await getScreenRecordingStatus();
        if (ok) setScreenRec(true);
      } catch {
        /* ignore — keep polling */
      }
    }, 1000);
    return () => window.clearInterval(id);
  }, [screenRec]);

  // Initial load + poll for Automation→Finder. Same shape as the two
  // grants above. On the very first call after install the probe fires
  // the macOS Automation prompt — that's by design and the only way
  // to actually trigger it; macOS has no "request without sending"
  // path for AppleEvents.
  useEffect(() => {
    let alive = true;
    getFinderAutomationStatus()
      .then((ok) => {
        if (alive) setFinderAutomation(ok);
      })
      .catch(() => {
        /* leave null */
      });
    return () => {
      alive = false;
    };
  }, []);
  useEffect(() => {
    if (finderAutomation === null || finderAutomation === true) return;
    const id = window.setInterval(async () => {
      try {
        const ok = await getFinderAutomationStatus();
        if (ok) setFinderAutomation(true);
      } catch {
        /* ignore — keep polling */
      }
    }, 1000);
    return () => window.clearInterval(id);
  }, [finderAutomation]);

  // ── "Set up permissions" — one click chains both grants. ─────────────
  // ALWAYS resets the TCC entry first (via tccutil) and re-fires the
  // macOS prompt. This is what makes the button useful for the stuck
  // case: a user who toggled the System-Settings switch on but is
  // *still* being asked for permission has a stale TCC entry — the
  // stored code-requirement is from a previous binary (e.g. the
  // pre-v0.23.2 ad-hoc signature) and doesn't match the current
  // cert-signed binary, so `AXIsProcessTrusted` returns false even
  // though the switch looks on. `tccutil reset` wipes the stale entry;
  // the re-fired macOS prompt produces a clean grant against the
  // *current* signature. For a fresh install the reset is a no-op, so
  // the same button handles both cases. macOS still requires the
  // toggle itself — no app, no password, can replace that — but the
  // reset removes the friction of figuring out why a "granted"
  // permission isn't actually granted.
  const setUpPermissions = async () => {
    chainOpenedScreenRec.current = false;
    chainOpenedFinder.current = false;
    setChaining(true);
    try {
      if (accessibility !== true) {
        await forceResetAndRequestGrant();
      } else if (screenRec !== true) {
        chainOpenedScreenRec.current = true;
        await forceResetScreenRecordingGrant();
      } else if (finderAutomation !== true) {
        chainOpenedFinder.current = true;
        await forceResetFinderAutomationGrant();
      }
    } catch (e) {
      setStatus({ kind: "err", message: String(e) });
    }
  };

  // While chaining: walk through the three grants in order. Each time
  // one flips to granted, auto-reset + re-prompt the next still-missing
  // one (once per grant). Clear the chaining flag when all three are in
  // place. Order matches the row order: Accessibility → Screen
  // Recording → Automation→Finder.
  useEffect(() => {
    if (!chaining) return;
    const accOk = accessibility === true;
    const scrOk = screenRec === true;
    const finOk = finderAutomation === true;
    if (accOk && !scrOk && !chainOpenedScreenRec.current) {
      chainOpenedScreenRec.current = true;
      try {
        forceResetScreenRecordingGrant();
      } catch (e) {
        setStatus({ kind: "err", message: String(e) });
      }
    }
    if (accOk && scrOk && !finOk && !chainOpenedFinder.current) {
      chainOpenedFinder.current = true;
      try {
        forceResetFinderAutomationGrant();
      } catch (e) {
        setStatus({ kind: "err", message: String(e) });
      }
    }
    if (accOk && scrOk && finOk) setChaining(false);
  }, [chaining, accessibility, screenRec, finderAutomation]);

  const dirty =
    cfg !== null && (cfg.enabled !== enabled || cfg.hotkey !== hotkey);

  const save = async () => {
    setBusy(true);
    setStatus(null);
    try {
      const applied = await setExpanderConfig(enabled, hotkey || DEFAULT_HOTKEY);
      setCfg(applied);
      setHotkey(applied.hotkey);
      setEnabled(applied.enabled);
      setStatus({
        kind: "ok",
        message: applied.enabled
          ? `Expander armed: ${prettyHotkey(applied.hotkey)}`
          : "Expander disabled.",
      });
    } catch (e) {
      setStatus({ kind: "err", message: String(e) });
    } finally {
      setBusy(false);
    }
  };

  const reset = () => {
    setHotkey(DEFAULT_HOTKEY);
    setStatus(null);
  };

  return (
    <div className="relative flex min-h-0 flex-1 flex-col overflow-auto p-6">
      {/* macOS permissions card — one consolidated card (it replaced two
          separate per-permission banners). "Set up permissions" chains
          the user through both grants with a single click: it opens the
          first missing System Settings pane, and the chaining effect
          auto-opens the second once the first flips to granted. macOS
          does NOT allow an app to grant Accessibility / Screen Recording
          — the toggle is always the user's, by design — so the card
          guides the flow rather than automating it. Renders only while a
          permission is missing; the granted state is silent. */}
      {(accessibility === false || screenRec === false || finderAutomation === false) && (
        <div className="-mt-2 mb-4 w-full">
          <div className="rounded border border-amber-500/60 bg-[var(--color-bg)] text-[12px] text-[var(--color-fg)] shadow-md ring-1 ring-amber-500/30">
            {/* Header + the one-click chained setup action. */}
            <div className="flex items-center gap-2 border-b border-amber-500/30 px-3 py-2">
              <AlertTriangle size={14} className="shrink-0 text-amber-500" />
              <span className="flex-1 font-medium">macOS permissions needed</span>
              <button
                onClick={() => void setUpPermissions()}
                className="rounded bg-[var(--color-accent)] px-3 py-1 text-[11px] font-medium text-[var(--color-accent-fg)] hover:opacity-90"
              >
                {chaining ? "Setting up…" : "Set up permissions"}
              </button>
            </div>

            {/* Explainer — honest about what the button can and can't do. */}
            <p className="px-3 pt-2 text-[var(--color-muted)]">
              <b className="text-[var(--color-fg)]">Set up permissions</b> wipes
              any stale macOS TCC entry for Inspector Rust (via{" "}
              <code>tccutil reset</code>, no admin password) and re-fires
              the macOS permission prompt. Click <b>Allow → Open System
              Settings</b>, flip the <b>Inspector Rust</b> switch — once both
              grants are in, this card auto-prompts to restart. macOS only
              lets <i>you</i> flip the switch; the reset removes the friction
              when a switch <i>looks</i> on but Inspector Rust still asks.
            </p>

            {/* Live per-permission status. */}
            <div className="flex flex-col gap-1.5 px-3 py-2">
              <PermRow
                label="Accessibility"
                hint="Lets Inspector Rust paste and run the text expander"
                granted={accessibility}
                onOpen={() =>
                  void openAccessibilitySettings().catch((e) =>
                    setStatus({ kind: "err", message: String(e) }),
                  )
                }
              />
              <PermRow
                label="Screen Recording"
                hint="Lets the OCR and screenshot region capture work"
                granted={screenRec}
                onOpen={() =>
                  void openScreenRecordingSettings().catch((e) =>
                    setStatus({ kind: "err", message: String(e) }),
                  )
                }
              />
              <PermRow
                label="Automation → Finder"
                hint="Lets Ctrl+Shift+F read the current Finder selection"
                granted={finderAutomation}
                onOpen={() =>
                  void openFinderAutomationSettings().catch((e) =>
                    setStatus({ kind: "err", message: String(e) }),
                  )
                }
              />
            </div>

            {/* Troubleshooting — collapsed by default. */}
            <details className="border-t border-amber-500/30 px-3 py-2 text-[11px] text-[var(--color-muted)]">
              <summary className="cursor-pointer">
                Switch is already on, but it still doesn&apos;t work?
              </summary>
              <p className="mt-1.5">
                macOS keys each grant to the app&apos;s code signature. As of
                v0.23.2 <code>scripts/install-macos.sh</code> signs every build
                with a stable self-signed certificate, so a grant survives
                rebuilds — you should only need to do this once. If a switch
                shows on but Inspector Rust still asks, the grant is stale:
                reset it, then re-toggle and relaunch.
              </p>
              <div className="mt-2 flex flex-wrap gap-2">
                <button
                  onClick={async () => {
                    if (
                      !window.confirm(
                        "Reset the stale Accessibility + Screen Recording + Automation→Finder grants for Inspector Rust and re-fire the macOS prompts? Use this when a switch shows on but Inspector Rust still asks for permission.",
                      )
                    )
                      return;
                    try {
                      await forceResetAndRequestGrant();
                      await forceResetScreenRecordingGrant();
                      await forceResetFinderAutomationGrant();
                    } catch (e) {
                      setStatus({ kind: "err", message: String(e) });
                    }
                  }}
                  className="rounded border border-[var(--color-border)] px-2.5 py-1 hover:border-[var(--color-accent)] hover:text-[var(--color-accent)]"
                >
                  Reset stale grants
                </button>
                <button
                  onClick={async () => {
                    try {
                      setAccessibility(await getAccessibilityStatus());
                      setScreenRec(await getScreenRecordingStatus());
                      setFinderAutomation(await getFinderAutomationStatus());
                    } catch (e) {
                      setStatus({ kind: "err", message: String(e) });
                    }
                  }}
                  className="rounded border border-[var(--color-border)] px-2.5 py-1 hover:border-[var(--color-accent)] hover:text-[var(--color-accent)]"
                >
                  Re-check now
                </button>
                <button
                  onClick={async () => {
                    if (
                      !window.confirm(
                        "Quit Inspector Rust now? Re-launch it via Spotlight / Dock to pick up a freshly-granted permission.",
                      )
                    )
                      return;
                    try {
                      await quitApp();
                    } catch (e) {
                      setStatus({ kind: "err", message: String(e) });
                    }
                  }}
                  className="rounded border border-amber-500/60 bg-amber-500/10 px-2.5 py-1 text-amber-600 hover:bg-amber-500/20 dark:text-amber-400"
                >
                  Quit Inspector Rust
                </button>
              </div>
            </details>
          </div>
        </div>
      )}

      <div className="w-full">
        {/* Sound — master toggle for UI feedback cues. At the top so it's
            easy to silence the app. */}
        <div className="mb-6">
          <Section
            icon={<Volume2 size={16} className="text-[var(--color-accent)]" />}
            title="Sounds"
            subtitle="Play short feedback cues for actions: snippet expansion, OCR recognised, screenshot captured, recording start/stop, and copy to clipboard."
          >
            <Row label="Feedback sounds">
              <label className="flex cursor-pointer items-center gap-2 text-[12px]">
                <input
                  type="checkbox"
                  checked={soundEnabled ?? true}
                  disabled={soundEnabled === null || soundBusy}
                  onChange={(e) => void toggleSound(e.target.checked)}
                  className="accent-[var(--color-accent)]"
                />
                <span className="text-[var(--color-muted)]">
                  {soundEnabled === null
                    ? "Loading…"
                    : soundEnabled
                      ? "On — actions play a short sound"
                      : "Off — the app stays silent"}
                </span>
              </label>
            </Row>
          </Section>
        </div>

        {/* Touchpad gestures — BetterTouchTool-style 3-finger swipe/tap. */}
        <div className="mb-6">
          <Section
            icon={<Hand size={16} className="text-[var(--color-accent)]" />}
            title="Touchpad gestures"
            subtitle="3-finger swipe up / down → volume up / down, 3-finger tap → mute, tip-tap → switch tabs. Off by default. macOS uses the private MultitouchSupport framework; if gestures don't fire, grant Input Monitoring (System Settings → Privacy & Security)."
          >
            <Row label="Enable gestures">
              <label className="flex cursor-pointer items-center gap-2 text-[12px]">
                <input
                  type="checkbox"
                  checked={gestureCfg?.enabled ?? false}
                  disabled={gestureCfg === null || gestureBusy}
                  onChange={(e) => void toggleGestures(e.target.checked)}
                  className="accent-[var(--color-accent)]"
                />
                <span className="text-[var(--color-muted)]">
                  {gestureCfg === null
                    ? "Loading…"
                    : gestureCfg.enabled
                      ? "On — 3-finger swipe controls volume, tap toggles mute"
                      : "Off"}
                </span>
              </label>
            </Row>
            {gestureCfg?.enabled && (
              <Row label="Tip-tap tab switch">
                <label className="flex cursor-pointer items-center gap-2 text-[12px]">
                  <input
                    type="checkbox"
                    checked={gestureCfg?.tiptap ?? true}
                    disabled={gestureCfg === null || gestureBusy}
                    onChange={(e) => void toggleTipTap(e.target.checked)}
                    className="accent-[var(--color-accent)]"
                  />
                  <span className="text-[var(--color-muted)]">
                    Rest one finger, tap a second to its right → next tab (Ctrl+Tab); tap to
                    its left → previous tab (Ctrl+Shift+Tab)
                  </span>
                </label>
              </Row>
            )}
          </Section>
        </div>

        {/* Window snapping — drag a window to a screen edge to snap it (macOS). */}
        {IS_MAC && (
          <div className="mb-6">
            <Section
              icon={<LayoutGrid size={16} className="text-[var(--color-accent)]" />}
              title="Window snapping"
              subtitle="Drag a window to a screen edge to snap it: top → maximize, left/right → half. A preview shows the target zone; release to snap. Off by default. Needs Accessibility (System Settings → Privacy & Security → Accessibility)."
            >
              <Row label="Enable snapping">
                <label className="flex cursor-pointer items-center gap-2 text-[12px]">
                  <input
                    type="checkbox"
                    checked={snapCfg?.enabled ?? false}
                    disabled={snapCfg === null || snapBusy}
                    onChange={(e) => void toggleSnap(e.target.checked)}
                    className="accent-[var(--color-accent)]"
                  />
                  <span className="text-[var(--color-muted)]">
                    {snapCfg === null
                      ? "Loading…"
                      : snapCfg.enabled
                        ? "On — drag a window to an edge to snap it"
                        : "Off"}
                  </span>
                </label>
              </Row>
            </Section>
          </div>
        )}

        {/* Window palette — Moom-style hover palette over the green zoom button. */}
        {IS_MAC && (
          <div className="mb-6">
            <Section
              icon={<LayoutGrid size={16} className="text-[var(--color-accent)]" />}
              title="Window palette"
              subtitle="Hover the green zoom button of a window → a palette appears with preset layouts and a hex grid you drag a region over to snap the window there. Off by default. Needs Accessibility (System Settings → Privacy & Security → Accessibility)."
            >
              <Row label="Enable palette">
                <label className="flex cursor-pointer items-center gap-2 text-[12px]">
                  <input
                    type="checkbox"
                    checked={paletteCfg?.enabled ?? false}
                    disabled={paletteCfg === null || paletteBusy}
                    onChange={(e) => void updatePalette({ enabled: e.target.checked })}
                    className="accent-[var(--color-accent)]"
                  />
                  <span className="text-[var(--color-muted)]">
                    {paletteCfg === null
                      ? "Loading…"
                      : paletteCfg.enabled
                        ? "On — hover a window's green button"
                        : "Off"}
                  </span>
                </label>
              </Row>
              {paletteCfg?.enabled && (
                <Row label="Hex grid density">
                  <div className="flex items-center gap-1.5 text-[12px]">
                    <input
                      type="number"
                      min={2}
                      max={16}
                      value={paletteCfg.cols}
                      disabled={paletteBusy}
                      onChange={(e) => void updatePalette({ cols: Number(e.target.value) })}
                      className="w-12 rounded-md border border-[var(--color-border)] bg-[var(--color-bg)] px-1.5 py-0.5 text-[var(--color-fg)]"
                      aria-label="Hex grid columns"
                    />
                    <span className="text-[var(--color-muted)]">×</span>
                    <input
                      type="number"
                      min={2}
                      max={16}
                      value={paletteCfg.rows}
                      disabled={paletteBusy}
                      onChange={(e) => void updatePalette({ rows: Number(e.target.value) })}
                      className="w-12 rounded-md border border-[var(--color-border)] bg-[var(--color-bg)] px-1.5 py-0.5 text-[var(--color-fg)]"
                      aria-label="Hex grid rows"
                    />
                    <span className="text-[var(--color-muted)]">cells</span>
                  </div>
                </Row>
              )}
            </Section>
          </div>
        )}

        {/* Timer alarm — overlay (loud, dismiss-to-stop) vs OS notification. */}
        <div className="mb-6">
          <Section
            icon={<AlarmClock size={16} className="text-[var(--color-accent)]" />}
            title="Timer alarm"
            subtitle="How a finished timer / countdown / alarm alerts you."
          >
            <Row label="Style">
              <div className="flex flex-col gap-1.5 text-[12px]">
                <label className="flex cursor-pointer items-start gap-2">
                  <input
                    type="radio"
                    name="alarm-style"
                    checked={alarmStyle === "overlay"}
                    disabled={alarmStyle === null}
                    onChange={() => void chooseAlarmStyle("overlay")}
                    className="mt-0.5 accent-[var(--color-accent)]"
                  />
                  <span className="text-[var(--color-muted)]">
                    <span className="text-[var(--color-fg)]">Alarm overlay</span> (default)
                    — a loud sound (system volume is raised while it rings) + a
                    fullscreen overlay you click to stop.
                  </span>
                </label>
                <label className="flex cursor-pointer items-start gap-2">
                  <input
                    type="radio"
                    name="alarm-style"
                    checked={alarmStyle === "notification"}
                    disabled={alarmStyle === null}
                    onChange={() => void chooseAlarmStyle("notification")}
                    className="mt-0.5 accent-[var(--color-accent)]"
                  />
                  <span className="text-[var(--color-muted)]">
                    <span className="text-[var(--color-fg)]">OS notification</span> — the
                    classic quiet system notification + a short sound.
                  </span>
                </label>
              </div>
            </Row>
          </Section>
        </div>

        {/* Popup hotkey — the global shortcut that opens the search popup. */}
        <PopupHotkeySection />

        {/* Second, optional clipboard-history hotkey (default Ctrl+Shift+V). */}
        <div className="mt-6">
          <HistoryHotkeySection />
        </div>

        {/* Global action shortcuts — rebindable OCR / screenshot / timesheet / … */}
        <div className="mt-6">
          <Section
            icon={<Keyboard size={16} className="text-[var(--color-accent)]" />}
            title="Global shortcuts"
            subtitle="Rebind the global action hotkeys. Press a combo to set it (Backspace clears, Esc cancels); a conflict with any other binding is rejected. Note for Windows: Ctrl+Shift+T is the browser's 'reopen closed tab' — rebind Timesheet here to avoid the clash."
          >
            <GlobalShortcutsSection />
          </Section>
        </div>

        {/* Text expander section */}
        <div className="mt-6">
        <Section
          icon={<Wand2 size={16} className="text-[var(--color-accent)]" />}
          title="Text expander"
          subtitle="Type a snippet abbreviation in any text field, then press your hotkey to replace it with the snippet body. Like aText / TextExpander."
        >
          {/* Granted state — shown inline since it's not actionable.
              Special case: if we *just* detected the false→true edge
              while polling, prompt for a restart, because the running
              process can't see the new grant until macOS re-evaluates
              AXIsProcessTrusted on a fresh launch. */}
          {accessibility === true && justGranted && (
            <div className="mb-4 flex flex-col gap-2 rounded border border-emerald-500/40 bg-emerald-500/5 px-3 py-2.5 text-[12px]">
              <div className="flex items-start gap-2">
                <CheckCircle2 size={14} className="mt-0.5 shrink-0 text-emerald-500" />
                <div className="flex-1">
                  <div className="font-medium">Access detected — one more step</div>
                  <div className="mt-0.5 text-[var(--color-muted)]">
                    macOS caches the trust check per-process, so the
                    running Inspector Rust can't actually use the just-granted
                    permission until it relaunches. The new instance will
                    take ~1 second to start.
                  </div>
                </div>
              </div>
              <div className="flex flex-wrap gap-2 pl-6">
                <button
                  onClick={async () => {
                    setRestarting(true);
                    try {
                      await relaunchApp();
                      // We won't reach this point — process is exiting.
                    } catch (e) {
                      setRestarting(false);
                      setStatus({ kind: "err", message: String(e) });
                    }
                  }}
                  disabled={restarting}
                  className="rounded bg-emerald-500 px-2.5 py-1 text-[11px] font-medium text-white hover:opacity-90 disabled:opacity-50"
                >
                  {restarting ? "Restarting…" : "Restart now"}
                </button>
                <button
                  onClick={() => setJustGranted(false)}
                  className="rounded border border-[var(--color-border)] px-2.5 py-1 text-[11px] text-[var(--color-muted)] hover:border-[var(--color-accent)] hover:text-[var(--color-accent)]"
                  title="Dismiss this prompt — the expander will work next time you launch Inspector Rust"
                >
                  Later
                </button>
              </div>
            </div>
          )}
          {accessibility === true && !justGranted && (
            <div className="mb-4 flex items-center gap-2 rounded border border-emerald-500/30 bg-emerald-500/5 px-3 py-2 text-[12px]">
              <CheckCircle2 size={14} className="shrink-0 text-emerald-500" />
              <span className="font-medium">Accessibility access granted</span>
            </div>
          )}

          <Row label="Enable">
            <label className="flex cursor-pointer items-center gap-2 text-[12px]">
              <input
                type="checkbox"
                checked={enabled}
                onChange={(e) => setEnabled(e.target.checked)}
                className="accent-[var(--color-accent)]"
              />
              <span className="text-[var(--color-muted)]">
                {enabled ? "Hotkey is registered globally" : "Hotkey is unregistered"}
              </span>
            </label>
          </Row>

          <Row
            label="Hotkey"
            help="Press a key combination, or pick a preset. Backspace clears, Esc cancels. Names match the W3C KeyboardEvent.code spec (Digit1, KeyE, Backquote, …). Tip: digit keys (Alt+1 …) are the most reliable — they're in the same place on every keyboard layout."
          >
            <div className="flex flex-wrap items-center gap-2">
              <HotkeyCapture
                value={hotkey}
                onChange={setHotkey}
                disabled={busy}
              />
              <button
                onClick={reset}
                disabled={busy || hotkey === DEFAULT_HOTKEY}
                className="rounded border border-[var(--color-border)] px-2 py-1 text-[11px] text-[var(--color-muted)] hover:border-[var(--color-accent)] hover:text-[var(--color-fg)] disabled:opacity-40"
                title={`Reset to ${prettyHotkey(DEFAULT_HOTKEY)}`}
              >
                Reset
              </button>
              <span className="text-[11px] text-[var(--color-muted)]">presets:</span>
              {QUICK_HOTKEYS.map((q) => {
                const active = hotkey === q.value;
                return (
                  <button
                    key={q.value}
                    onClick={() => setHotkey(q.value)}
                    disabled={busy}
                    className={
                      "rounded border px-2 py-1 text-[11px] disabled:opacity-40 " +
                      (active
                        ? "border-[var(--color-accent)] text-[var(--color-accent)]"
                        : "border-[var(--color-border)] hover:border-[var(--color-accent)] hover:text-[var(--color-accent)]")
                    }
                    title={`Use ${q.label}`}
                  >
                    {q.label}
                  </button>
                );
              })}
            </div>
          </Row>

          <div className="mt-4 flex items-center gap-3">
            <button
              onClick={() => void save()}
              disabled={!dirty || busy || !hotkey}
              className="rounded bg-[var(--color-accent)] px-3 py-1 text-[12px] text-[var(--color-accent-fg)] hover:opacity-90 disabled:opacity-50"
            >
              {busy ? "Saving…" : dirty ? "Save & re-register" : "No changes"}
            </button>
            {status && (
              <span
                className={
                  "text-[11px] " +
                  (status.kind === "ok"
                    ? "text-[var(--color-muted)]"
                    : "text-red-400")
                }
              >
                {status.message}
              </span>
            )}
          </div>

          <div className="mt-3 flex flex-col gap-2 rounded border border-dashed border-[var(--color-border)] p-3">
            <div className="flex items-start gap-3">
              <PlayCircle size={14} className="mt-0.5 shrink-0 text-[var(--color-accent)]" />
              <div className="flex-1 text-[11px] text-[var(--color-muted)]">
                <div className="text-[var(--color-fg)]">Diagnose expansion (no paste)</div>
                <p className="mt-0.5">
                  Type a snippet abbreviation in any text field, place the
                  cursor right after it, then click <b>Diagnose</b>. Inspector Rust
                  will hide its popup, capture the word before your cursor,
                  look it up — and report what it found, without pasting.
                  This isolates the lookup from the paste step so you can
                  see exactly where expansion is breaking.
                </p>
              </div>
              <button
                onClick={async () => {
                  setDiagnose({ kind: "running" });
                  try {
                    const data = await diagnoseExpandAtCursor();
                    setDiagnose({ kind: "result", data });
                  } catch (e) {
                    setDiagnose({ kind: "err", message: String(e) });
                  }
                }}
                disabled={diagnose?.kind === "running"}
                className="rounded border border-[var(--color-border)] px-3 py-1 text-[11px] hover:border-[var(--color-accent)] hover:text-[var(--color-accent)] disabled:opacity-50"
              >
                {diagnose?.kind === "running" ? "Capturing…" : "Diagnose"}
              </button>
            </div>

            {diagnose?.kind === "result" && (
              <div className="rounded border border-[var(--color-border)] bg-[var(--color-surface)] p-2 text-[11px]">
                <div className="grid grid-cols-[110px_1fr] gap-x-3 gap-y-1">
                  <span className="text-[var(--color-muted)]">Capture path</span>
                  <span>
                    {diagnose.data.path === "ax" ? (
                      <span className="rounded bg-emerald-500/10 px-1.5 py-0.5 font-medium text-emerald-400">
                        macOS AX (clean — no clipboard touch)
                      </span>
                    ) : diagnose.data.path === "uia" ? (
                      <span className="rounded bg-emerald-500/10 px-1.5 py-0.5 font-medium text-emerald-400">
                        Windows UIA (clean — no clipboard touch)
                      </span>
                    ) : (
                      <span className="rounded bg-amber-500/10 px-1.5 py-0.5 font-medium text-amber-400">
                        Clipboard fallback — focused app didn&apos;t expose accessibility info
                      </span>
                    )}
                  </span>
                  <span className="text-[var(--color-muted)]">Captured</span>
                  <code className="rounded bg-[var(--color-bg)] px-1 font-[var(--font-mono)]">
                    {diagnose.data.captured || "(empty)"}
                  </code>
                  <span className="text-[var(--color-muted)]">Snippet match</span>
                  <span>
                    {diagnose.data.matched_abbreviation ? (
                      <code className="rounded bg-[var(--color-bg)] px-1 font-[var(--font-mono)] text-emerald-400">
                        {diagnose.data.matched_abbreviation}
                      </code>
                    ) : (
                      <span className="text-amber-400">
                        no match — add a snippet with this abbreviation
                      </span>
                    )}
                  </span>
                  {diagnose.data.paste_preview && (
                    <>
                      <span className="text-[var(--color-muted)]">Would paste</span>
                      <code className="block truncate rounded bg-[var(--color-bg)] px-1 font-[var(--font-mono)]">
                        {diagnose.data.paste_preview}
                      </code>
                    </>
                  )}
                </div>
                {!diagnose.data.captured && (
                  <p className="mt-2 text-[var(--color-muted)]">
                    Empty capture usually means the popup didn't lose focus
                    fast enough, or there was no text before your cursor.
                    Try again with the cursor placed right after a typed
                    abbreviation.
                  </p>
                )}
                {diagnose.data.captured && !diagnose.data.matched_abbreviation && (
                  <p className="mt-2 text-[var(--color-muted)]">
                    The capture worked, but no snippet has{" "}
                    <code className="rounded bg-[var(--color-bg)] px-1">
                      {diagnose.data.captured}
                    </code>{" "}
                    as its abbreviation. Open the <b>Snippets</b> tab and
                    create one, or pick a different abbreviation.
                  </p>
                )}
              </div>
            )}
            {diagnose?.kind === "err" && (
              <div className="text-[11px] text-red-400">{diagnose.message}</div>
            )}
          </div>

          <details className="mt-5 rounded border border-[var(--color-border)] bg-[var(--color-surface)] p-3 text-[11px] text-[var(--color-muted)]">
            <summary className="cursor-pointer font-medium">How it works</summary>
            <ul className="mt-2 list-disc space-y-1 pl-5">
              <li>
                When you press the hotkey, Inspector Rust synthesizes{" "}
                <kbd className="rounded border border-[var(--color-border)] px-1">
                  Cmd/Ctrl+Shift+←
                </kbd>{" "}
                to select the previous word, copies it, and looks it up in
                your snippets.
              </li>
              <li>
                On a hit, the snippet body is written to the clipboard and{" "}
                <kbd className="rounded border border-[var(--color-border)] px-1">
                  Cmd/Ctrl+V
                </kbd>{" "}
                pastes over the still-selected abbreviation.
              </li>
              <li>
                On a miss, the selection stays visible (visual cue) and your
                clipboard is left untouched.
              </li>
              <li>
                Caveats: <b>terminals</b> (iTerm2, Terminal.app, kitty, …) don&apos;t
                expose the input line via accessibility and have no GUI
                &quot;select previous word&quot; — the abbreviation hotkey does nothing
                there. Use a <b>Direct hotkey → snippet</b> slot (below) or the popup
                instead. Password fields refuse synthetic paste in many apps.
              </li>
            </ul>
          </details>
        </Section>
        </div>

        {/* Clipboard privacy (v0.76.0) */}
        <div className="mt-8">
        <Section
          icon={<Lock size={16} className="text-[var(--color-accent)]" />}
          title="Clipboard privacy"
          subtitle="Keep secrets out of the clipboard history, and auto-wipe sensitive copies."
        >
          {privacy === null ? (
            <p className="text-[12px] text-[var(--color-muted)]">Loading…</p>
          ) : (
            <div className="flex flex-col gap-4">
              <label className="flex flex-col gap-1.5">
                <span className="text-[12px]">
                  <span className="text-[var(--color-fg)]">Never capture from these apps</span>
                  <span className="block text-[11px] text-[var(--color-muted)]">
                    One app-name (or part of it) per line or comma-separated — e.g. 1Password,
                    KeePassXC, Bitwarden. Case-insensitive substring match against the frontmost app.
                  </span>
                </span>
                <textarea
                  value={privacy.exclude_apps}
                  onChange={(e) => setPrivacy({ ...privacy, exclude_apps: e.target.value })}
                  onBlur={() => void savePrivacy(privacy)}
                  rows={3}
                  placeholder={"1Password\nKeePassXC\nBitwarden"}
                  className="rounded-md border border-[var(--color-border)] bg-[var(--color-surface)] p-2 font-[var(--font-mono)] text-[12px]"
                />
              </label>

              <label className="flex items-center justify-between gap-3">
                <span className="text-[12px]">
                  <span className="text-[var(--color-fg)]">Auto-clear the clipboard after</span>
                  <span className="block text-[11px] text-[var(--color-muted)]">
                    Wipes a copied value this many seconds later (a newer copy cancels it). 0 = off.
                  </span>
                </span>
                <span className="flex items-center gap-1.5">
                  <input
                    type="number"
                    min={0}
                    max={3600}
                    value={privacy.auto_clear_seconds}
                    onChange={(e) =>
                      setPrivacy({
                        ...privacy,
                        auto_clear_seconds: Math.max(0, Math.min(3600, parseInt(e.target.value, 10) || 0)),
                      })
                    }
                    onBlur={() => void savePrivacy(privacy)}
                    className="w-20 rounded-md border border-[var(--color-border)] bg-[var(--color-surface)] px-2 py-1 text-right text-[12px] tabular-nums"
                  />
                  <span className="text-[11px] text-[var(--color-muted)]">sec</span>
                </span>
              </label>

              {privacySaved && (
                <span className="flex items-center gap-1.5 text-[11px] text-green-500">
                  <CheckCircle2 size={12} /> Saved
                </span>
              )}
            </div>
          )}
        </Section>
        </div>

        {/* Passive auto-expansion (aText-style) section */}
        <div className="mt-6">
          <Section
            icon={<Wand2 size={16} className="text-[var(--color-accent)]" />}
            title="Auto-Expansion (aText-Stil)"
            subtitle="Snippets expandieren automatisch beim Tippen — ganz ohne Hotkey, in jeder App. Wie aText / TextExpander."
          >
            {accessibility === false && (
              <div className="mb-4 flex items-center gap-2 rounded border border-amber-500/40 bg-amber-500/10 px-3 py-2 text-[12px]">
                <AlertTriangle size={14} className="shrink-0 text-amber-500" />
                <span>
                  Die passive Tastatur-Überwachung braucht macOS-Accessibility — gewähre sie im Abschnitt oben, dann aktiviert sich der Modus.
                </span>
              </div>
            )}

            {aeCfg === null ? (
              <p className="text-[12px] text-[var(--color-muted)]">Lade…</p>
            ) : (
              <div className="flex flex-col gap-3">
                {/* Master toggle */}
                <label className="flex items-center justify-between gap-3">
                  <span className="text-[12px]">
                    <span className="text-[var(--color-fg)]">Auto-Expansion aktivieren</span>
                    <span className="block text-[11px] text-[var(--color-muted)]">
                      Überwacht systemweit deine Eingaben und ersetzt Abkürzungen automatisch.
                    </span>
                  </span>
                  <input
                    type="checkbox"
                    checked={aeCfg.enabled}
                    disabled={aeSaving}
                    onChange={(e) => updateAutoExpand({ enabled: e.target.checked })}
                    className="h-4 w-4 shrink-0 accent-[var(--color-accent)]"
                  />
                </label>

                {/* Trigger choice */}
                <div className={aeCfg.enabled ? "" : "pointer-events-none opacity-50"}>
                  <div className="text-[11px] text-[var(--color-muted)]">Auslöser</div>
                  <div className="mt-1 flex gap-2">
                    {(["delimiter", "immediate"] as const).map((t) => (
                      <button
                        key={t}
                        onClick={() => updateAutoExpand({ trigger: t })}
                        disabled={aeSaving}
                        className={`flex-1 rounded border px-3 py-1.5 text-[11px] ${
                          aeCfg.trigger === t
                            ? "border-[var(--color-accent)] text-[var(--color-accent)]"
                            : "border-[var(--color-border)] text-[var(--color-muted)] hover:border-[var(--color-accent)]"
                        }`}
                      >
                        {t === "delimiter" ? "Nach Trennzeichen" : "Sofort"}
                        <span className="mt-0.5 block text-[10px] opacity-70">
                          {t === "delimiter"
                            ? "Leertaste / Satzzeichen löst aus"
                            : "Sobald die Abkürzung fertig getippt ist"}
                        </span>
                      </button>
                    ))}
                  </div>
                </div>

                {/* Boolean options */}
                <div className={`flex flex-col gap-2 ${aeCfg.enabled ? "" : "pointer-events-none opacity-50"}`}>
                  <label className="flex items-center justify-between gap-3 text-[12px]">
                    <span>Groß-/Kleinschreibung beachten</span>
                    <input
                      type="checkbox"
                      checked={aeCfg.match_case}
                      disabled={aeSaving}
                      onChange={(e) => updateAutoExpand({ match_case: e.target.checked })}
                      className="h-4 w-4 shrink-0 accent-[var(--color-accent)]"
                    />
                  </label>
                  <label className="flex items-center justify-between gap-3 text-[12px]">
                    <span>
                      Auch innerhalb von Wörtern expandieren
                      <span className="block text-[11px] text-[var(--color-muted)]">
                        Aus: nur eigenständige Wörter lösen aus.
                      </span>
                    </span>
                    <input
                      type="checkbox"
                      checked={aeCfg.expand_inside_words}
                      disabled={aeSaving}
                      onChange={(e) => updateAutoExpand({ expand_inside_words: e.target.checked })}
                      className="h-4 w-4 shrink-0 accent-[var(--color-accent)]"
                    />
                  </label>
                  <label className="flex items-center justify-between gap-3 text-[12px]">
                    <span>
                      Rückgängig per Backspace
                      <span className="block text-[11px] text-[var(--color-muted)]">
                        Ein Backspace direkt nach der Expansion stellt die Abkürzung wieder her.
                      </span>
                    </span>
                    <input
                      type="checkbox"
                      checked={aeCfg.undo_enabled}
                      disabled={aeSaving}
                      onChange={(e) => updateAutoExpand({ undo_enabled: e.target.checked })}
                      className="h-4 w-4 shrink-0 accent-[var(--color-accent)]"
                    />
                  </label>
                </div>

                <details className="mt-1 rounded border border-[var(--color-border)] bg-[var(--color-surface)] p-3 text-[11px] text-[var(--color-muted)]">
                  <summary className="cursor-pointer font-medium">Hinweise</summary>
                  <ul className="mt-2 list-disc space-y-1 pl-5">
                    <li>
                      Funktioniert in TextEdit/Notepad, Mail, Slack/Discord (Electron),
                      Chrome/Safari/Edge, VS Code u. v. m.
                    </li>
                    <li>In Passwortfeldern wird <b>nie</b> expandiert.</li>
                    <li>
                      Platzhalter wie <code className="rounded bg-[var(--color-bg)] px-1">{"{date}"}</code>,{" "}
                      <code className="rounded bg-[var(--color-bg)] px-1">{"{clipboard}"}</code> und{" "}
                      <code className="rounded bg-[var(--color-bg)] px-1">{"{cursor}"}</code> greifen auch hier.
                    </li>
                    <li>
                      Terminals: je nach OS eingeschränkt. Linux (Wayland) bietet keinen
                      systemweiten Tap — dort bleibt der Hotkey-/Popup-Weg.
                    </li>
                  </ul>
                </details>
              </div>
            )}
          </Section>
        </div>

        {/* Direct hotkey → snippet section */}
        <div className="mt-6">
          <Section
            icon={<Zap size={16} className="text-[var(--color-accent)]" />}
            title="Direct hotkey → snippet"
            subtitle="Press a hotkey, paste a snippet's body straight away — no abbreviation typed. Reads nothing, so it works in any app, including terminals (iTerm2, Terminal.app, …)."
          >
            {accessibility === false && (
              <div className="mb-4 flex items-center gap-2 rounded border border-amber-500/40 bg-amber-500/10 px-3 py-2 text-[12px]">
                <AlertTriangle size={14} className="shrink-0 text-amber-500" />
                <span>
                  These hotkeys synthesize <kbd className="rounded border border-[var(--color-border)] px-1">Cmd+V</kbd>, so they need macOS Accessibility access too — grant it in the section above.
                </span>
              </div>
            )}

            {slots.length === 0 ? (
              <p className="mb-3 text-[12px] text-[var(--color-muted)]">
                No direct slots yet. Add one to bind a hotkey straight to a snippet.
              </p>
            ) : (
              <div className="mb-3 flex flex-col gap-2">
                {slots.map((slot, i) => {
                  const known = snippetList.some((s) => s.id === slot.snippet_id);
                  return (
                    <div key={i} className="flex flex-wrap items-center gap-2">
                      <HotkeyCapture
                        value={slot.hotkey}
                        onChange={(h) => updateSlot(i, { hotkey: h })}
                        disabled={slotsBusy}
                      />
                      <span className="text-[12px] text-[var(--color-muted)]">→</span>
                      <select
                        value={slot.snippet_id}
                        onChange={(e) =>
                          updateSlot(i, { snippet_id: Number(e.target.value) })
                        }
                        disabled={slotsBusy}
                        className={
                          "max-w-[280px] truncate rounded border bg-[var(--color-surface)] px-2 py-1 text-[12px] " +
                          (known
                            ? "border-[var(--color-border)]"
                            : "border-amber-500/50 text-amber-400")
                        }
                      >
                        {!known && (
                          <option value={slot.snippet_id}>
                            ⚠ snippet deleted — pick another
                          </option>
                        )}
                        {snippetList.map((s) => (
                          <option key={s.id} value={s.id}>
                            {s.abbreviation} — {s.title || "(untitled)"}
                          </option>
                        ))}
                      </select>
                      <button
                        onClick={() => removeSlot(i)}
                        disabled={slotsBusy}
                        className="rounded px-1.5 text-[14px] text-[var(--color-muted)] hover:text-red-400 disabled:opacity-40"
                        title="Remove this slot"
                      >
                        ×
                      </button>
                    </div>
                  );
                })}
              </div>
            )}

            <div className="flex items-center gap-3">
              <button
                onClick={addSlot}
                disabled={slotsBusy}
                className="rounded border border-[var(--color-border)] px-2.5 py-1 text-[11px] hover:border-[var(--color-accent)] hover:text-[var(--color-accent)] disabled:opacity-40"
              >
                + Add slot
              </button>
              <button
                onClick={() => void saveSlots()}
                disabled={slotsBusy || !slotsDirty}
                className="rounded bg-[var(--color-accent)] px-3 py-1 text-[12px] text-[var(--color-accent-fg)] hover:opacity-90 disabled:opacity-50"
              >
                {slotsBusy ? "Saving…" : slotsDirty ? "Save & register" : "No changes"}
              </button>
              {slotsStatus && (
                <span
                  className={
                    "text-[11px] " +
                    (slotsStatus.kind === "ok"
                      ? "text-[var(--color-muted)]"
                      : "text-red-400")
                  }
                >
                  {slotsStatus.message}
                </span>
              )}
            </div>

            <p className="mt-3 text-[11px] leading-snug text-[var(--color-muted)]">
              Pick a hotkey that doesn&apos;t clash with your text-expander hotkey, <kbd className="rounded border border-[var(--color-border)] px-1">{IS_MAC ? "⌃Space" : "Ctrl+Space"}</kbd>, or <kbd className="rounded border border-[var(--color-border)] px-1">{IS_MAC ? "⌃⇧O" : "Ctrl+Shift+O"}</kbd> — the backend rejects collisions. Long bodies are fine (it pastes, doesn&apos;t type). The clipboard is restored afterward.
            </p>
          </Section>
        </div>

        {/* Appearance section */}
        <div className="mt-6">
          <Section
            icon={<SunMoon size={16} className="text-[var(--color-accent)]" />}
            title="Appearance"
            subtitle="Choose a colour theme. System follows your OS light/dark setting; Light and Dark override it."
          >
            <Row label="Theme">
              <div
                role="radiogroup"
                aria-label="Colour theme"
                className="flex overflow-hidden rounded-lg border border-[var(--color-border)]"
              >
                {([
                  { value: "system", label: "System", Icon: Monitor },
                  { value: "light", label: "Light", Icon: Sun },
                  { value: "dark", label: "Dark", Icon: Moon },
                ] as const).map(({ value, label, Icon }) => {
                  const active = theme === value;
                  return (
                    <button
                      key={value}
                      type="button"
                      role="radio"
                      aria-checked={active}
                      disabled={theme === null || themeBusy}
                      onClick={() => void changeTheme(value)}
                      className={
                        "flex items-center gap-1.5 px-3 py-1.5 text-[12px] transition-colors " +
                        "border-r border-[var(--color-border)] last:border-r-0 " +
                        (active
                          ? "bg-[var(--color-accent)] text-[var(--color-accent-fg)]"
                          : "bg-[var(--color-surface)] text-[var(--color-muted)] hover:text-[var(--color-fg)]")
                      }
                    >
                      <Icon size={13} className="shrink-0" />
                      {label}
                    </button>
                  );
                })}
              </div>
            </Row>
            <p className="mt-2 text-[11px] leading-snug text-[var(--color-muted)]">
              {theme === null
                ? "Loading…"
                : theme === "system"
                  ? "Following your operating system's light/dark setting."
                  : `Forced ${theme} — ignores the OS setting until you switch back to System.`}
            </p>

            <Row label="Overlay size">
              <div
                role="radiogroup"
                aria-label="Overlay size"
                className="flex overflow-hidden rounded-lg border border-[var(--color-border)]"
              >
                {([
                  { value: "small", label: "Small" },
                  { value: "medium", label: "Medium" },
                  { value: "large", label: "Large" },
                ] as const).map(({ value, label }) => {
                  const active = winSize === value;
                  return (
                    <button
                      key={value}
                      type="button"
                      role="radio"
                      aria-checked={active}
                      disabled={winSize === null || winSizeBusy}
                      onClick={() => void changeWinSize(value)}
                      className={
                        "flex items-center gap-1.5 px-3 py-1.5 text-[12px] transition-colors " +
                        "border-r border-[var(--color-border)] last:border-r-0 " +
                        (active
                          ? "bg-[var(--color-accent)] text-[var(--color-accent-fg)]"
                          : "bg-[var(--color-surface)] text-[var(--color-muted)] hover:text-[var(--color-fg)]")
                      }
                    >
                      {label}
                    </button>
                  );
                })}
              </div>
            </Row>
            <p className="mt-2 text-[11px] leading-snug text-[var(--color-muted)]">
              {winSize === null
                ? "Loading…"
                : "Popup window dimensions — Small 600×430, Medium 700×500, Large 840×600. Applies immediately and recentres on the next open."}
            </p>
          </Section>
        </div>

        {/* Startup section */}
        <div className="mt-6">
          <Section
            icon={<Power size={16} className="text-[var(--color-accent)]" />}
            title="Startup"
            subtitle={IS_MAC
              ? "Have Inspector Rust launch automatically when you log in. Uses a LaunchAgent (~/Library/LaunchAgents/InspectorRust.plist) — no Dock icon, opens hidden in the tray."
              : "Have Inspector Rust launch automatically when you sign in. Registered via the Windows run-key — opens hidden in the system tray."}
          >
            <Row label={IS_MAC ? "Start at login" : "Start with Windows"}>
              <label className="flex cursor-pointer items-center gap-2 text-[12px]">
                <input
                  type="checkbox"
                  checked={autostart ?? false}
                  disabled={autostart === null || autostartBusy}
                  onChange={(e) => void toggleAutostart(e.target.checked)}
                  className="accent-[var(--color-accent)]"
                />
                <span className="text-[var(--color-muted)]">
                  {autostart === null
                    ? "Loading…"
                    : autostart
                      ? "Enabled — Inspector Rust launches on login"
                      : "Disabled — start Inspector Rust manually"}
                </span>
              </label>
            </Row>
            <Row label="Always keep running">
              <label className="flex cursor-pointer items-start gap-2 text-[12px]">
                <input
                  type="checkbox"
                  checked={keepalive ?? false}
                  disabled={keepalive === null || keepaliveBusy}
                  onChange={(e) => void toggleKeepalive(e.target.checked)}
                  className="mt-0.5 accent-[var(--color-accent)]"
                />
                <span className="text-[var(--color-muted)]">
                  {keepalive === null
                    ? "Loading…"
                    : keepalive
                      ? "On — auto-relaunches within ~30 s if it ever isn't running (crash / quit). Turn off to quit for good."
                      : "Off — the app stays closed once you quit it"}
                </span>
              </label>
            </Row>
          </Section>
        </div>

        {/* Paste behaviour section */}
        <div className="mt-6">
          <Section
            icon={<ClipboardType size={16} className="text-[var(--color-accent)]" />}
            title="Paste"
            subtitle="Control how clipboard entries land in the destination app."
          >
            <Row label="Plain text only">
              <label className="flex cursor-pointer items-center gap-2 text-[12px]">
                <input
                  type="checkbox"
                  checked={plainTextOnly ?? true}
                  disabled={plainTextOnly === null || plainTextSaving}
                  onChange={(e) => void togglePlainText(e.target.checked)}
                  className="accent-[var(--color-accent)]"
                />
                <span className="text-[var(--color-muted)]">
                  {plainTextOnly === null
                    ? "Loading…"
                    : plainTextOnly
                      ? "HTML / RTF entries are stripped to plain text on paste"
                      : "Original formatting is preserved when pasting HTML / RTF"}
                </span>
              </label>
            </Row>

            <div className="mt-1 rounded border border-dashed border-[var(--color-border)] bg-[var(--color-surface)] p-2.5 text-[11px] text-[var(--color-muted)]">
              <span className="text-[var(--color-fg)]">Tip — one-shot override:</span>{" "}
              hold{" "}
              <kbd className="rounded border border-[var(--color-border)] bg-[var(--color-bg)] px-1 font-[var(--font-mono)]">
                Shift
              </kbd>{" "}
              while pressing{" "}
              <kbd className="rounded border border-[var(--color-border)] bg-[var(--color-bg)] px-1 font-[var(--font-mono)]">
                Enter
              </kbd>{" "}
              in the popup to paste with original formatting{" "}
              <em>this once</em>, regardless of the toggle above.
            </div>
          </Section>
        </div>

        {/* Capture section — OCR / screenshot / region-pick behaviours. */}
        <div className="mt-6">
          <Section
            icon={<Camera size={16} className="text-[var(--color-accent)]" />}
            title="Capture"
            subtitle="OCR and screenshot region capture (Ctrl+Shift+O / Ctrl+Shift+S)."
          >
            <Row label="Keep OCR source image in history">
              <label className="flex cursor-pointer items-center gap-2 text-[12px]">
                <input
                  type="checkbox"
                  checked={ocrSaveSource ?? false}
                  disabled={ocrSaveSource === null || ocrSaveSourceSaving}
                  onChange={(e) => void toggleOcrSaveSource(e.target.checked)}
                  className="accent-[var(--color-accent)]"
                />
                <span className="text-[var(--color-muted)]">
                  {ocrSaveSource === null
                    ? "Loading…"
                    : ocrSaveSource
                      ? "OCR saves both the source PNG and the recognised text to history"
                      : "OCR saves only the recognised text — the source PNG is discarded"}
                </span>
              </label>
            </Row>
          </Section>
        </div>

        {/* Input Lock section — type `freeze` in the search bar to
            block all keyboard / mouse input; release with the chord. */}
        <div className="mt-6">
          <Section
            icon={<Lock size={16} className="text-[var(--color-accent)]" />}
            title="Input lock"
            subtitle="Type `freeze` in the popup to block all keyboard + mouse input — release with the configured chord."
          >
            <Row
              label="Unlock chord"
              help="Click Capture, then press the keys you want to use simultaneously. Release any key to save. Esc cancels."
            >
              <div className="flex flex-wrap items-center gap-2">
                <div className="flex items-center gap-1 rounded border border-[var(--color-border)] bg-[var(--color-surface)] px-2 py-1 font-[var(--font-mono)] text-[12px]">
                  {capturedChord !== null ? (
                    capturedChord.size === 0 ? (
                      <span className="text-[var(--color-muted)]">Press your chord…</span>
                    ) : (
                      Array.from(capturedChord).map((k, i) => (
                        <span key={i} className="rounded bg-[var(--color-bg)] px-1.5 py-0.5">
                          {k}
                        </span>
                      ))
                    )
                  ) : inputLockChord === null ? (
                    <span className="text-[var(--color-muted)]">Loading…</span>
                  ) : inputLockChord.length === 0 ? (
                    <span className="text-[var(--color-muted)]">(none)</span>
                  ) : (
                    inputLockChord.map((k, i) => (
                      <span key={i} className="rounded bg-[var(--color-bg)] px-1.5 py-0.5">
                        {k}
                      </span>
                    ))
                  )}
                </div>
                <button
                  type="button"
                  onClick={() => setCapturedChord(new Set())}
                  disabled={inputLockChord === null || inputLockSaving || capturedChord !== null}
                  className="rounded border border-[var(--color-border)] px-2.5 py-1 text-[11px] hover:border-[var(--color-accent)] hover:text-[var(--color-accent)] disabled:opacity-40"
                >
                  {capturedChord !== null ? "Capturing…" : "Capture"}
                </button>
                {capturedChord !== null && (
                  <button
                    type="button"
                    onClick={() => setCapturedChord(null)}
                    className="rounded border border-[var(--color-border)] px-2.5 py-1 text-[11px] text-[var(--color-muted)] hover:border-amber-500 hover:text-amber-500"
                  >
                    Cancel
                  </button>
                )}
                {inputLockStatus && (
                  <span
                    className={
                      "text-[11px] " +
                      (inputLockStatus.kind === "ok"
                        ? "text-[var(--color-muted)]"
                        : "text-red-400")
                    }
                  >
                    {inputLockStatus.message}
                  </span>
                )}
              </div>
            </Row>

            <div className="mt-1 rounded border border-dashed border-[var(--color-border)] bg-[var(--color-surface)] p-2.5 text-[11px] text-[var(--color-muted)]">
              <span className="text-[var(--color-fg)]">Safety:</span> OS-level
              shortcuts ({IS_MAC ? "⌥⌘Esc Force Quit" : "Ctrl+Alt+Del"}) always
              work — Inspector Rust can't intercept them, so you can never be
              truly locked out. Linux Wayland is not supported by the underlying
              grab API (X11 sessions only).
            </div>
          </Section>
        </div>

        {/* Keyboard shortcuts cheat sheet */}
        <div className="mt-6">
          <Section
            icon={<Keyboard size={16} className="text-[var(--color-accent)]" />}
            title="Keyboard shortcuts"
            subtitle="Global shortcuts fire from anywhere on your system. Popup shortcuts only fire while Inspector Rust's popup is visible."
          >
            <ShortcutsTable />
          </Section>
        </div>

        {/* Cleaning workflow (v0.60.0) */}
        <div className="mt-6">
          <Section
            icon={<Trash2 size={16} className="text-[var(--color-accent)]" />}
            title="Cleaning"
            subtitle="Delete cache / log / temp files inside known-safe folders. Type `clean` in the search bar — it always shows a preview and deletes only after you confirm."
          >
            {cleanCfg === null ? (
              <p className="text-[12px] text-[var(--color-muted)]">Lade…</p>
            ) : (
              <div className="flex flex-col gap-3">
                {/* Level picker */}
                <div>
                  <div className="text-[11px] text-[var(--color-muted)]">Umfang</div>
                  <div className="mt-1 flex gap-2">
                    {(["safe", "standard", "aggressive"] as CleanerLevel[]).map((lv) => (
                      <button
                        key={lv}
                        onClick={() => void updateCleaner({ level: lv })}
                        className={`flex-1 rounded border px-3 py-1.5 text-[11px] capitalize ${
                          cleanCfg.level === lv
                            ? "border-[var(--color-accent)] text-[var(--color-accent)]"
                            : "border-[var(--color-border)] text-[var(--color-muted)] hover:border-[var(--color-accent)]"
                        }`}
                      >
                        {lv === "safe" ? "Safe" : lv === "standard" ? "Standard" : "Aggressive"}
                      </button>
                    ))}
                  </div>
                  {cleanCfg.level === "aggressive" && (
                    <div className="mt-2 flex items-center gap-2 rounded border border-amber-500/40 bg-amber-500/10 px-3 py-2 text-[11px]">
                      <AlertTriangle size={13} className="shrink-0 text-amber-500" />
                      <span>
                        Aggressive löscht auch globale Dev-Tool-Caches (npm/pnpm/Gradle/Cargo) — diese müssen danach neu geladen werden.
                      </span>
                    </div>
                  )}
                </div>

                {/* Min age */}
                <label className="flex items-center justify-between gap-3 text-[12px]">
                  <span>
                    Nur Dateien älter als
                    <span className="block text-[11px] text-[var(--color-muted)]">
                      Tage seit letzter Änderung.
                    </span>
                  </span>
                  <input
                    type="number"
                    min={0}
                    max={3650}
                    value={cleanCfg.min_age_days}
                    onChange={(e) =>
                      void updateCleaner({
                        min_age_days: Math.max(0, parseInt(e.target.value, 10) || 0),
                      })
                    }
                    className="w-20 rounded border border-[var(--color-border)] bg-[var(--color-bg)] px-2 py-1 text-right text-[12px]"
                  />
                </label>

                {/* Category checkboxes for the current level */}
                <div className="flex flex-col gap-1.5">
                  <div className="text-[11px] text-[var(--color-muted)]">Kategorien</div>
                  {cleanCats
                    .filter((c) => {
                      const rank = (l: CleanerLevel) =>
                        l === "safe" ? 0 : l === "standard" ? 1 : 2;
                      return rank(c.level) <= rank(cleanCfg.level);
                    })
                    .map((c) => {
                      const enabled = cleanCfg.categories[c.key] ?? c.default_enabled;
                      return (
                        <label
                          key={c.key}
                          className="flex items-center justify-between gap-3 text-[12px]"
                        >
                          <span>{c.label}</span>
                          <input
                            type="checkbox"
                            checked={enabled}
                            onChange={(e) =>
                              void updateCleaner({
                                categories: {
                                  ...cleanCfg.categories,
                                  [c.key]: e.target.checked,
                                },
                              })
                            }
                            className="h-4 w-4 shrink-0 accent-[var(--color-accent)]"
                          />
                        </label>
                      );
                    })}
                </div>
                <p className="text-[11px] text-[var(--color-muted)]">
                  Es werden ausschließlich Dateien innerhalb fest definierter Cache-/Log-/Temp-Ordner gelöscht — niemals Dokumente, Desktop o. Ä. Symlinks werden nie verfolgt.
                </p>
              </div>
            )}
          </Section>
        </div>

        {/* Timesheet (time tracking) — macOS for now (OS module is macOS-only) */}
        {IS_MAC && (
          <div className="mt-6">
            <TimesheetSection />
          </div>
        )}

        {/* Bruno defaults — German income-tax calculator personal params */}
        <div className="mt-6">
          <BrunoSection />
        </div>

        {/* Meme library directory */}
        {MEME_ENABLED && (
          <div className="mt-6">
            <MemeSection />
          </div>
        )}

        {/* Backup & restore section */}
        <div className="mt-6">
          <Section
            icon={<Archive size={16} className="text-[var(--color-accent)]" />}
            title="Backup & restore"
            subtitle="Export everything (or just parts) to a single JSON file. Import merges back: snippets upsert by abbreviation, history dedupes by SHA-256, notes append."
          >
            <Row label="What to export">
              <div className="flex flex-col gap-1.5 text-[12px]">
                <BackupCheckbox
                  label="Clipboard history"
                  checked={includeHistory}
                  onChange={setIncludeHistory}
                />
                <BackupCheckbox
                  label="Snippets"
                  checked={includeSnippets}
                  onChange={setIncludeSnippets}
                />
                <BackupCheckbox
                  label="Notes"
                  checked={includeNotes}
                  onChange={setIncludeNotes}
                />
              </div>
            </Row>

            <div className="mt-4 flex flex-wrap items-center gap-2">
              <button
                onClick={() => void onExport()}
                disabled={backupBusy !== null}
                className="flex items-center gap-1.5 rounded bg-[var(--color-accent)] px-3 py-1 text-[12px] text-[var(--color-accent-fg)] hover:opacity-90 disabled:opacity-50"
              >
                <Download size={12} />
                {backupBusy === "export" ? "Exporting…" : "Export…"}
              </button>
              <button
                onClick={() => void onImport()}
                disabled={backupBusy !== null}
                className="flex items-center gap-1.5 rounded border border-[var(--color-border)] px-3 py-1 text-[12px] hover:border-[var(--color-accent)] hover:text-[var(--color-accent)] disabled:opacity-50"
              >
                <Upload size={12} />
                {backupBusy === "import" ? "Importing…" : "Import…"}
              </button>
              <span className="ml-auto text-[11px] text-[var(--color-muted)]">
                Import merges into the current database
              </span>
            </div>

            {backupStatus && (
              <div
                className={
                  "mt-3 rounded border px-2.5 py-1.5 text-[11px] " +
                  (backupStatus.kind === "err"
                    ? "border-red-500/40 bg-red-500/5 text-red-400"
                    : "border-[var(--color-border)] bg-[var(--color-surface)] text-[var(--color-muted)]")
                }
              >
                {backupStatus.kind === "ok" ? (
                  backupStatus.message
                ) : backupStatus.kind === "import-ok" ? (
                  <>
                    Imported{" "}
                    <b>{backupStatus.result.notes_imported}</b> notes,{" "}
                    <b>{backupStatus.result.snippets_imported}</b> snippets,{" "}
                    <b>{backupStatus.result.history_imported}</b> history
                    {backupStatus.result.errors.length > 0 && (
                      <>
                        {" — "}
                        <span className="text-red-400">
                          {backupStatus.result.errors[0]}
                          {backupStatus.result.errors.length > 1 &&
                            ` (+${backupStatus.result.errors.length - 1} more)`}
                        </span>
                      </>
                    )}
                  </>
                ) : (
                  <>Failed: {backupStatus.message}</>
                )}
              </div>
            )}
          </Section>
        </div>

        {IS_LINUX && (
          <div className="mt-6">
            <LinuxShortcutsSettings />
          </div>
        )}

        {/* About section — inline (replaced the former modal dialog) */}
        <div className="mt-6">
          <Section
            icon={<Info size={16} className="text-[var(--color-accent)]" />}
            title="About"
            subtitle="Version, license, project info."
          >
            <AboutContent version={appVersion} />
          </Section>
        </div>
      </div>
    </div>
  );
}

/** One permission's live status row inside the macOS-permissions card.
 *  `granted`: true = enabled, false = missing, null = still loading. */
/** Map a `KeyboardEvent.key` (UI-layer key name) to the backend's
 *  key-name vocabulary (`input_lock::key_from_str`). Returns `null`
 *  for keys the backend can't represent (function keys, modifiers,
 *  arrows, …) so the chord-capture handler can ignore them. */
function chordKeyName(eventKey: string): string | null {
  if (eventKey === " ") return "space";
  if (eventKey === "Enter") return "return";
  if (eventKey === "Tab") return "tab";
  if (eventKey === "Escape") return "escape";
  if (eventKey.length === 1) {
    const lo = eventKey.toLowerCase();
    if (/^[a-z0-9]$/.test(lo)) return lo;
  }
  return null;
}

function PermRow({
  label,
  hint,
  granted,
  onOpen,
}: {
  label: string;
  hint: string;
  granted: boolean | null;
  onOpen: () => void;
}) {
  return (
    <div className="flex items-center gap-2.5 rounded border border-[var(--color-border)] bg-[var(--color-surface)] px-2.5 py-1.5">
      {granted === true ? (
        <CheckCircle2 size={15} className="shrink-0 text-emerald-500" />
      ) : (
        <span
          className="h-3.5 w-3.5 shrink-0 rounded-full border-2 border-amber-500"
          aria-hidden
        />
      )}
      <div className="min-w-0 flex-1">
        <div className="font-medium text-[var(--color-fg)]">{label}</div>
        <div className="truncate text-[10px] text-[var(--color-muted)]">{hint}</div>
      </div>
      {granted === true ? (
        <span className="shrink-0 text-[11px] font-medium text-emerald-500">
          Enabled
        </span>
      ) : (
        <button
          onClick={onOpen}
          className="shrink-0 rounded bg-[var(--color-accent)] px-2.5 py-1 text-[11px] text-[var(--color-accent-fg)] hover:opacity-90"
        >
          Open
        </button>
      )}
    </div>
  );
}

function BackupCheckbox({
  label,
  checked,
  onChange,
}: {
  label: string;
  checked: boolean;
  onChange: (v: boolean) => void;
}) {
  return (
    <label className="flex cursor-pointer items-center gap-2">
      <input
        type="checkbox"
        checked={checked}
        onChange={(e) => onChange(e.target.checked)}
        className="accent-[var(--color-accent)]"
      />
      <span>{label}</span>
    </label>
  );
}

function TimesheetSection() {
  const [cfg, setCfg] = useState<{
    idle_seconds: number;
    retention_days: number;
    claude_watcher: boolean;
    denylist: string;
    daily_goal_minutes: number;
  } | null>(null);
  const [saved, setSaved] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [savedOk, setSavedOk] = useState(false);

  useEffect(() => {
    getTimesheetConfig()
      .then((c) => {
        setCfg(c);
        setSaved(JSON.stringify(c));
      })
      .catch(() => undefined);
  }, []);

  if (!cfg) return null;
  const dirty = saved !== JSON.stringify(cfg);

  const save = async () => {
    setBusy(true);
    setSavedOk(false);
    try {
      await setTimesheetConfig(cfg);
      setSaved(JSON.stringify(cfg));
      setSavedOk(true);
    } catch (e) {
      console.error("save timesheet config", e);
    } finally {
      setBusy(false);
    }
  };

  return (
    <Section
      icon={<Clock size={16} className="text-[var(--color-accent)]" />}
      title="Timesheet"
      subtitle="Opt-in time tracking (track on/off). Records app usage by focus; idle auto-pauses; window titles + URLs are encrypted at rest. Nothing leaves your machine (the browser extension uses a loopback socket only)."
    >
      <Row label="Idle threshold (seconds)" help="Inactivity beyond this retroactively pauses the active interval as idle.">
        <input
          type="number"
          min={10}
          value={cfg.idle_seconds}
          onChange={(e) => setCfg({ ...cfg, idle_seconds: Number(e.target.value) || 300 })}
          className="w-28 rounded border border-[var(--color-border)] bg-[var(--color-surface)] px-2 py-1 text-[12px] tabular-nums"
        />
      </Row>
      <Row label="Retention (days)" help="On the next “track on”, events older than this are deleted. 0 = keep forever.">
        <input
          type="number"
          min={0}
          value={cfg.retention_days}
          onChange={(e) => setCfg({ ...cfg, retention_days: Number(e.target.value) || 0 })}
          className="w-28 rounded border border-[var(--color-border)] bg-[var(--color-surface)] px-2 py-1 text-[12px] tabular-nums"
        />
      </Row>
      <Row label="Claude Code usage" help="Watch ~/.claude/projects for active Claude Code usage per project (time + tokens).">
        <label className="flex cursor-pointer items-center gap-2 text-[12px]">
          <input
            type="checkbox"
            checked={cfg.claude_watcher}
            onChange={(e) => setCfg({ ...cfg, claude_watcher: e.target.checked })}
            className="accent-[var(--color-accent)]"
          />
          <span className="text-[var(--color-muted)]">
            {cfg.claude_watcher ? "On — track Claude Code time" : "Off"}
          </span>
        </label>
      </Row>
      <Row label="Daily focus goal (minutes)" help="Target active time per day (0 = off). Shows a progress bar in the Timesheet day view.">
        <input
          type="number"
          min={0}
          step={30}
          value={cfg.daily_goal_minutes}
          onChange={(e) => setCfg({ ...cfg, daily_goal_minutes: Number(e.target.value) || 0 })}
          className="w-28 rounded border border-[var(--color-border)] bg-[var(--color-surface)] px-2 py-1 text-[12px] tabular-nums"
        />
      </Row>
      <Row label="Privacy denylist" help="Comma/newline-separated app names or hostnames. For matches, only the app name + time are stored — no window title or URL.">
        <textarea
          value={cfg.denylist}
          placeholder="1Password, KeePassXC, bank.com"
          onChange={(e) => setCfg({ ...cfg, denylist: e.target.value })}
          rows={2}
          className="w-full rounded border border-[var(--color-border)] bg-[var(--color-surface)] px-2 py-1 text-[12px]"
        />
      </Row>
      <div className="mt-2 flex items-center gap-2">
        <button
          onClick={() => void save()}
          disabled={!dirty || busy}
          className="rounded bg-[var(--color-accent)] px-3 py-1 text-[12px] font-medium text-[var(--color-accent-fg)] hover:opacity-90 disabled:opacity-40"
        >
          {busy ? "Saving…" : "Save"}
        </button>
        {dirty && !busy && <span className="text-[11px] text-[var(--color-muted)]">Unsaved changes</span>}
        {savedOk && !dirty && <span className="text-[11px] text-[var(--color-muted)]">Saved</span>}
      </div>
      <Row label="App → category rules" help="Auto-categorize future events. Set rules by assigning a category to an app in the Timesheet's “By app” view; remove them here.">
        <CategoryRulesList />
      </Row>
    </Section>
  );
}

function CategoryRulesList() {
  const [rules, setRules] = useState<[string, string][]>([]);
  const reload = () => trackCategoryRules().then(setRules).catch(() => undefined);
  useEffect(() => {
    void reload();
  }, []);
  if (rules.length === 0) {
    return <p className="text-[12px] text-[var(--color-muted)]">No rules yet.</p>;
  }
  return (
    <div className="flex flex-col gap-1">
      {rules.map(([app, cat]) => (
        <div key={app} className="flex items-center gap-2 text-[12px]">
          <span className="min-w-0 flex-1 truncate">{app}</span>
          <span className="shrink-0 rounded-full bg-[var(--color-accent)]/15 px-1.5 text-[11px] text-[var(--color-accent)]">
            {cat}
          </span>
          <button
            type="button"
            onClick={() => void trackDeleteCategoryRule(app).then(reload)}
            className="shrink-0 rounded p-1 text-[var(--color-muted)] hover:bg-[var(--color-surface)] hover:text-rose-400"
            title="Delete rule"
          >
            <Trash2 size={13} />
          </button>
        </div>
      ))}
    </div>
  );
}

function Section({
  icon,
  title,
  subtitle,
  children,
}: {
  icon: React.ReactNode;
  title: string;
  subtitle?: string;
  children: React.ReactNode;
}) {
  return (
    <div>
      <div className="mb-1 flex items-center gap-2">
        {icon}
        <h2 className="text-[14px] font-semibold">{title}</h2>
      </div>
      {subtitle && (
        <p className="mb-4 text-[12px] text-[var(--color-muted)]">
          {subtitle}
        </p>
      )}
      <div className="rounded-lg border border-[var(--color-border)] p-4">
        {children}
      </div>
    </div>
  );
}

function Row({
  label,
  help,
  children,
}: {
  label: string;
  help?: string;
  children: React.ReactNode;
}) {
  return (
    <div className="mb-4 last:mb-0">
      <div className="mb-1 flex items-center gap-2 text-[12px] font-medium">
        <Zap size={11} className="text-[var(--color-accent)] opacity-70" />
        <span>{label}</span>
      </div>
      <div>{children}</div>
      {help && (
        <p className="mt-1 text-[11px] leading-snug text-[var(--color-muted)]">
          {help}
        </p>
      )}
    </div>
  );
}

// Re-export so `import { Keyboard } from ...` keeps the icon utility nearby
// when we add more settings sections.
export { Keyboard };

// ── Keyboard shortcuts cheat sheet ─────────────────────────────────────────

/** Reference table for every shortcut the app binds. Three groups so
 *  the user can scan quickly for "what fires from anywhere" vs.
 *  "what only works inside the popup". Modifier glyphs adapt to the
 *  current OS via `IS_MAC` from `lib/platform.ts`. */
function ShortcutsTable() {
  const cmd = IS_MAC ? "⌘" : "Ctrl";
  const shift = IS_MAC ? "⇧" : "Shift";
  const alt = IS_MAC ? "⌥" : "Alt";
  const join = IS_MAC ? "" : "+";
  const k = (...parts: string[]) => parts.join(join);

  const groups: Array<{ heading: string; rows: Array<[string, string, string?]> }> = [
    {
      heading: "Global — work from anywhere",
      rows: [
        [k("Ctrl", shift, "V"), "Open Inspector Rust popup", "OS-locked, not configurable"],
        [k("Ctrl", shift, "O"), "OCR region capture", IS_MAC ? "Drag a marquee over text on screen → text → clipboard" : "Stub — macOS-only for now"],
        [k("Ctrl", shift, "S"), "Screenshot region", IS_MAC ? "Drag a marquee → PNG → clipboard + history (no OCR)" : "Stub — macOS-only for now"],
        [k("Ctrl", shift, "C"), "Color picker — eyedropper", "Click a pixel → hex (#RRGGBB) → clipboard + history (v0.17.0+)"],
        [k(alt, "1"), "Trigger text expander", "Default Alt+1 — configurable above; opt-in"],
      ],
    },
    {
      heading: "Popup — list navigation",
      rows: [
        ["⏎", "Paste selected entry", "Plain text downgrade follows the Paste setting"],
        [k(shift, "⏎"), "Paste with original formatting", "One-shot override of the plain-text setting"],
        ["↑ / ↓", "Navigate entries"],
        [k(shift, "↑ / ↓"), "System volume up / down", "±6% per press (macOS)"],
        ["Esc", "Close popup"],
      ],
    },
    {
      heading: "Popup — image entry actions",
      rows: [
        [k(cmd, "B"), "Cut out background → ~/Downloads", "Real subject segmentation via U²-Net"],
        [k(cmd, "S"), "Save image to Downloads", "Saves the entry's PNG bytes unchanged"],
      ],
    },
    {
      heading: "Popup — text entry transforms",
      rows: [
        [`${cmd}1 … ${cmd}9`, "Apply a string transform", "On a selected text entry — see the preview-pane toolbar"],
        [k(cmd, "1"), "Remove vowels"],
        [k(cmd, "2"), "UPPERCASE", `${cmd}3 lowercase · ${cmd}4 Title Case`],
        [k(cmd, "5"), "camelCase", `${cmd}6 snake_case · ${cmd}7 kebab-case`],
        [k(cmd, "8"), "Base64 encode", `${cmd}9 URL encode · decode pair is click-only`],
      ],
    },
  ];

  return (
    <div className="space-y-4">
      {groups.map((g) => (
        <div key={g.heading}>
          <div className="mb-1.5 text-[10px] font-semibold uppercase tracking-wide text-[var(--color-muted)]">
            {g.heading}
          </div>
          <table className="w-full text-[12px]">
            <tbody>
              {g.rows.map(([keys, action, hint]) => (
                <tr key={keys + action} className="align-top">
                  <td className="w-[140px] py-1 pr-3">
                    <kbd className="inline-block rounded border border-[var(--color-border)] bg-[var(--color-surface)] px-1.5 py-0.5 font-[var(--font-mono)] text-[11px]">
                      {keys}
                    </kbd>
                  </td>
                  <td className="py-1 pr-2">{action}</td>
                  {hint && (
                    <td className="py-1 text-right text-[10px] text-[var(--color-muted)]">
                      {hint}
                    </td>
                  )}
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      ))}
    </div>
  );
}

// ── Bruno (Brutto→Netto) personal defaults ─────────────────────────

const BRUNO_STATES: Array<{ code: string; name: string }> = [
  { code: "bw", name: "Baden-Württemberg" },
  { code: "by", name: "Bayern" },
  { code: "be", name: "Berlin" },
  { code: "bb", name: "Brandenburg" },
  { code: "hb", name: "Bremen" },
  { code: "hh", name: "Hamburg" },
  { code: "he", name: "Hessen" },
  { code: "mv", name: "Mecklenburg-Vorpommern" },
  { code: "ni", name: "Niedersachsen" },
  { code: "nw", name: "Nordrhein-Westfalen" },
  { code: "rp", name: "Rheinland-Pfalz" },
  { code: "sl", name: "Saarland" },
  { code: "sn", name: "Sachsen" },
  { code: "st", name: "Sachsen-Anhalt" },
  { code: "sh", name: "Schleswig-Holstein" },
  { code: "th", name: "Thüringen" },
];

/** Configure the global popup-show hotkey. Backend registers it at
 *  startup from the same settings key. Validates against the still-
 *  hard-coded global shortcuts (OCR / Screenshot / Eyedropper / Finder
 *  Ctrl+Shift+O/S/C/F) + expander + direct slots before applying. */
function PopupHotkeySection() {
  const [hotkey, setHotkey] = useState<string>("");
  const [defaultHotkey, setDefaultHotkey] = useState<string>("");
  const [stored, setStored] = useState<string>("");
  const [busy, setBusy] = useState(false);
  const [status, setStatus] = useState<{ kind: "ok" | "err"; message: string } | null>(null);

  useEffect(() => {
    void Promise.all([getPopupHotkey(), getPopupHotkeyDefault()])
      .then(([h, d]) => {
        setHotkey(h);
        setStored(h);
        setDefaultHotkey(d);
      })
      .catch((e) => setStatus({ kind: "err", message: String(e) }));
  }, []);

  const dirty = hotkey.trim() !== "" && hotkey !== stored;

  const save = async () => {
    if (!hotkey.trim()) return;
    setBusy(true);
    setStatus(null);
    try {
      const applied = await setPopupHotkey(hotkey);
      setHotkey(applied);
      setStored(applied);
      setStatus({ kind: "ok", message: `Popup hotkey armed: ${prettyHotkey(applied)}` });
    } catch (e) {
      setStatus({ kind: "err", message: String(e) });
    } finally {
      setBusy(false);
    }
  };

  // Platform-aware presets. Bare modifiers (just Cmd / Win / Super)
  // aren't supported by the OS-level global-shortcut plugin — registering
  // those would need a separate event-tap implementation per OS. So the
  // presets all carry at least one regular key.
  const PRESETS: ReadonlyArray<{ label: string; value: string }> = IS_MAC
    ? [
        { label: "⌃Space (default)", value: "Ctrl+Space" },
        { label: "⌃⇧V", value: "Ctrl+Shift+KeyV" },
        { label: "⌘⇧Space", value: "Cmd+Shift+Space" },
        { label: "⌘J", value: "Cmd+KeyJ" },
      ]
    : IS_LINUX
      ? [
          { label: "Ctrl+Space (default)", value: "Ctrl+Space" },
          { label: "Ctrl+Shift+V", value: "Ctrl+Shift+KeyV" },
          { label: "Super+Space", value: "Super+Space" },
          { label: "Alt+Space", value: "Alt+Space" },
        ]
      : [
          { label: "Ctrl+Space (default)", value: "Ctrl+Space" },
          { label: "Ctrl+Shift+V", value: "Ctrl+Shift+KeyV" },
          { label: "Win+Shift+V", value: "Win+Shift+KeyV" },
          { label: "Alt+Space", value: "Alt+Space" },
        ];

  const platformModifier = IS_MAC ? "⌘" : IS_LINUX ? "Super" : "Win";

  return (
    <Section
      icon={<Keyboard size={16} className="text-[var(--color-accent)]" />}
      title="Popup hotkey"
      subtitle="The global shortcut that opens the search popup. Change it if the default collides with another app you use."
    >
      <Row
        label="Hotkey"
        help={`Press a key combination, or pick a preset. Backspace clears, Esc cancels. Bare ${platformModifier} alone isn't supported by the global-shortcut plugin — pick a combo with at least one regular key (Cmd+Space, Cmd+J, …).`}
      >
        <div className="flex flex-wrap items-center gap-2">
          <HotkeyCapture value={hotkey} onChange={setHotkey} disabled={busy} />
          <button
            onClick={() => defaultHotkey && setHotkey(defaultHotkey)}
            disabled={busy || !defaultHotkey || hotkey === defaultHotkey}
            className="rounded border border-[var(--color-border)] px-2 py-1 text-[11px] text-[var(--color-muted)] hover:border-[var(--color-accent)] hover:text-[var(--color-fg)] disabled:opacity-40"
            title={defaultHotkey ? `Reset to ${prettyHotkey(defaultHotkey)}` : ""}
          >
            Reset
          </button>
          <span className="text-[11px] text-[var(--color-muted)]">presets:</span>
          {PRESETS.map((p) => {
            const active = hotkey === p.value;
            return (
              <button
                key={p.value}
                onClick={() => setHotkey(p.value)}
                disabled={busy}
                className={
                  "rounded border px-2 py-1 text-[11px] disabled:opacity-40 " +
                  (active
                    ? "border-[var(--color-accent)] text-[var(--color-accent)]"
                    : "border-[var(--color-border)] hover:border-[var(--color-accent)] hover:text-[var(--color-accent)]")
                }
              >
                {p.label}
              </button>
            );
          })}
        </div>
      </Row>

      <div className="mt-4 flex items-center gap-2">
        <button
          onClick={() => void save()}
          disabled={busy || !dirty}
          className="rounded bg-[var(--color-accent)] px-3 py-1 text-[12px] text-[var(--color-accent-fg)] hover:opacity-90 disabled:opacity-40"
        >
          {busy ? "Saving…" : "Save"}
        </button>
        {!dirty && stored && (
          <span className="text-[11px] text-[var(--color-muted)]">
            Currently armed: <code className="rounded bg-[var(--color-surface)] px-1 font-[var(--font-mono)]">{prettyHotkey(stored)}</code>
          </span>
        )}
      </div>

      {status && (
        <div
          className={
            "mt-3 rounded border px-3 py-2 text-[12px] " +
            (status.kind === "ok"
              ? "border-emerald-500/40 bg-emerald-500/5 text-[var(--color-fg)]"
              : "border-amber-500/40 bg-amber-500/5 text-[var(--color-fg)]")
          }
        >
          {status.message}
        </div>
      )}
    </Section>
  );
}

// ── Clipboard-history hotkey ───────────────────────────────────────────
// A SECOND, optional global shortcut that also opens the popup (clipboard
// history), alongside the main popup hotkey. Default Ctrl+Shift+V. An empty
// value disables it.
function HistoryHotkeySection() {
  const [hotkey, setHotkey] = useState<string>("");
  const [defaultHotkey, setDefaultHotkey] = useState<string>("");
  const [stored, setStored] = useState<string>("");
  const [busy, setBusy] = useState(false);
  const [status, setStatus] = useState<{ kind: "ok" | "err"; message: string } | null>(null);

  useEffect(() => {
    void Promise.all([getHistoryHotkey(), getHistoryHotkeyDefault()])
      .then(([h, d]) => {
        setHotkey(h);
        setStored(h);
        setDefaultHotkey(d);
      })
      .catch((e) => setStatus({ kind: "err", message: String(e) }));
  }, []);

  // Empty is a valid value here (= disabled), so any change is dirty.
  const dirty = hotkey !== stored;

  const save = async () => {
    setBusy(true);
    setStatus(null);
    try {
      const applied = await setHistoryHotkey(hotkey);
      setHotkey(applied);
      setStored(applied);
      setStatus({
        kind: "ok",
        message: applied.trim()
          ? `Clipboard-history hotkey armed: ${prettyHotkey(applied)}`
          : "Clipboard-history hotkey disabled",
      });
    } catch (e) {
      setStatus({ kind: "err", message: String(e) });
    } finally {
      setBusy(false);
    }
  };

  const PRESETS: ReadonlyArray<{ label: string; value: string }> = IS_MAC
    ? [
        { label: "⌃⇧V (default)", value: "Ctrl+Shift+KeyV" },
        { label: "⌘⇧V", value: "Cmd+Shift+KeyV" },
        { label: "⌘⇧C", value: "Cmd+Shift+KeyC" },
      ]
    : [
        { label: "Ctrl+Shift+V (default)", value: "Ctrl+Shift+KeyV" },
        { label: "Ctrl+Shift+H", value: "Ctrl+Shift+KeyH" },
        { label: "Alt+V", value: "Alt+KeyV" },
      ];

  return (
    <Section
      icon={<Keyboard size={16} className="text-[var(--color-accent)]" />}
      title="Clipboard-history hotkey"
      subtitle="A second, optional global shortcut that also opens the clipboard history — alongside the main popup hotkey. Clear it to disable."
    >
      <Row
        label="Hotkey"
        help="Press a key combination, or pick a preset. Backspace / the Disable button clears it (turns the second hotkey off)."
      >
        <div className="flex flex-wrap items-center gap-2">
          <HotkeyCapture value={hotkey} onChange={setHotkey} disabled={busy} />
          <button
            onClick={() => setHotkey("")}
            disabled={busy || hotkey === ""}
            className="rounded border border-[var(--color-border)] px-2 py-1 text-[11px] text-[var(--color-muted)] hover:border-[var(--color-accent)] hover:text-[var(--color-fg)] disabled:opacity-40"
            title="Disable the second hotkey"
          >
            Disable
          </button>
          <button
            onClick={() => defaultHotkey && setHotkey(defaultHotkey)}
            disabled={busy || !defaultHotkey || hotkey === defaultHotkey}
            className="rounded border border-[var(--color-border)] px-2 py-1 text-[11px] text-[var(--color-muted)] hover:border-[var(--color-accent)] hover:text-[var(--color-fg)] disabled:opacity-40"
            title={defaultHotkey ? `Reset to ${prettyHotkey(defaultHotkey)}` : ""}
          >
            Reset
          </button>
          <span className="text-[11px] text-[var(--color-muted)]">presets:</span>
          {PRESETS.map((p) => {
            const active = hotkey === p.value;
            return (
              <button
                key={p.value}
                onClick={() => setHotkey(p.value)}
                disabled={busy}
                className={
                  "rounded border px-2 py-1 text-[11px] disabled:opacity-40 " +
                  (active
                    ? "border-[var(--color-accent)] text-[var(--color-accent)]"
                    : "border-[var(--color-border)] hover:border-[var(--color-accent)] hover:text-[var(--color-accent)]")
                }
              >
                {p.label}
              </button>
            );
          })}
        </div>
      </Row>

      <div className="mt-4 flex items-center gap-2">
        <button
          onClick={() => void save()}
          disabled={busy || !dirty}
          className="rounded bg-[var(--color-accent)] px-3 py-1 text-[12px] text-[var(--color-accent-fg)] hover:opacity-90 disabled:opacity-40"
        >
          {busy ? "Saving…" : "Save"}
        </button>
        {!dirty && (
          <span className="text-[11px] text-[var(--color-muted)]">
            {stored.trim() ? (
              <>
                Currently armed:{" "}
                <code className="rounded bg-[var(--color-surface)] px-1 font-[var(--font-mono)]">
                  {prettyHotkey(stored)}
                </code>
              </>
            ) : (
              "Disabled"
            )}
          </span>
        )}
      </div>

      {status && (
        <div
          className={
            "mt-3 rounded border px-3 py-2 text-[12px] " +
            (status.kind === "ok"
              ? "border-emerald-500/40 bg-emerald-500/5 text-[var(--color-fg)]"
              : "border-amber-500/40 bg-amber-500/5 text-[var(--color-fg)]")
          }
        >
          {status.message}
        </div>
      )}
    </Section>
  );
}

function BrunoSection() {
  const [defs, setDefs] = useState<BrunoDefaults | null>(null);
  const [saved, setSaved] = useState<BrunoDefaults | null>(null);
  const [savingBruno, setSavingBruno] = useState(false);

  useEffect(() => {
    brunoGetDefaults()
      .then((d) => {
        setDefs(d);
        setSaved(d);
      })
      .catch(() => undefined);
  }, []);

  if (!defs) return null;

  const dirty =
    saved !== null &&
    (saved.tax_class !== defs.tax_class ||
      saved.state !== defs.state ||
      saved.children !== defs.children ||
      saved.is_church_member !== defs.is_church_member ||
      Math.abs(saved.health_add - defs.health_add) > 1e-6);

  const save = async () => {
    if (!defs) return;
    setSavingBruno(true);
    try {
      await brunoSetDefaults(defs);
      setSaved(defs);
    } catch (e) {
      console.error("save bruno defaults", e);
    } finally {
      setSavingBruno(false);
    }
  };

  return (
    <Section
      icon={<Euro size={16} className="text-[var(--color-accent)]" />}
      title="Bruno — Brutto/Netto-Defaults"
      subtitle="Die Werte werden auf jeden `bruno <€>`-Aufruf angewandt. Steuerjahr 2025 (vereinfacht; keine Steuerberatung)."
    >
      <Row label="Steuerklasse">
        <div className="flex gap-1">
          {[1, 2, 3, 4, 5, 6].map((c) => (
            <button
              key={c}
              onClick={() => setDefs({ ...defs, tax_class: c })}
              className={
                "h-7 w-9 rounded text-[12px] font-medium " +
                (defs.tax_class === c
                  ? "bg-[var(--color-accent)] text-[var(--color-accent-fg)]"
                  : "border border-[var(--color-border)] hover:bg-[var(--color-surface)]")
              }
            >
              {c}
            </button>
          ))}
        </div>
      </Row>

      <Row label="Bundesland" help="Beeinflusst den Kirchensteuersatz (BW + BY: 8 %, sonst 9 %).">
        <select
          value={defs.state}
          onChange={(e) => setDefs({ ...defs, state: e.target.value })}
          className="w-full rounded border border-[var(--color-border)] bg-[var(--color-bg)] px-2 py-1 text-[12px]"
        >
          {BRUNO_STATES.map((s) => (
            <option key={s.code} value={s.code}>
              {s.name}
            </option>
          ))}
        </select>
      </Row>

      <Row label="Kinder" help="Beeinflusst Pflegeversicherung (Ermäßigung ab Kind #2) + Kinderfreibetrag.">
        <input
          type="number"
          min={0}
          max={10}
          value={defs.children}
          onChange={(e) =>
            setDefs({ ...defs, children: Math.max(0, parseInt(e.target.value, 10) || 0) })
          }
          className="w-20 rounded border border-[var(--color-border)] bg-[var(--color-bg)] px-2 py-1 text-[12px] tabular-nums"
        />
      </Row>

      <Row label="Kirchensteuerpflichtig">
        <label className="flex cursor-pointer items-center gap-2 text-[12px]">
          <input
            type="checkbox"
            checked={defs.is_church_member}
            onChange={(e) => setDefs({ ...defs, is_church_member: e.target.checked })}
          />
          <span>Ja, Mitglied einer kirchensteuererhebenden Religionsgemeinschaft</span>
        </label>
      </Row>

      <Row
        label="KV-Zusatzbeitrag (%)"
        help="Krankenkassen-Zusatzbeitrag. Durchschnitt 2025 ≈ 2,5 %. TK: 2,45 %, Barmer: 3,29 %."
      >
        <input
          type="number"
          step={0.05}
          min={0}
          max={10}
          value={defs.health_add}
          onChange={(e) =>
            setDefs({ ...defs, health_add: Math.max(0, parseFloat(e.target.value) || 0) })
          }
          className="w-24 rounded border border-[var(--color-border)] bg-[var(--color-bg)] px-2 py-1 text-[12px] tabular-nums"
        />
      </Row>

      <div className="mt-2 flex items-center gap-2">
        <button
          onClick={save}
          disabled={!dirty || savingBruno}
          className="rounded bg-[var(--color-accent)] px-3 py-1 text-[12px] font-medium text-[var(--color-accent-fg)] hover:opacity-90 disabled:opacity-40"
        >
          {savingBruno ? "Speichert…" : "Speichern"}
        </button>
        {dirty && !savingBruno && (
          <span className="text-[11px] text-[var(--color-muted)]">
            Ungespeicherte Änderungen
          </span>
        )}
      </div>
    </Section>
  );
}

// ── Meme library directory ─────────────────────────────────────────────
// Where the `meme [query]` picker scans for GIFs/images. Defaults to a
// home-relative `My Drive/media/memes`; on Windows with Google Drive in
// streaming mode the library lives under a drive letter (e.g.
// `G:\My Drive\media\memes`), so it's overridable here.
function MemeSection() {
  const [dir, setDir] = useState<string | null>(null);
  const [saved, setSaved] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [savedOk, setSavedOk] = useState(false);

  useEffect(() => {
    getMemeDir()
      .then((d) => {
        setDir(d);
        setSaved(d);
      })
      .catch(() => {
        setDir("");
        setSaved("");
      });
  }, []);

  if (dir === null) return null;

  const dirty = saved !== null && saved !== dir;

  const browse = async () => {
    await setSuppressHide(true).catch(() => {});
    try {
      const path = await openDialog({
        multiple: false,
        directory: true,
        title: "Select your meme library folder",
      });
      if (typeof path === "string") setDir(path);
    } catch {
      /* cancelled */
    } finally {
      await setSuppressHide(false).catch(() => {});
    }
  };

  const save = async () => {
    setBusy(true);
    setSavedOk(false);
    try {
      await setMemeDir(dir);
      // Re-read so a blank value reflects the resolved default.
      const effective = await getMemeDir();
      setDir(effective);
      setSaved(effective);
      setSavedOk(true);
    } catch (e) {
      console.error("save meme dir", e);
    } finally {
      setBusy(false);
    }
  };

  return (
    <Section
      icon={<Smile size={16} className="text-[var(--color-accent)]" />}
      title="Meme library"
      subtitle="The folder the `meme [query]` picker scans (recursively) for GIFs/images. On Windows with Google Drive in streaming mode the library is under a drive letter, e.g. G:\\My Drive\\media\\memes — set it here."
    >
      <Row label="Folder">
        <div className="flex w-full items-center gap-2">
          <input
            type="text"
            value={dir}
            spellCheck={false}
            placeholder="(default: ~/My Drive/media/memes)"
            onChange={(e) => {
              setDir(e.target.value);
              setSavedOk(false);
            }}
            className="min-w-0 flex-1 rounded border border-[var(--color-border)] bg-[var(--color-bg)] px-2 py-1 font-mono text-[12px]"
          />
          <button
            onClick={browse}
            className="shrink-0 rounded border border-[var(--color-border)] px-3 py-1 text-[12px] hover:bg-[var(--color-surface)]"
          >
            Browse…
          </button>
        </div>
      </Row>

      <div className="mt-2 flex items-center gap-2">
        <button
          onClick={save}
          disabled={!dirty || busy}
          className="rounded bg-[var(--color-accent)] px-3 py-1 text-[12px] font-medium text-[var(--color-accent-fg)] hover:opacity-90 disabled:opacity-40"
        >
          {busy ? "Saving…" : "Save"}
        </button>
        <button
          onClick={() => {
            setDir("");
            setSavedOk(false);
          }}
          className="rounded border border-[var(--color-border)] px-3 py-1 text-[12px] hover:bg-[var(--color-surface)]"
        >
          Reset to default
        </button>
        {dirty && !busy && (
          <span className="text-[11px] text-[var(--color-muted)]">Unsaved changes</span>
        )}
        {savedOk && !dirty && (
          <span className="text-[11px] text-[var(--color-muted)]">Saved</span>
        )}
      </div>
      <p className="mt-1 text-[11px] text-[var(--color-muted)]">
        Animated previews only render for folders inside the scoped default
        location; a custom folder still lists and copies memes.
      </p>
    </Section>
  );
}
