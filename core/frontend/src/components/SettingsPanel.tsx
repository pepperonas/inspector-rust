import { CellCountField } from "./CellCountField";
import { AiProvidersSection } from "./AiProvidersSection";
import { appVersion as fetchAppVersion, getPulseConfig, setPulseConfig, type PulseConfig } from "../lib/ipc";
import { Fragment, useEffect, useRef, useState } from "react";
import { useTauriEvent } from "../hooks/useTauriEvent";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { open as openDialog, save as saveDialog } from "@tauri-apps/plugin-dialog";
import {
  Activity,
  AlertTriangle,
  Archive,
  ScrollText,
  Cloud,
  HardDrive,
  Camera,
  Clock,
  Lock,
  CheckCircle2,
  ClipboardType,
  Download,
  Euro,
  Gauge,
  GitBranch,
  Info,
  Keyboard,
  Monitor,
  Moon,
  PlayCircle,
  Power,
  Smile,
  Sun,
  Smartphone,
  SunMoon,
  Trash2,
  AlarmClock,
  Hand,
  LayoutGrid,
  Upload,
  Volume2,
  MousePointerClick,
  Wand2,
  Dices,
  ShieldAlert,
  Type,
  Zap,
  CloudSun,
  Music,
  History,
} from "lucide-react";
import { openUrl } from "@tauri-apps/plugin-opener";
import { AboutContent } from "./AboutContent";
import { ChangelogView } from "./ChangelogView";
import { LanguageSection } from "./LanguageSection";
import { InlineMd } from "./InlineMd";
import { locale, useT, tn, tSplit, type MessageKey } from "../lib/i18n";
import { Section, Row } from "./SettingsSection";
import { RecordSettingsSection } from "./RecordSettingsSection";
import { crtOffMs } from "../lib/md3-motion";
import { IS_LINUX, IS_MAC } from "../lib/platform";
import { LinuxShortcutsSettings } from "./LinuxShortcutsSettings";
import {
  diagnoseExpandAtCursor,
  forceResetAndRequestGrant,
  brunoGetDefaults,
  fakerGetDefaults,
  secGetDefaults,
  secSetDefaults,
  secCatalog,
  secPathExists,
  fakerSetDefaults,
  fakerLocales,
  fakerCatalog,
  figletGetDefaults,
  figletSetDefaults,
  figletFonts,
  type FakerLocaleOption,
  brunoSetDefaults,
  getMemeDir,
  setMemeDir,
  getWeatherConfig,
  getSpotifyConfig,
  setSpotifyConfig,
  spotifyHistoryClear,
  type SpotifyConfig,
  setWeatherConfig,
  type WeatherConfig,
  getDbSpace,
  getHistoryMax,
  compactDb,
  setHistoryMax,
  type HistoryLimit,
  getAnimationStage,
  getCrtAnimation,
  setCrtAnimation,
  type CrtAnimation,
  adbStatus,
  type AdbStatus,
  getSnippetStorage,
  type SnippetStorage,
  getLineageHighlight,
  setLineageHighlight,
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
  getCursorWrapConfig,
  setCursorWrapConfig,
  type CursorWrapConfig,
  getWindowPaletteConfig,
  setWindowPaletteConfig,
  type WindowPaletteConfig,
  type PaletteTrigger,
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
  getScreenshotSound,
  setScreenshotSound,
  type ScreenshotSoundStyle,
  setSoundEnabled,
  getAlarmStyle,
  setAlarmStyle,
  type AlarmStyle,
  getThemePreference,
  getClipboardPrivacy,
  setClipboardPrivacy,
  type ClipboardPrivacy,
  importBackup,
  isBackupEncrypted,
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
  setAnimationStage,
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
import type { DbSpace } from "../lib/ipc";
import {
  githubDeviceCancel,
  githubDeviceStart,
  githubDisconnect,
  githubRefresh,
  githubStatus,
  setGithubConfig,
  type GithubConfig,
  type GithubStatus,
} from "../lib/ipc";
import {
  getSyncConfig,
  getPagespeedKey,
  getRepoConfig,
  setRepoCloneDir,
  setGithubToken,
  type RepoConfig,
  setPagespeedKey,
  getAutoBackupConfig,
  setAutoBackupConfig,
  getAutoBackupStatus,
  setAutoBackupPassword,
  autoBackupListSnapshots,
  autoBackupNow,
  autoBackupRestore,
  getDeviceSyncConfig,
  setDeviceSyncConfig,
  getDeviceSyncStatus,
  setDeviceSyncPassphrase,
  deviceSyncNow,
  type AutoBackupConfig,
  type AutoBackupStatus,
  type AutoBackupSnapshot,
  type DeviceSyncConfig,
  type DeviceSyncStatus,
  getPopupCloseOnBlur,
  setPopupCloseOnBlur,
  syncTestConnection,
  getSyncStatus,
  setSyncConfig,
  syncNow,
  type SyncConfig,
  type SyncProbe,
  type SyncStatus,
} from "../lib/ipc";
import type { FakerDefaults } from "../lib/faker";
import type { FigletDefaults, FigletAlign, FigletComment, FigletFontMeta } from "../lib/figlet";
import type { SecDefaults } from "../lib/sec";
import { applyThemeAnimated, normaliseTheme, type ThemePreference } from "../lib/theme";
import {
  normaliseAnimationStage,
  stageLabel,
  type AnimationStage,
} from "../lib/motion-stage";
import { MEME_ENABLED } from "../lib/meme";
import { SETTINGS_CATEGORIES } from "../lib/settings-sections";
import type { BackupImportResult, Snippet } from "../lib/types";
import { formatBytes } from "../lib/format";
import { confirmDialog } from "../lib/confirm";
import { HotkeyCapture } from "./HotkeyCapture";
import { GlobalShortcutsSection } from "./GlobalShortcutsSection";
import { GestureBindingsSection } from "./GestureBindingsSection";

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
  /** `settings <section>` deep-link: scroll to + highlight this section. */
  jumpTo?: { id: string; nonce: number } | null;
}

export function SettingsPanel({ onBackupImported, jumpTo }: Props = {}) {
  const t = useT();
  const scrollRef = useRef<HTMLDivElement>(null);
  // `settings <section>` deep-link: scroll the target section into view and
  // flash a highlight ring. Re-triggers on every jump via the nonce.
  useEffect(() => {
    if (!jumpTo) return;
    const el = document.getElementById(`settings-${jumpTo.id}`);
    if (!el) return;
    // The jump fires on mount, while the sections above still load their
    // state — they grow afterwards and push the target down (a deep link
    // to "sync" stopped in the window palette). So jump instantly and keep
    // the target pinned while the layout settles, until the user scrolls.
    const root = scrollRef.current;
    const pin = () => el.scrollIntoView({ behavior: "auto", block: "start" });
    pin();
    let pinning = true;
    const release = () => {
      pinning = false;
    };
    const ro = typeof ResizeObserver === "undefined" ? null : new ResizeObserver(() => {
      if (pinning) pin();
    });
    if (root) {
      for (const child of root.children) ro?.observe(child);
    }
    const userInput = ["wheel", "touchstart", "pointerdown", "keydown"] as const;
    for (const ev of userInput) root?.addEventListener(ev, release, { passive: true });
    const stop = window.setTimeout(release, 1200);
    el.classList.add("settings-jump-highlight");
    const flash = window.setTimeout(() => el.classList.remove("settings-jump-highlight"), 1800);
    return () => {
      window.clearTimeout(flash);
      window.clearTimeout(stop);
      ro?.disconnect();
      for (const ev of userInput) root?.removeEventListener(ev, release);
    };
  }, [jumpTo]);

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
    { kind: "ok"; message: string } | { kind: "err"; message: string } | null
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
  const [inputLockStatus, setInputLockStatus] = useState<{
    kind: "ok" | "err";
    message: string;
  } | null>(null);

  // ── Direct hotkey → snippet slots ───────────────────────────────────────
  const [snippetList, setSnippetList] = useState<Snippet[]>([]);
  const [slots, setSlots] = useState<DirectSlot[]>([]);
  const [savedSlots, setSavedSlots] = useState<DirectSlot[]>([]);
  const [slotsBusy, setSlotsBusy] = useState(false);
  const [slotsStatus, setSlotsStatus] = useState<{
    kind: "ok" | "err";
    message: string;
  } | null>(null);
  useEffect(() => {
    listSnippets()
      .then(setSnippetList)
      .catch(() => undefined);
    getDirectSlots()
      .then((s) => {
        setSlots(s);
        setSavedSlots(s);
      })
      .catch(() => undefined);
  }, []);
  const slotKey = (s: { hotkey: string; snippet_id: number }) =>
    `${s.hotkey}\u0000${s.snippet_id}`;
  const slotsDirty = slots.map(slotKey).join("|") !== savedSlots.map(slotKey).join("|");
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
  const removeSlot = (i: number) => setSlots((cur) => cur.filter((_, j) => j !== i));
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
            ? t("set.slots.none")
            : tn("set.slots.registered", applied.length),
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
  useTauriEvent<boolean>("autostart-changed", (e) => setAutostart(e.payload));
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
  const patchSnap = async (patch: Partial<WindowSnapConfig>) => {
    if (!snapCfg) return;
    setSnapBusy(true);
    const previous = snapCfg;
    const optimistic = { ...snapCfg, ...patch };
    setSnapCfg(optimistic);
    try {
      setSnapCfg(await setWindowSnapConfig(optimistic));
    } catch (e) {
      console.error("window-snap config failed", e);
      setSnapCfg(previous);
    } finally {
      setSnapBusy(false);
    }
  };
  const toggleSnap = (next: boolean) => patchSnap({ enabled: next });

  // ── Window palette (Moom-style, macOS, v0.84.138) ────────────────────────
  const [paletteCfg, setPaletteCfg] = useState<WindowPaletteConfig | null>(null);
  const [paletteBusy, setPaletteBusy] = useState(false);
  useEffect(() => {
    if (!IS_MAC) return;
    getWindowPaletteConfig()
      .then(setPaletteCfg)
      .catch(() => setPaletteCfg(null));
  }, []);
  // The LATEST config, not the one this render closed over: the density
  // fields save columns and rows independently, and a second save that
  // started from a stale copy would silently undo the first.
  const paletteRef = useRef<WindowPaletteConfig | null>(null);
  useEffect(() => {
    paletteRef.current = paletteCfg;
  }, [paletteCfg]);
  const updatePalette = async (patch: Partial<WindowPaletteConfig>) => {
    const base = paletteRef.current;
    if (!base) return;
    setPaletteBusy(true);
    const next = { ...base, ...patch };
    paletteRef.current = next;
    setPaletteCfg(next);
    try {
      const saved = await setWindowPaletteConfig(next);
      // A newer edit may have started meanwhile — don't overwrite it.
      if (paletteRef.current === next) {
        paletteRef.current = saved;
        setPaletteCfg(saved);
      }
    } catch (e) {
      console.error("window-palette config failed", e);
      if (paletteRef.current === next) {
        paletteRef.current = base;
        setPaletteCfg(base);
      }
    } finally {
      setPaletteBusy(false);
    }
  };

  // ── Cursor wrap-around ("Infinity Monitor", macOS) ───────────────────────
  const [wrapCfg, setWrapCfg] = useState<CursorWrapConfig | null>(null);
  const [wrapBusy, setWrapBusy] = useState(false);
  useEffect(() => {
    if (!IS_MAC) return;
    getCursorWrapConfig()
      .then(setWrapCfg)
      .catch(() => setWrapCfg(null));
  }, []);
  const patchWrap = async (patch: Partial<CursorWrapConfig>) => {
    if (!wrapCfg) return;
    setWrapBusy(true);
    const previous = wrapCfg;
    const optimistic = { ...wrapCfg, ...patch };
    setWrapCfg(optimistic);
    try {
      setWrapCfg(await setCursorWrapConfig(optimistic));
    } catch (e) {
      console.error("cursor-wrap config failed", e);
      setWrapCfg(previous);
    } finally {
      setWrapBusy(false);
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
  // Screenshot shutter style (v0.111.0) — the backend previews on change.
  const [shotSound, setShotSound] = useState<ScreenshotSoundStyle | null>(null);
  useEffect(() => {
    getScreenshotSound()
      .then(setShotSound)
      .catch(() => setShotSound("snap"));
  }, []);
  const changeShotSound = async (next: ScreenshotSoundStyle) => {
    const previous = shotSound;
    setShotSound(next);
    try {
      const applied = await setScreenshotSound(next);
      setShotSound(applied);
    } catch (e) {
      console.error("screenshot sound change failed", e);
      setShotSound(previous);
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
  const patchGestures = async (patch: Partial<GestureConfig>) => {
    if (!gestureCfg) return;
    setGestureBusy(true);
    const previous = gestureCfg;
    const optimistic = { ...gestureCfg, ...patch };
    setGestureCfg(optimistic); // optimistic — the IPC re-applies the source
    try {
      const applied = await setGestureConfig(optimistic);
      setGestureCfg(applied);
    } catch (e) {
      console.error("gesture config change failed", e);
      setGestureCfg(previous);
    } finally {
      setGestureBusy(false);
    }
  };
  const toggleGestures = (next: boolean) => patchGestures({ enabled: next });

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
    // Animated (Etappe 6): colours glide via the one-shot theme-switching
    // blanket transition; the mount-time apply in App.tsx stays instant.
    applyThemeAnimated(next);
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

  // ── Animation stage (Full / Reduced / Off, v0.163.0) ─────────────────────
  // Persisting broadcasts `animation-stage-changed`; every window's
  // motion-stage runtime (initMotion in main.tsx) applies the root class
  // live, so no re-apply is needed here.
  const [animStage, setAnimStage] = useState<AnimationStage | null>(null);
  const [animStageBusy, setAnimStageBusy] = useState(false);
  useEffect(() => {
    getAnimationStage()
      .then((s) => setAnimStage(normaliseAnimationStage(s)))
      .catch(() => setAnimStage("full"));
  }, []);
  const changeAnimStage = async (next: AnimationStage) => {
    setAnimStage(next); // optimistic — the picker must feel instant
    setAnimStageBusy(true);
    try {
      await setAnimationStage(next);
    } catch (e) {
      console.error("animation stage persist failed", e);
    } finally {
      setAnimStageBusy(false);
    }
  };

  // ── Popup overlay size ───────────────────────────────────────────────────
  // `winSize` is null until the persisted preset loads; the segmented control
  // disables itself in that window. Changing it resizes the live window on the
  // backend (next show recentres it).
  const [winSize, setWinSize] = useState<string | null>(null);
  const [winSizeBusy, setWinSizeBusy] = useState(false);
  // Lineage rails in the history list (v0.93.1) — purely visual.
  const [lineageRails, setLineageRails] = useState(true);
  useEffect(() => {
    void getLineageHighlight().then(setLineageRails).catch(() => undefined);
  }, []);
  const changeLineageRails = async (value: boolean) => {
    setLineageRails(value); // optimistic — the toggle must feel instant
    try {
      await setLineageHighlight(value);
    } catch {
      setLineageRails(!value);
    }
  };
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
    fetchAppVersion()
      .then(setAppVersion)
      .catch(() => undefined);
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
        message: t("set.inputlock.saved", { chord: keys.join(" + ") }),
      });
    } catch (e) {
      setInputLockStatus({ kind: "err", message: String(e) });
    } finally {
      setInputLockSaving(false);
    }
  };

  // ── Backup section state ────────────────────────────────────────────────
  // Export always covers the whole app (history, snippets, notes, 2FA,
  // settings); the timesheet is the one opt-in extra (large, machine-bound).
  const [includeTimesheet, setIncludeTimesheet] = useState(false);
  const [encryptBackup, setEncryptBackup] = useState(false);
  const [backupPassword, setBackupPassword] = useState("");
  const [backupPassword2, setBackupPassword2] = useState("");
  const [showBackupPassword, setShowBackupPassword] = useState(false);
  // A picked import file that turned out to be encrypted — we hold it here
  // and ask for the password inline before actually importing.
  const [pendingImport, setPendingImport] = useState<string | null>(null);
  const [importPassword, setImportPassword] = useState("");
  const [backupBusy, setBackupBusy] = useState<"export" | "import" | null>(null);
  const [backupStatus, setBackupStatus] = useState<
    | { kind: "ok"; message: string }
    | { kind: "import-ok"; result: BackupImportResult }
    | { kind: "err"; message: string }
    | null
  >(null);

  const onExport = async () => {
    if (encryptBackup) {
      if (backupPassword.length < 4) {
        setBackupStatus({
          kind: "err",
          message: t("set.backup.pw_short"),
        });
        return;
      }
      if (backupPassword !== backupPassword2) {
        setBackupStatus({ kind: "err", message: t("set.backup.pw_mismatch") });
        return;
      }
    }
    setBackupStatus(null);
    setBackupBusy("export");
    await setSuppressHide(true).catch(() => {});
    try {
      const stamp = new Date().toISOString().replace(/[:.]/g, "-").slice(0, 19);
      const path = await saveDialog({
        title: t("set.backup.save_title"),
        defaultPath: `inspector-rust-backup-${stamp}.json`,
        filters: [{ name: "JSON", extensions: ["json"] }],
      });
      if (!path) return;
      const bytes = await saveBackupToFile(path, {
        includeTimesheet,
        password: encryptBackup ? backupPassword : undefined,
      });
      const filename = path.split("/").pop() ?? path;
      setBackupStatus({
        kind: "ok",
        message: t(encryptBackup ? "set.backup.exported_enc" : "set.backup.exported", { size: formatBytes(bytes), file: filename }),
      });
    } catch (e) {
      setBackupStatus({ kind: "err", message: String(e) });
    } finally {
      await setSuppressHide(false).catch(() => {});
      setBackupBusy(null);
    }
  };

  const runImport = async (path: string, password?: string) => {
    setBackupBusy("import");
    try {
      const result = await importBackup(path, password);
      setBackupStatus({ kind: "import-ok", result });
      setPendingImport(null);
      setImportPassword("");
      if (onBackupImported) await onBackupImported();
    } catch (e) {
      // Wrong password / corrupt file — keep the pending state so the user
      // can correct the password without re-picking the file.
      setBackupStatus({ kind: "err", message: String(e) });
    } finally {
      setBackupBusy(null);
    }
  };

  const onImport = async () => {
    setBackupStatus(null);
    setPendingImport(null);
    setImportPassword("");
    await setSuppressHide(true).catch(() => {});
    try {
      const path = await openDialog({
        multiple: false,
        directory: false,
        filters: [{ name: "JSON", extensions: ["json"] }],
        title: t("set.backup.open_title"),
      });
      if (!path) return;
      if (await isBackupEncrypted(path)) {
        // Ask for the password inline; the actual import runs on "Unlock".
        setPendingImport(path);
        return;
      }
      await runImport(path);
    } catch (e) {
      setBackupStatus({ kind: "err", message: String(e) });
    } finally {
      await setSuppressHide(false).catch(() => {});
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
    // Backoff, not a fixed 1 s beat (v0.105.0): this probe is NOT a cheap
    // flag read — it runs a real AppleScript against Finder (subprocess +
    // Apple Event, 2 s watchdog). A user who declined the Automation grant
    // used to fork an osascript every second for as long as the Settings tab
    // was open. 2 s → ×1.5 → capped at 30 s; the panel remounts per tab
    // switch, so re-opening Settings starts fresh at 2 s again.
    let alive = true;
    let delay = 2000;
    let id = 0;
    const probe = async () => {
      try {
        const ok = await getFinderAutomationStatus();
        if (!alive) return;
        if (ok) {
          setFinderAutomation(true);
          return;
        }
      } catch {
        /* ignore — keep polling */
      }
      if (!alive) return;
      delay = Math.min(30_000, delay * 1.5);
      id = window.setTimeout(() => void probe(), delay);
    };
    id = window.setTimeout(() => void probe(), delay);
    return () => {
      alive = false;
      window.clearTimeout(id);
    };
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

  const dirty = cfg !== null && (cfg.enabled !== enabled || cfg.hotkey !== hotkey);

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
          ? t("set.expander.armed", { hotkey: prettyHotkey(applied.hotkey) })
          : t("set.expander.disabled"),
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

  // Every settings block by section id. The ORDER on screen comes from
  // SETTINGS_CATEGORIES (lib/settings-sections.ts) — the one place it lives.
  const blocks: Record<string, React.ReactNode> = {
        /* Popup behavior — the very first thing: how the overlay dismisses. */
    "behavior": (
        <div className="mb-6">
          <PopupBehaviorSection />
        </div>
    ),
        /* Sound — master toggle for UI feedback cues. At the top so it's
            easy to silence the app. */
    "sounds": (
        <div className="mb-6">
          <Section
            icon={<Volume2 size={16} className="text-[var(--color-accent)]" />}
            id="sounds"
      title={t("settings.section.sounds")}
            subtitle={t("set.sounds.subtitle")}
          >
            <Row label={t("set.sounds.feedback")}>
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
                    ? t("common.loading")
                    : soundEnabled
                      ? t("set.sounds.on")
                      : t("set.sounds.off")}
                </span>
              </label>
            </Row>
            <Row label={t("set.sounds.shot")}>
              <div className="flex flex-col gap-1 text-[12px]">
                <select
                  value={shotSound ?? "camera"}
                  disabled={shotSound === null}
                  onChange={(e) => void changeShotSound(e.target.value as ScreenshotSoundStyle)}
                  className="w-fit rounded-md border border-[var(--color-border)] bg-[var(--color-surface)] px-2 py-1 text-[12px] text-[var(--color-fg)]"
                >
                  <option value="camera">{t("set.sounds.camera")}</option>
                  <option value="snap">{t("set.sounds.snap")}</option>
                  <option value="dslr">{t("set.sounds.dslr")}</option>
                  <option value="switch">{t("set.sounds.switch")}</option>
                  <option value="off">{t("set.sounds.shot_off")}</option>
                </select>
                <span className="text-[var(--color-muted)]">
                  {t("set.sounds.preview")}
                </span>
              </div>
            </Row>
          </Section>
        </div>
    ),
        /* Touchpad gestures — BetterTouchTool-style 3-finger swipe/tap. */
    "gestures": (
        <div className="mb-6">
          <Section
            icon={<Hand size={16} className="text-[var(--color-accent)]" />}
            id="gestures"
      title={t("settings.section.gestures")}
            subtitle={t("set.gestures.subtitle")}
          >
            <Row label={t("set.gestures.enable")}>
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
                    ? t("common.loading")
                    : gestureCfg.enabled
                      ? t("set.gestures.on")
                      : t("common.off")}
                </span>
              </label>
            </Row>
            {gestureCfg?.enabled && (
              <>
                <Row label={t("set.gestures.bindings")}>
                  <p className="mb-2 text-[11px] text-[var(--color-muted)]">
                    {t("set.gestures.bindings_help")}
                  </p>
                  <GestureBindingsSection />
                </Row>
                <Row label={t("set.gestures.typing_guard")}>
                  <label className="flex cursor-pointer items-center gap-2 text-[12px]">
                    <input
                      type="checkbox"
                      checked={gestureCfg?.typing_guard ?? true}
                      disabled={gestureBusy}
                      onChange={(e) => void patchGestures({ typing_guard: e.target.checked })}
                      className="accent-[var(--color-accent)]"
                    />
                    <span className="text-[var(--color-muted)]">
                      {t("set.gestures.typing_guard_help")}
                    </span>
                  </label>
                </Row>
                <Row label={t("set.gestures.live")}>
                  <span className="text-[var(--color-muted)]">
                    <InlineMd text={t("set.gestures.live_help")} />
                  </span>
                </Row>
              </>
            )}
          </Section>
        </div>
    ),
        /* Window snapping — drag a window to a screen edge to snap it (macOS). */
    "window": (
        IS_MAC && (
          <div className="mb-6">
            <Section
              icon={<LayoutGrid size={16} className="text-[var(--color-accent)]" />}
              id="window"
      title={t("set.snap.title")}
              subtitle={t("set.snap.subtitle")}
            >
              <Row label={t("set.snap.enable")}>
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
                      ? t("common.loading")
                      : snapCfg.enabled
                        ? t("set.snap.on")
                        : t("common.off")}
                  </span>
                </label>
              </Row>
              {snapCfg?.enabled && (
                <Row label={t("set.snap.delay")}>
                  <div className="flex flex-col gap-1.5 text-[12px]">
                    <select
                      value={String(snapCfg.dwell_ms)}
                      disabled={snapBusy}
                      onChange={(e) => void patchSnap({ dwell_ms: Number(e.target.value) })}
                      className="w-56 rounded border border-[var(--color-border)] bg-[var(--color-surface)] px-2 py-1 text-[12px] text-[var(--color-fg)]"
                    >
                      <option value="0">{t("set.snap.instant")}</option>
                      <option value="200">200 ms</option>
                      <option value="350">{t("set.snap.default_ms")}</option>
                      <option value="500">500 ms</option>
                      <option value="800">800 ms</option>
                    </select>
                    <span className="text-[var(--color-muted)]">
                      {t("set.snap.delay_help")}
                    </span>
                  </div>
                </Row>
              )}
            </Section>
          </div>
        )
    ),
        /* Cursor wrap-around — pointer reappears on the opposite outer edge (macOS). */
    "cursor-wrap": (
        IS_MAC && (
          <div className="mb-6">
            <Section
              icon={<MousePointerClick size={16} className="text-[var(--color-accent)]" />}
              id="cursor-wrap"
              title={t("settings.section.cursor-wrap")}
              subtitle={t("set.wrap.subtitle")}
            >
              <Row label={t("set.wrap.enable")}>
                <label className="flex cursor-pointer items-center gap-2 text-[12px]">
                  <input
                    type="checkbox"
                    checked={wrapCfg?.enabled ?? false}
                    disabled={wrapCfg === null || wrapBusy}
                    onChange={(e) => void patchWrap({ enabled: e.target.checked })}
                    className="accent-[var(--color-accent)]"
                  />
                  <span className="text-[var(--color-muted)]">
                    {wrapCfg === null
                      ? t("common.loading")
                      : wrapCfg.enabled
                        ? t("set.wrap.on")
                        : t("common.off")}
                  </span>
                </label>
              </Row>
              {wrapCfg?.enabled && (
                <>
                  <Row label={t("set.wrap.edges")}>
                    <div className="flex flex-wrap items-center gap-x-4 gap-y-1.5 text-[12px]">
                      {(
                        [
                          ["left", t("set.wrap.left")],
                          ["right", t("set.wrap.right")],
                          ["top", t("set.wrap.top")],
                          ["bottom", t("set.wrap.bottom")],
                        ] as const
                      ).map(([key, label]) => (
                        <label key={key} className="flex cursor-pointer items-center gap-1.5">
                          <input
                            type="checkbox"
                            checked={wrapCfg[key]}
                            disabled={wrapBusy}
                            onChange={(e) => void patchWrap({ [key]: e.target.checked })}
                            className="accent-[var(--color-accent)]"
                          />
                          <span className="text-[var(--color-muted)]">{label}</span>
                        </label>
                      ))}
                    </div>
                  </Row>
                  <Row label={t("set.wrap.deadzone")}>
                    <div className="flex flex-col gap-1.5 text-[12px]">
                      <select
                        value={String(wrapCfg.corner_deadzone_px)}
                        disabled={wrapBusy}
                        onChange={(e) =>
                          void patchWrap({ corner_deadzone_px: Number(e.target.value) })
                        }
                        className="w-56 rounded border border-[var(--color-border)] bg-[var(--color-surface)] px-2 py-1 text-[12px] text-[var(--color-fg)]"
                      >
                        <option value="0">{t("set.wrap.deadzone_off")}</option>
                        <option value="8">{t("set.wrap.deadzone_default")}</option>
                        <option value="16">16 px</option>
                        <option value="24">24 px</option>
                        <option value="40">40 px</option>
                      </select>
                      <span className="text-[var(--color-muted)]">
                        {t("set.wrap.deadzone_help")}
                      </span>
                    </div>
                  </Row>
                  <Row label={t("set.wrap.drag")}>
                    <label className="flex cursor-pointer items-center gap-2 text-[12px]">
                      <input
                        type="checkbox"
                        checked={wrapCfg.wrap_during_drag}
                        disabled={wrapBusy}
                        onChange={(e) => void patchWrap({ wrap_during_drag: e.target.checked })}
                        className="accent-[var(--color-accent)]"
                      />
                      <span className="text-[var(--color-muted)]">
                        {wrapCfg.wrap_during_drag
                          ? t("set.wrap.drag_on")
                          : t("set.wrap.drag_off")}
                      </span>
                    </label>
                  </Row>
                </>
              )}
            </Section>
          </div>
        )
    ),
        /* Window palette — Moom-style hover palette over the green zoom button. */
    "window-palette": (
        IS_MAC && (
          <div className="mb-6">
            <Section
              icon={<LayoutGrid size={16} className="text-[var(--color-accent)]" />}
              id="window-palette"
              title={t("set.palette.title")}
              subtitle={t("set.palette.subtitle")}
            >
              <Row label={t("set.palette.enable")}>
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
                      ? t("common.loading")
                      : paletteCfg.enabled
                        ? t("common.on")
                        : t("common.off")}
                  </span>
                </label>
              </Row>
              {paletteCfg?.enabled && (
                <Row label={t("set.palette.summon")}>
                  <div className="flex flex-col gap-1.5 text-[12px]">
                    <select
                      value={paletteCfg.trigger}
                      disabled={paletteBusy}
                      onChange={(e) =>
                        void updatePalette({ trigger: e.target.value as PaletteTrigger })
                      }
                      className="rounded-md border border-[var(--color-border)] bg-[var(--color-bg)] px-1.5 py-1 text-[var(--color-fg)]"
                      aria-label={t("set.palette.trigger")}
                    >
                      <option value="zoom_hover">{t("set.palette.zoom_hover")}</option>
                      <option value="titlebar_modifier">{t("set.palette.titlebar")}</option>
                      <option value="hotkey">{t("set.palette.hotkey")}</option>
                    </select>
                    <span className="text-[var(--color-muted)]">
                      {paletteCfg.trigger === "titlebar_modifier"
                        ? t("set.palette.help_titlebar")
                        : paletteCfg.trigger === "hotkey"
                          ? t("set.palette.help_hotkey")
                          : paletteCfg.hide_system_menu
                            ? t("set.palette.help_hidden")
                            : t("set.palette.help_beside")}
                    </span>
                    {paletteCfg.trigger === "zoom_hover" && (
                      <label className="flex items-center gap-2">
                        <input
                          type="checkbox"
                          checked={paletteCfg.hide_system_menu}
                          disabled={paletteBusy}
                          onChange={(e) =>
                            void updatePalette({ hide_system_menu: e.target.checked })
                          }
                          className="accent-[var(--color-accent)]"
                        />
                        <span className="text-[var(--color-fg)]">
                          {t("set.palette.hide_menu")}
                        </span>
                      </label>
                    )}
                    <span className="text-[var(--color-muted)]">
                      {t("set.palette.shortcut_always")}
                    </span>
                  </div>
                </Row>
              )}
              {paletteCfg?.enabled && (
                <Row label={t("set.palette.density")}>
                  <div className="flex items-center gap-1.5 text-[12px]">
                    <CellCountField
                      value={paletteCfg.cols}
                      onCommit={(cols) => void updatePalette({ cols })}
                      label={t("set.palette.cols")}
                    />
                    <span className="text-[var(--color-muted)]">×</span>
                    <CellCountField
                      value={paletteCfg.rows}
                      onCommit={(rows) => void updatePalette({ rows })}
                      label={t("set.palette.rows")}
                    />
                    <span className="text-[var(--color-muted)]">{t("set.palette.cells")}</span>
                  </div>
                </Row>
              )}
            </Section>
          </div>
        )
    ),
        /* Timer alarm — overlay (loud, dismiss-to-stop) vs OS notification. */
    "timer-alarm": (
        <div className="mb-6">
          <Section
            icon={<AlarmClock size={16} className="text-[var(--color-accent)]" />}
            id="timer-alarm"
      title={t("settings.section.timer-alarm")}
            subtitle={t("set.alarm.subtitle")}
          >
            <Row label={t("set.alarm.style")}>
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
                    <span className="text-[var(--color-fg)]">{t("set.alarm.overlay")}</span>{" "}
                    {t("set.alarm.overlay_help")}
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
                    <span className="text-[var(--color-fg)]">{t("set.alarm.notification")}</span>{" "}
                    {t("set.alarm.notification_help")}
                  </span>
                </label>
              </div>
            </Row>
          </Section>
        </div>
    ),
        /* Global action shortcuts — rebindable OCR / screenshot / timesheet / … */
    "global-shortcuts": (
        <div className="mb-6">
          <Section
            icon={<Keyboard size={16} className="text-[var(--color-accent)]" />}
            id="global-shortcuts"
      title={t("settings.section.global-shortcuts")}
            subtitle={t("set.shortcuts.subtitle")}
          >
            <GlobalShortcutsSection />
          </Section>
        </div>
    ),
        /* Text expander section */
    "expander": (
        <div className="mb-6">
          <Section
            icon={<Wand2 size={16} className="text-[var(--color-accent)]" />}
            id="expander"
      title={t("settings.section.expander")}
            subtitle={t("set.expander.subtitle")}
          >
            {/* Granted state — shown inline since it's not actionable.
              Special case: if we *just* detected the false→true edge
              while polling, prompt for a restart, because the running
              process can't see the new grant until macOS re-evaluates
              AXIsProcessTrusted on a fresh launch. */}
            {accessibility === true && justGranted && (
              <div className="confirm-enter mb-4 flex flex-col gap-2 rounded border border-emerald-500/40 bg-emerald-500/5 px-3 py-2.5 text-[12px]">
                <div className="flex items-start gap-2">
                  <CheckCircle2 size={14} className="mt-0.5 shrink-0 text-emerald-500" />
                  <div className="flex-1">
                    <div className="font-medium">{t("set.ax.detected")}</div>
                    <div className="mt-0.5 text-[var(--color-muted)]">
                      {t("set.ax.detected_help")}
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
                    {restarting ? t("set.ax.restarting") : t("set.ax.restart")}
                  </button>
                  <button
                    onClick={() => setJustGranted(false)}
                    className="rounded border border-[var(--color-border)] px-2.5 py-1 text-[11px] text-[var(--color-muted)] hover:border-[var(--color-accent)] hover:text-[var(--color-accent)]"
                    title={t("set.ax.later_title")}
                  >
                    {t("set.ax.later")}
                  </button>
                </div>
              </div>
            )}
            {accessibility === true && !justGranted && (
              <div className="confirm-enter mb-4 flex items-center gap-2 rounded border border-emerald-500/30 bg-emerald-500/5 px-3 py-2 text-[12px]">
                <CheckCircle2 size={14} className="shrink-0 text-emerald-500" />
                <span className="font-medium">{t("set.ax.granted")}</span>
              </div>
            )}

            <Row label={t("set.expander.enable")}>
              <label className="flex cursor-pointer items-center gap-2 text-[12px]">
                <input
                  type="checkbox"
                  checked={enabled}
                  onChange={(e) => setEnabled(e.target.checked)}
                  className="accent-[var(--color-accent)]"
                />
                <span className="text-[var(--color-muted)]">
                  {enabled ? t("set.expander.registered") : t("set.expander.unregistered")}
                </span>
              </label>
            </Row>

            <Row
              label={t("set.expander.hotkey")}
              help={t("set.expander.hotkey_help")}
            >
              <div className="flex flex-wrap items-center gap-2">
                <HotkeyCapture value={hotkey} onChange={setHotkey} disabled={busy} />
                <button
                  onClick={reset}
                  disabled={busy || hotkey === DEFAULT_HOTKEY}
                  className="rounded border border-[var(--color-border)] px-2 py-1 text-[11px] text-[var(--color-muted)] hover:border-[var(--color-accent)] hover:text-[var(--color-fg)] disabled:opacity-40"
                  title={t("set.reset_to", { value: prettyHotkey(DEFAULT_HOTKEY) })}
                >
                  {t("common.reset")}
                </button>
                <span className="text-[11px] text-[var(--color-muted)]">{t("set.expander.presets")}</span>
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
                      title={t("set.expander.use", { label: q.label })}
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
                {busy ? t("common.saving") : dirty ? t("set.expander.save") : t("common.no_changes")}
              </button>
              {status && (
                <span
                  className={
                    "text-[11px] " +
                    (status.kind === "ok" ? "text-[var(--color-muted)]" : "text-red-400")
                  }
                >
                  {status.message}
                </span>
              )}
            </div>

            <div className="mt-3 flex flex-col gap-2 rounded border border-dashed border-[var(--color-border)] p-3">
              <div className="flex items-start gap-3">
                <PlayCircle
                  size={14}
                  className="mt-0.5 shrink-0 text-[var(--color-accent)]"
                />
                <div className="flex-1 text-[11px] text-[var(--color-muted)]">
                  <div className="text-[var(--color-fg)]">
                    {t("set.diag.title")}
                  </div>
                  <p className="mt-0.5">
                    <InlineMd text={t("set.diag.help")} />
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
                  {diagnose?.kind === "running" ? t("set.diag.capturing") : t("set.diag.button")}
                </button>
              </div>

              {diagnose?.kind === "result" && (
                <div className="rounded border border-[var(--color-border)] bg-[var(--color-surface)] p-2 text-[11px]">
                  <div className="grid grid-cols-[110px_1fr] gap-x-3 gap-y-1">
                    <span className="text-[var(--color-muted)]">{t("set.diag.path")}</span>
                    <span>
                      {diagnose.data.path === "ax" ? (
                        <span className="rounded bg-emerald-500/10 px-1.5 py-0.5 font-medium text-emerald-400">
                          {t("set.diag.ax")}
                        </span>
                      ) : diagnose.data.path === "uia" ? (
                        <span className="rounded bg-emerald-500/10 px-1.5 py-0.5 font-medium text-emerald-400">
                          {t("set.diag.uia")}
                        </span>
                      ) : (
                        <span className="rounded bg-amber-500/10 px-1.5 py-0.5 font-medium text-amber-400">
                          {t("set.diag.fallback")}
                        </span>
                      )}
                    </span>
                    <span className="text-[var(--color-muted)]">{t("set.diag.captured")}</span>
                    <code className="rounded bg-[var(--color-bg)] px-1 font-[var(--font-mono)]">
                      {diagnose.data.captured || t("list.empty_clip")}
                    </code>
                    <span className="text-[var(--color-muted)]">{t("set.diag.match")}</span>
                    <span>
                      {diagnose.data.matched_abbreviation ? (
                        <code className="rounded bg-[var(--color-bg)] px-1 font-[var(--font-mono)] text-emerald-400">
                          {diagnose.data.matched_abbreviation}
                        </code>
                      ) : (
                        <span className="text-amber-400">
                          {t("set.diag.no_match")}
                        </span>
                      )}
                    </span>
                    {diagnose.data.paste_preview && (
                      <>
                        <span className="text-[var(--color-muted)]">{t("set.diag.would_paste")}</span>
                        <code className="block truncate rounded bg-[var(--color-bg)] px-1 font-[var(--font-mono)]">
                          {diagnose.data.paste_preview}
                        </code>
                      </>
                    )}
                  </div>
                  {!diagnose.data.captured && (
                    <p className="mt-2 text-[var(--color-muted)]">
                      {t("set.diag.empty_help")}
                    </p>
                  )}
                  {diagnose.data.captured && !diagnose.data.matched_abbreviation && (
                    <p className="mt-2 text-[var(--color-muted)]">
                      <InlineMd text={t("set.diag.nomatch_help", { abbr: "`" + diagnose.data.captured + "`" })} />
                    </p>
                  )}
                </div>
              )}
              {diagnose?.kind === "err" && (
                <div className="text-[11px] text-red-400">{diagnose.message}</div>
              )}
            </div>

            <details className="mt-5 rounded border border-[var(--color-border)] bg-[var(--color-surface)] p-3 text-[11px] text-[var(--color-muted)]">
              <summary className="cursor-pointer font-medium">{t("set.howto")}</summary>
              <ul className="mt-2 list-disc space-y-1 pl-5">
                <li><InlineMd text={t("set.expander.how1")} /></li>
                <li><InlineMd text={t("set.expander.how2")} /></li>
                <li>{t("set.expander.how3")}</li>
                <li><InlineMd text={t("set.expander.how4")} /></li>
              </ul>
            </details>
          </Section>
        </div>
    ),
        /* Snippets — count + on-disk storage (v0.95.0) */
    "snippets": (
        <div className="mb-6">
          <SnippetsSection />
        </div>
    ),
        /* Clipboard privacy (v0.76.0) */
    "clipboard-history": (
        <div className="mb-6">
          <HistoryLimitSection />
        </div>
    ),
        /* Android (adb) — status + setup guide (v0.119.0) */
    "adb": (
        <div className="mb-6">
          <AdbSection />
        </div>
    ),
    "clipboard-privacy": (
        <div className="mb-6">
          <Section
            icon={<Lock size={16} className="text-[var(--color-accent)]" />}
            id="clipboard-privacy"
      title={t("settings.section.clipboard-privacy")}
            subtitle={t("set.privacy.subtitle")}
          >
            {privacy === null ? (
              <p className="text-[12px] text-[var(--color-muted)]">{t("common.loading")}</p>
            ) : (
              <div className="flex flex-col gap-4">
                <label className="flex flex-col gap-1.5">
                  <span className="text-[12px]">
                    <span className="text-[var(--color-fg)]">
                      {t("set.privacy.exclude")}
                    </span>
                    <span className="block text-[11px] text-[var(--color-muted)]">
                      {t("set.privacy.exclude_help")}
                    </span>
                  </span>
                  <textarea
                    value={privacy.exclude_apps}
                    onChange={(e) =>
                      setPrivacy({ ...privacy, exclude_apps: e.target.value })
                    }
                    onBlur={() => void savePrivacy(privacy)}
                    rows={3}
                    placeholder={"1Password\nKeePassXC\nBitwarden"}
                    className="rounded-md border border-[var(--color-border)] bg-[var(--color-surface)] p-2 font-[var(--font-mono)] text-[12px]"
                  />
                </label>

                <label className="flex items-center justify-between gap-3">
                  <span className="text-[12px]">
                    <span className="text-[var(--color-fg)]">
                      {t("set.privacy.autoclear")}
                    </span>
                    <span className="block text-[11px] text-[var(--color-muted)]">
                      {t("set.privacy.autoclear_help")}
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
                          auto_clear_seconds: Math.max(
                            0,
                            Math.min(3600, parseInt(e.target.value, 10) || 0),
                          ),
                        })
                      }
                      onBlur={() => void savePrivacy(privacy)}
                      className="w-20 rounded-md border border-[var(--color-border)] bg-[var(--color-surface)] px-2 py-1 text-right text-[12px] tabular-nums"
                    />
                    <span className="text-[11px] text-[var(--color-muted)]">{t("common.sec")}</span>
                  </span>
                </label>

                {privacySaved && (
                  <span className="confirm-enter flex items-center gap-1.5 text-[11px] text-green-500">
                    <CheckCircle2 size={12} /> {t("common.saved")}
                  </span>
                )}
              </div>
            )}
          </Section>
        </div>
    ),
        /* Passive auto-expansion (aText-style) section */
    "auto-expand": (
        <div className="mb-6">
          <Section
            id="auto-expand"
            icon={<Wand2 size={16} className="text-[var(--color-accent)]" />}
            title={t("set.ae.title")}
            subtitle={t("set.ae.subtitle")}
          >
            {accessibility === false && (
              <div className="mb-4 flex items-center gap-2 rounded border border-amber-500/40 bg-amber-500/10 px-3 py-2 text-[12px]">
                <AlertTriangle size={14} className="shrink-0 text-amber-500" />
                <span>
                  {t("set.ae.needs_ax")}
                </span>
              </div>
            )}

            {aeCfg === null ? (
              <p className="text-[12px] text-[var(--color-muted)]">{t("common.loading")}</p>
            ) : (
              <div className="flex flex-col gap-3">
                {/* Master toggle */}
                <label className="flex items-center justify-between gap-3">
                  <span className="text-[12px]">
                    <span className="text-[var(--color-fg)]">
                      {t("set.ae.enable")}
                    </span>
                    <span className="block text-[11px] text-[var(--color-muted)]">
                      {t("set.ae.enable_help")}
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
                  <div className="text-[11px] text-[var(--color-muted)]">{t("set.ae.trigger")}</div>
                  <div className="mt-1 flex gap-2">
                    {(["delimiter", "immediate"] as const).map((trig) => (
                      <button
                        key={trig}
                        onClick={() => updateAutoExpand({ trigger: trig })}
                        disabled={aeSaving}
                        className={`flex-1 rounded border px-3 py-1.5 text-[11px] ${
                          aeCfg.trigger === trig
                            ? "border-[var(--color-accent)] text-[var(--color-accent)]"
                            : "border-[var(--color-border)] text-[var(--color-muted)] hover:border-[var(--color-accent)]"
                        }`}
                      >
                        {trig === "delimiter" ? t("set.ae.delimiter") : t("set.ae.immediate")}
                        <span className="mt-0.5 block text-[10px] opacity-70">
                          {trig === "delimiter"
                            ? t("set.ae.delimiter_help")
                            : t("set.ae.immediate_help")}
                        </span>
                      </button>
                    ))}
                  </div>
                </div>

                {/* Boolean options */}
                <div
                  className={`flex flex-col gap-2 ${aeCfg.enabled ? "" : "pointer-events-none opacity-50"}`}
                >
                  <label className="flex items-center justify-between gap-3 text-[12px]">
                    <span>{t("set.ae.match_case")}</span>
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
                      {t("set.ae.inside")}
                      <span className="block text-[11px] text-[var(--color-muted)]">
                        {t("set.ae.inside_help")}
                      </span>
                    </span>
                    <input
                      type="checkbox"
                      checked={aeCfg.expand_inside_words}
                      disabled={aeSaving}
                      onChange={(e) =>
                        updateAutoExpand({ expand_inside_words: e.target.checked })
                      }
                      className="h-4 w-4 shrink-0 accent-[var(--color-accent)]"
                    />
                  </label>
                  <label className="flex items-center justify-between gap-3 text-[12px]">
                    <span>
                      {t("set.ae.undo")}
                      <span className="block text-[11px] text-[var(--color-muted)]">
                        {t("set.ae.undo_help")}
                      </span>
                    </span>
                    <input
                      type="checkbox"
                      checked={aeCfg.undo_enabled}
                      disabled={aeSaving}
                      onChange={(e) =>
                        updateAutoExpand({ undo_enabled: e.target.checked })
                      }
                      className="h-4 w-4 shrink-0 accent-[var(--color-accent)]"
                    />
                  </label>
                </div>

                <details className="mt-1 rounded border border-[var(--color-border)] bg-[var(--color-surface)] p-3 text-[11px] text-[var(--color-muted)]">
                  <summary className="cursor-pointer font-medium">{t("set.ae.notes")}</summary>
                  <ul className="mt-2 list-disc space-y-1 pl-5">
                    <li>{t("set.ae.note1")}</li>
                    <li><InlineMd text={t("set.ae.note2")} /></li>
                    <li><InlineMd text={t("set.ae.note3", { date: "`{date}`", clipboard: "`{clipboard}`", cursor: "`{cursor}`" })} /></li>
                    <li>{t("set.ae.note4")}</li>
                  </ul>
                </details>
              </div>
            )}
          </Section>
        </div>
    ),
        /* Direct hotkey → snippet section */
    "direct-slots": (
        <div className="mb-6">
          <Section
            id="direct-slots"
            icon={<Zap size={16} className="text-[var(--color-accent)]" />}
            title={t("settings.section.direct-slots")}
            subtitle={t("set.slots.subtitle")}
          >
            {accessibility === false && (
              <div className="mb-4 flex items-center gap-2 rounded border border-amber-500/40 bg-amber-500/10 px-3 py-2 text-[12px]">
                <AlertTriangle size={14} className="shrink-0 text-amber-500" />
                <span>
                  <InlineMd text={t("set.slots.needs_ax")} />
                </span>
              </div>
            )}

            {slots.length === 0 ? (
              <p className="mb-3 text-[12px] text-[var(--color-muted)]">
                {t("set.slots.empty")}
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
                            {t("set.slots.deleted")}
                          </option>
                        )}
                        {snippetList.map((s) => (
                          <option key={s.id} value={s.id}>
                            {s.abbreviation} — {s.title || t("set.slots.untitled")}
                          </option>
                        ))}
                      </select>
                      <button
                        onClick={() => removeSlot(i)}
                        disabled={slotsBusy}
                        className="rounded px-1.5 text-[14px] text-[var(--color-muted)] hover:text-red-400 disabled:opacity-40"
                        title={t("set.slots.remove")}
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
                {t("set.slots.add")}
              </button>
              <button
                onClick={() => void saveSlots()}
                disabled={slotsBusy || !slotsDirty}
                className="rounded bg-[var(--color-accent)] px-3 py-1 text-[12px] text-[var(--color-accent-fg)] hover:opacity-90 disabled:opacity-50"
              >
                {slotsBusy ? t("common.saving") : slotsDirty ? t("set.slots.save") : t("common.no_changes")}
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
              <InlineMd text={t("set.slots.help", { popup: "`" + (IS_MAC ? "⌃Space" : "Ctrl+Space") + "`", ocr: "`" + (IS_MAC ? "⌃⇧O" : "Ctrl+Shift+O") + "`" })} />
            </p>
          </Section>
        </div>
    ),
    "language": <LanguageSection />,
        /* Appearance section */
    "appearance": (
        <div className="mb-6">
          <Section
            icon={<SunMoon size={16} className="text-[var(--color-accent)]" />}
            id="appearance"
      title={t("settings.section.appearance")}
            subtitle={t("set.appearance.subtitle")}
          >
            <Row label={t("set.appearance.theme")}>
              <div
                role="radiogroup"
                aria-label={t("set.appearance.theme_aria")}
                className="flex overflow-hidden rounded-lg border border-[var(--color-border)]"
              >
                {(
                  [
                    { value: "system", label: t("set.appearance.system"), Icon: Monitor },
                    { value: "light", label: t("set.appearance.light"), Icon: Sun },
                    { value: "dark", label: t("set.appearance.dark"), Icon: Moon },
                  ] as const
                ).map(({ value, label, Icon }) => {
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
                ? t("common.loading")
                : theme === "system"
                  ? t("set.appearance.follow_os")
                  : t(theme === "light" ? "set.appearance.forced_light" : "set.appearance.forced_dark")}
            </p>

            <Row label={t("set.appearance.animations")}>
              <div
                role="radiogroup"
                aria-label={t("set.appearance.animations")}
                className="flex overflow-hidden rounded-lg border border-[var(--color-border)]"
              >
                {(["full", "reduced", "off"] as const).map((value) => {
                  const active = animStage === value;
                  return (
                    <button
                      key={value}
                      type="button"
                      role="radio"
                      aria-checked={active}
                      disabled={animStage === null || animStageBusy}
                      onClick={() => void changeAnimStage(value)}
                      className={
                        "flex items-center gap-1.5 px-3 py-1.5 text-[12px] transition-colors " +
                        "border-r border-[var(--color-border)] last:border-r-0 " +
                        (active
                          ? "bg-[var(--color-accent)] text-[var(--color-accent-fg)]"
                          : "bg-[var(--color-surface)] text-[var(--color-muted)] hover:text-[var(--color-fg)]")
                      }
                    >
                      {stageLabel(value)}
                    </button>
                  );
                })}
              </div>
            </Row>
            <p className="mt-2 text-[11px] leading-snug text-[var(--color-muted)]">
              {animStage === "off"
                ? t("set.appearance.anim_off")
                : animStage === "reduced"
                  ? t("set.appearance.anim_reduced")
                  : t("set.appearance.anim_full")}
            </p>

            <Row label={t("set.appearance.size")}>
              <div
                role="radiogroup"
                aria-label={t("set.appearance.size")}
                className="flex overflow-hidden rounded-lg border border-[var(--color-border)]"
              >
                {(
                  [
                    { value: "small", label: t("set.appearance.small") },
                    { value: "medium", label: t("set.appearance.medium") },
                    { value: "large", label: t("set.appearance.large") },
                  ] as const
                ).map(({ value, label }) => {
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
                ? t("common.loading")
                : t("set.appearance.size_help")}
            </p>
            <Row label={t("set.appearance.lineage")}>
              <label className="flex cursor-pointer items-center gap-2 text-[12px]">
                <input
                  type="checkbox"
                  checked={lineageRails}
                  onChange={(e) => void changeLineageRails(e.target.checked)}
                  className="accent-[var(--color-accent)]"
                />
                {t("set.appearance.lineage_toggle")}
              </label>
            </Row>
            <p className="mt-2 text-[11px] leading-snug text-[var(--color-muted)]">
              {t("set.appearance.lineage_help")}
            </p>
            <CrtAnimationRow />
          </Section>
        </div>
    ),
        /* Startup section */
    "startup": (
        <div className="mb-6">
          <Section
            icon={<Power size={16} className="text-[var(--color-accent)]" />}
            id="startup"
      title={t("settings.section.startup")}
            subtitle={IS_MAC ? t("set.startup.subtitle_mac") : t("set.startup.subtitle_win")}
          >
            <Row label={IS_MAC ? t("tray.autostart_login") : t("tray.autostart_windows")}>
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
                    ? t("common.loading")
                    : autostart
                      ? t("set.startup.on")
                      : t("set.startup.off")}
                </span>
              </label>
            </Row>
            <Row label={t("set.startup.keepalive")}>
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
                    ? t("common.loading")
                    : keepalive
                      ? t("set.startup.keepalive_on")
                      : t("set.startup.keepalive_off")}
                </span>
              </label>
            </Row>
          </Section>
        </div>
    ),
        /* Paste behaviour section */
    "paste": (
        <div className="mb-6">
          <Section
            id="paste"
            icon={<ClipboardType size={16} className="text-[var(--color-accent)]" />}
            title={t("settings.section.paste")}
            subtitle={t("set.paste.subtitle")}
          >
            <Row label={t("set.paste.plain")}>
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
                    ? t("common.loading")
                    : plainTextOnly
                      ? t("set.paste.plain_on")
                      : t("set.paste.plain_off")}
                </span>
              </label>
            </Row>

            <div className="mt-1 rounded border border-dashed border-[var(--color-border)] bg-[var(--color-surface)] p-2.5 text-[11px] text-[var(--color-muted)]">
              <span className="text-[var(--color-fg)]">{t("set.paste.tip_title")}</span>{" "}
              <InlineMd text={t("set.paste.tip")} />
            </div>
          </Section>
        </div>
    ),
        /* Capture section — OCR / screenshot / region-pick behaviours. */
    "capture": (
        <div className="mb-6">
          <Section
            id="capture"
            icon={<Camera size={16} className="text-[var(--color-accent)]" />}
            title={t("set.capture.title")}
            subtitle={t("set.capture.subtitle")}
          >
            <Row label={t("set.capture.keep_source")}>
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
                    ? t("common.loading")
                    : ocrSaveSource
                      ? t("set.capture.keep_on")
                      : t("set.capture.keep_off")}
                </span>
              </label>
            </Row>
          </Section>
        </div>
    ),
        /* Input Lock section — type `freeze` in the search bar to
            block all keyboard / mouse input; release with the chord. */
    "input-lock": (
        <div className="mb-6">
          <Section
            icon={<Lock size={16} className="text-[var(--color-accent)]" />}
            id="input-lock"
      title={t("settings.section.input-lock")}
            subtitle={t("set.inputlock.subtitle")}
          >
            <Row
              label={t("set.inputlock.chord")}
              help={t("set.inputlock.chord_help")}
            >
              <div className="flex flex-wrap items-center gap-2">
                <div className="flex items-center gap-1 rounded border border-[var(--color-border)] bg-[var(--color-surface)] px-2 py-1 font-[var(--font-mono)] text-[12px]">
                  {capturedChord !== null ? (
                    capturedChord.size === 0 ? (
                      <span className="text-[var(--color-muted)]">{t("set.inputlock.press")}</span>
                    ) : (
                      Array.from(capturedChord).map((k, i) => (
                        <span
                          key={i}
                          className="rounded bg-[var(--color-bg)] px-1.5 py-0.5"
                        >
                          {k}
                        </span>
                      ))
                    )
                  ) : inputLockChord === null ? (
                    <span className="text-[var(--color-muted)]">{t("common.loading")}</span>
                  ) : inputLockChord.length === 0 ? (
                    <span className="text-[var(--color-muted)]">{t("common.none")}</span>
                  ) : (
                    inputLockChord.map((k, i) => (
                      <span
                        key={i}
                        className="rounded bg-[var(--color-bg)] px-1.5 py-0.5"
                      >
                        {k}
                      </span>
                    ))
                  )}
                </div>
                <button
                  type="button"
                  onClick={() => setCapturedChord(new Set())}
                  disabled={
                    inputLockChord === null || inputLockSaving || capturedChord !== null
                  }
                  className="rounded border border-[var(--color-border)] px-2.5 py-1 text-[11px] hover:border-[var(--color-accent)] hover:text-[var(--color-accent)] disabled:opacity-40"
                >
                  {capturedChord !== null ? t("set.diag.capturing") : t("set.inputlock.capture")}
                </button>
                {capturedChord !== null && (
                  <button
                    type="button"
                    onClick={() => setCapturedChord(null)}
                    className="rounded border border-[var(--color-border)] px-2.5 py-1 text-[11px] text-[var(--color-muted)] hover:border-amber-500 hover:text-amber-500"
                  >
                    {t("common.cancel")}
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
              <span className="text-[var(--color-fg)]">{t("set.inputlock.safety_title")}</span>{" "}
              {t("set.inputlock.safety", { keys: IS_MAC ? t("set.inputlock.force_quit") : "Ctrl+Alt+Del" })}
            </div>
          </Section>
        </div>
    ),
        /* Keyboard shortcuts cheat sheet */
    "keyboard-shortcuts": (
        <div className="mb-6">
          <Section
            id="keyboard-shortcuts"
            icon={<Keyboard size={16} className="text-[var(--color-accent)]" />}
            title={t("set.cheatsheet.title")}
            subtitle={t("set.cheatsheet.subtitle")}
          >
            <ShortcutsTable />
          </Section>
        </div>
    ),
        /* Cleaning workflow (v0.60.0) */
    "cleaning": (
        <div className="mb-6">
          <Section
            icon={<Trash2 size={16} className="text-[var(--color-accent)]" />}
            id="cleaning"
      title={t("settings.section.cleaning")}
            subtitle={t("set.clean.subtitle")}
          >
            {cleanCfg === null ? (
              <p className="text-[12px] text-[var(--color-muted)]">{t("common.loading")}</p>
            ) : (
              <div className="flex flex-col gap-3">
                {/* Level picker */}
                <div>
                  <div className="text-[11px] text-[var(--color-muted)]">{t("set.clean.level")}</div>
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
                        {lv === "safe"
                          ? t("set.clean.safe")
                          : lv === "standard"
                            ? t("set.clean.standard")
                            : t("set.clean.aggressive")}
                      </button>
                    ))}
                  </div>
                  {cleanCfg.level === "aggressive" && (
                    <div className="mt-2 flex items-center gap-2 rounded border border-amber-500/40 bg-amber-500/10 px-3 py-2 text-[11px]">
                      <AlertTriangle size={13} className="shrink-0 text-amber-500" />
                      <span>
                        {t("set.clean.aggressive_warn")}
                      </span>
                    </div>
                  )}
                </div>

                {/* Min age */}
                <label className="flex items-center justify-between gap-3 text-[12px]">
                  <span>
                    {t("set.clean.min_age")}
                    <span className="block text-[11px] text-[var(--color-muted)]">
                      {t("set.clean.min_age_help")}
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
                  <div className="text-[11px] text-[var(--color-muted)]">{t("set.clean.categories")}</div>
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
                {/* Developer cleanup (v0.84.264): the stale-artifact scanner is
                    the only part whose search area the user defines. */}
                <div className="flex flex-col gap-2 border-t border-[var(--color-border)] pt-3">
                  <div className="text-[11px] text-[var(--color-muted)]">
                    {t("set.clean.dev")}
                  </div>
                  <label className="flex flex-col gap-1 text-[12px]">
                    <span>
                      {t("set.clean.dev_roots")}
                      <span className="block text-[11px] text-[var(--color-muted)]">
                        <InlineMd text={t("set.clean.dev_roots_help")} />
                      </span>
                    </span>
                    <textarea
                      rows={3}
                      defaultValue={cleanCfg.dev_roots.join("\n")}
                      onBlur={(e) =>
                        void updateCleaner({
                          dev_roots: e.target.value
                            .split("\n")
                            .map((s) => s.trim())
                            .filter(Boolean),
                        })
                      }
                      spellCheck={false}
                      className="rounded border border-[var(--color-border)] bg-[var(--color-bg)] px-2 py-1 font-mono text-[11px]"
                    />
                  </label>
                  <label className="flex items-center justify-between gap-3 text-[12px]">
                    <span>
                      {t("set.clean.stale")}
                      <span className="block text-[11px] text-[var(--color-muted)]">
                        {t("set.clean.stale_help")}
                      </span>
                    </span>
                    <input
                      type="number"
                      min={1}
                      max={3650}
                      value={cleanCfg.stale_days}
                      onChange={(e) =>
                        void updateCleaner({
                          stale_days: Math.max(1, parseInt(e.target.value, 10) || 1),
                        })
                      }
                      className="w-20 rounded border border-[var(--color-border)] bg-[var(--color-bg)] px-2 py-1 text-right text-[12px]"
                    />
                  </label>
                </div>

                <p className="text-[11px] text-[var(--color-muted)]">
                  <InlineMd text={t("set.clean.safety")} />
                </p>
              </div>
            )}
          </Section>
        </div>
    ),
        /* Timesheet (time tracking) — macOS for now (OS module is macOS-only) */
    "timesheet": (
        IS_MAC && (
          <div className="mb-6">
            <TimesheetSection />
          </div>
        )
    ),
        /* Bruno defaults — German income-tax calculator personal params */
    "pulse": (
        IS_MAC && (
          <div className="mb-6">
            <PulseSettingsSection />
          </div>
        )
    ),
        /* Bruno defaults — German income-tax calculator personal params */
    "bruno": (
        <div className="mb-6">
          <BrunoSection />
        </div>
    ),
        /* Faker defaults — fake-data generator */
    "faker": (
        <div className="mb-6">
          <FakerSection />
        </div>
    ),
        /* Security builders — pentest command builders */
    "security": (
        <div className="mb-6">
          <SecuritySection />
        </div>
    ),
        /* Figlet — ASCII-art banner defaults */
    "figlet": (
        <div className="mb-6">
          <FigletSection />
        </div>
    ),
        /* Meme library directory */
    "meme": MEME_ENABLED && (
          <div className="mb-6">
            <MemeSection />
          </div>
    ),
        /* AI providers — keys + models for the `task` / `ki` command */
    "ai": (
        <div className="mb-6">
          <AiProvidersSection />
        </div>
    ),
        /* Weather — OpenWeather API key + units */
    "weather": (
        <div className="mb-6">
          <WeatherSection />
        </div>
    ),
        /* Spotify — local play history (macOS) */
    "spotify": IS_MAC ? (
        <div className="mb-6">
          <SpotifySection />
        </div>
    ) : null,
        /* Cloud sync with cue (snippets) */
    "cloud-sync": (
        <div className="mb-6">
          <CloudSyncSection />
        </div>
    ),
        /* Backup & restore section */
    "backup": (
        <div className="mb-6">
          <Section
            icon={<Archive size={16} className="text-[var(--color-accent)]" />}
            id="backup"
      title={t("settings.section.backup")}
            subtitle={t("set.backup.subtitle")}
          >
            <Row label={t("set.backup.included")}>
              <div className="flex flex-col gap-1.5 text-[12px]">
                <span className="text-[var(--color-muted)]">
                  {t("set.backup.always")}
                </span>
                <BackupCheckbox
                  label={t("set.backup.timesheet")}
                  checked={includeTimesheet}
                  onChange={setIncludeTimesheet}
                />
              </div>
            </Row>

            <Row label={t("set.backup.encryption")}>
              <div className="flex flex-col gap-1.5 text-[12px]">
                <BackupCheckbox
                  label={t("set.backup.encrypt")}
                  checked={encryptBackup}
                  onChange={(v) => {
                    setEncryptBackup(v);
                    if (!v) {
                      setBackupPassword("");
                      setBackupPassword2("");
                    }
                  }}
                />
                {encryptBackup && (
                  <div className="flex flex-wrap items-center gap-2">
                    <input
                      type={showBackupPassword ? "text" : "password"}
                      value={backupPassword}
                      onChange={(e) => setBackupPassword(e.target.value)}
                      placeholder={t("common.password")}
                      className="w-40 rounded border border-[var(--color-border)] bg-[var(--color-bg)] px-2 py-1 text-[12px] outline-none focus:border-[var(--color-accent)]"
                    />
                    <input
                      type={showBackupPassword ? "text" : "password"}
                      value={backupPassword2}
                      onChange={(e) => setBackupPassword2(e.target.value)}
                      placeholder={t("set.backup.repeat")}
                      className="w-40 rounded border border-[var(--color-border)] bg-[var(--color-bg)] px-2 py-1 text-[12px] outline-none focus:border-[var(--color-accent)]"
                    />
                    <BackupCheckbox
                      label={t("common.show")}
                      checked={showBackupPassword}
                      onChange={setShowBackupPassword}
                    />
                    {backupPassword2.length > 0 && backupPassword !== backupPassword2 && (
                      <span className="text-[11px] text-red-400">
                        {t("set.backup.pw_mismatch")}
                      </span>
                    )}
                  </div>
                )}
                {encryptBackup && (
                  <span className="text-[11px] text-amber-500">
                    {t("set.backup.no_recovery")}
                  </span>
                )}
              </div>
            </Row>

            <div className="mt-4 flex flex-wrap items-center gap-2">
              <button
                onClick={() => void onExport()}
                disabled={backupBusy !== null}
                className="flex items-center gap-1.5 rounded bg-[var(--color-accent)] px-3 py-1 text-[12px] text-[var(--color-accent-fg)] hover:opacity-90 disabled:opacity-50"
              >
                <Download size={12} />
                {backupBusy === "export" ? t("set.backup.exporting") : t("set.backup.export")}
              </button>
              <button
                onClick={() => void onImport()}
                disabled={backupBusy !== null}
                className="flex items-center gap-1.5 rounded border border-[var(--color-border)] px-3 py-1 text-[12px] hover:border-[var(--color-accent)] hover:text-[var(--color-accent)] disabled:opacity-50"
              >
                <Upload size={12} />
                {backupBusy === "import" ? t("set.backup.importing") : t("set.backup.import")}
              </button>
              <span className="ml-auto text-[11px] text-[var(--color-muted)]">
                {t("set.backup.merge_note")}
              </span>
            </div>

            {pendingImport && (
              <div className="mt-3 flex flex-wrap items-center gap-2 rounded border border-[var(--color-border)] bg-[var(--color-surface)] px-2.5 py-2 text-[12px]">
                <Lock size={13} className="shrink-0 text-[var(--color-accent)]" />
                <span className="text-[var(--color-muted)]">
                  {t("set.backup.is_encrypted", { file: pendingImport.split("/").pop() ?? pendingImport })}
                </span>
                <input
                  type="password"
                  value={importPassword}
                  onChange={(e) => setImportPassword(e.target.value)}
                  onKeyDown={(e) => {
                    if (e.key === "Enter" && importPassword)
                      void runImport(pendingImport, importPassword);
                  }}
                  placeholder={t("common.password")}
                  autoFocus
                  className="w-40 rounded border border-[var(--color-border)] bg-[var(--color-bg)] px-2 py-1 text-[12px] outline-none focus:border-[var(--color-accent)]"
                />
                <button
                  onClick={() => void runImport(pendingImport, importPassword)}
                  disabled={backupBusy !== null || !importPassword}
                  className="rounded bg-[var(--color-accent)] px-2.5 py-1 text-[12px] text-[var(--color-accent-fg)] hover:opacity-90 disabled:opacity-50"
                >
                  {backupBusy === "import" ? t("set.backup.importing") : t("set.backup.unlock")}
                </button>
                <button
                  onClick={() => {
                    setPendingImport(null);
                    setImportPassword("");
                  }}
                  className="rounded border border-[var(--color-border)] px-2.5 py-1 text-[12px]"
                >
                  {t("common.cancel")}
                </button>
              </div>
            )}

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
                    <InlineMd text={t("set.backup.imported", {
                      history: backupStatus.result.history_imported,
                      snippets: backupStatus.result.snippets_imported,
                      notes: backupStatus.result.notes_imported,
                      totp: backupStatus.result.totp_imported,
                      settings: backupStatus.result.settings_imported,
                    })} />
                    {backupStatus.result.timesheet_imported > 0 && (
                      <InlineMd text={t("set.backup.imported_timesheet", { n: backupStatus.result.timesheet_imported })} />
                    )}
                    {backupStatus.result.errors.length > 0 && (
                      <>
                        {" — "}
                        <span className="text-red-400">
                          {backupStatus.result.errors[0]}
                          {backupStatus.result.errors.length > 1 &&
                            ` ${t("set.backup.more_errors", { n: backupStatus.result.errors.length - 1 })}`}
                        </span>
                      </>
                    )}
                  </>
                ) : (
                  <>{t("common.failed", { message: backupStatus.message })}</>
                )}
              </div>
            )}
          </Section>
        </div>
    ),
    "linux-shortcuts": (
        IS_LINUX && (
          <div id="settings-linux-shortcuts" className="mb-6 scroll-mt-12">
            <LinuxShortcutsSettings />
          </div>
        )
    ),
        /* About section — inline (replaced the former modal dialog) */
    "about": (
        <div className="mb-6">
          <Section
            id="about"
            icon={<Info size={16} className="text-[var(--color-accent)]" />}
            title={t("settings.section.about")}
            subtitle={t("set.about.subtitle")}
          >
            <AboutContent version={appVersion} />
          </Section>
        </div>
    ),
    "changelog": (
        <div className="mb-6">
          <Section
            id="changelog"
            icon={<ScrollText size={16} className="text-[var(--color-accent)]" />}
            title={t("settings.section.changelog")}
            subtitle={t("set.changelog.subtitle")}
          >
            <ChangelogView currentVersion={appVersion} />
          </Section>
        </div>
    ),
    "popup-hotkey": (
        <div className="mb-6">
          <PopupHotkeySection />
        </div>
    ),
    "history-hotkey": (
        <div className="mb-6">
          <HistoryHotkeySection />
        </div>
    ),
    "pagespeed": (
        <div className="mb-6">
          <PagespeedSection />
        </div>
    ),
    "repos": (
        <div className="mb-6">
          <RepositoriesSection />
        </div>
    ),
    "record": (
        <div className="mb-6">
          <RecordSettingsSection />
        </div>
    ),
    "github": (
        <div className="mb-6">
          <GithubSettingsSection />
        </div>
    ),
    "device-sync": (
        <div className="mb-6">
          <DeviceSyncSection />
        </div>
    ),
    "auto-backup": (
        <div className="mb-6">
          <AutoBackupSection onRestored={onBackupImported} />
        </div>
    ),
  };

  return (
    <div ref={scrollRef} className="relative flex min-h-0 flex-1 flex-col overflow-auto p-6">
      <SettingsCategoryNav scrollRef={scrollRef} />
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
              <span className="flex-1 font-medium">{t("set.perm.needed")}</span>
              <button
                onClick={() => void setUpPermissions()}
                className="rounded bg-[var(--color-accent)] px-3 py-1 text-[11px] font-medium text-[var(--color-accent-fg)] hover:opacity-90"
              >
                {chaining ? t("set.perm.setting_up") : t("set.perm.setup")}
              </button>
            </div>

            {/* Explainer — honest about what the button can and can't do. */}
            <p className="px-3 pt-2 text-[var(--color-muted)]">
              <InlineMd text={t("set.perm.explainer")} />
            </p>

            {/* Live per-permission status. */}
            <div className="flex flex-col gap-1.5 px-3 py-2">
              <PermRow
                label={t("set.perm.ax")}
                hint={t("set.perm.ax_hint")}
                granted={accessibility}
                onOpen={() =>
                  void openAccessibilitySettings().catch((e) =>
                    setStatus({ kind: "err", message: String(e) }),
                  )
                }
              />
              <PermRow
                label={t("set.perm.screen")}
                hint={t("set.perm.screen_hint")}
                granted={screenRec}
                onOpen={() =>
                  void openScreenRecordingSettings().catch((e) =>
                    setStatus({ kind: "err", message: String(e) }),
                  )
                }
              />
              <PermRow
                label={t("set.perm.finder")}
                hint={t("set.perm.finder_hint")}
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
                {t("set.perm.stuck")}
              </summary>
              <p className="mt-1.5">
                <InlineMd text={t("set.perm.stuck_help")} />
              </p>
              <div className="mt-2 flex flex-wrap gap-2">
                <button
                  onClick={async () => {
                    if (
                      !window.confirm(
                        t("set.perm.reset_confirm"),
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
                  {t("set.perm.reset")}
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
                  {t("set.perm.recheck")}
                </button>
                <button
                  onClick={async () => {
                    if (
                      !window.confirm(
                        t("set.perm.quit_confirm"),
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
                  {t("tray.quit")}
                </button>
              </div>
            </details>
          </div>
        </div>
      )}

      <div className="md3-settings-cascade w-full">
        {SETTINGS_CATEGORIES.map((cat) => {
          const items = cat.sections.filter((id) => blocks[id]);
          if (items.length === 0) return null;
          return (
            <section
              key={cat.id}
              id={`settings-cat-${cat.id}`}
              aria-labelledby={`settings-cat-${cat.id}-title`}
              className="mb-4 scroll-mt-12"
            >
              <h2
                id={`settings-cat-${cat.id}-title`}
                className="mb-3 border-b border-[var(--color-border)] pb-1 text-[11px] font-semibold uppercase tracking-wider text-[var(--color-muted)]"
              >
                {cat.label}
              </h2>
              {items.map((id) => (
                <Fragment key={id}>{blocks[id]}</Fragment>
              ))}
            </section>
          );
        })}
      </div>
    </div>
  );
}

/**
 * Sticky jump bar over the Settings tab: one chip per category, the one in
 * view highlighted. Scrolls the panel's own container (the tab scrolls, not
 * the window).
 */
/** Chips in the sticky bar — "Info" is a footer, not a destination. */
const NAV_CATEGORIES = SETTINGS_CATEGORIES.filter((c) => c.id !== "about");

function SettingsCategoryNav({ scrollRef }: { scrollRef: React.RefObject<HTMLDivElement | null> }) {
  const t = useT();
  const [active, setActive] = useState<string>(SETTINGS_CATEGORIES[0].id);

  useEffect(() => {
    const root = scrollRef.current;
    if (!root) return;
    let frame = 0;
    const update = () => {
      frame = 0;
      const top = root.getBoundingClientRect().top + 60;
      let current = SETTINGS_CATEGORIES[0].id;
      for (const c of SETTINGS_CATEGORIES) {
        const el = document.getElementById(`settings-cat-${c.id}`);
        if (el && el.getBoundingClientRect().top <= top) current = c.id;
      }
      // The last sections are too short to reach the top line — at the
      // bottom end the last chip wins, otherwise a click on it would leave
      // the highlight on its predecessor.
      if (root.scrollTop + root.clientHeight >= root.scrollHeight - 4) current = NAV_CATEGORIES[NAV_CATEGORIES.length - 1].id;
      setActive(current);
    };
    const onScroll = () => {
      if (!frame) frame = requestAnimationFrame(update);
    };
    root.addEventListener("scroll", onScroll, { passive: true });
    return () => {
      root.removeEventListener("scroll", onScroll);
      if (frame) cancelAnimationFrame(frame);
    };
  }, [scrollRef]);

  const jump = (id: string) => {
    const el = document.getElementById(`settings-cat-${id}`);
    if (!el) return;
    const reduced = window.matchMedia("(prefers-reduced-motion: reduce)").matches;
    el.scrollIntoView({ behavior: reduced ? "auto" : "smooth", block: "start" });
    setActive(id);
  };

  return (
    <nav
      aria-label={t("set.categories_aria")}
      className="sticky -top-6 z-10 -mx-6 -mt-6 mb-4 flex shrink-0 gap-1 overflow-x-auto border-b border-[var(--color-border)] bg-[var(--color-bg)] px-6 py-2"
    >
      {NAV_CATEGORIES.map((c) => (
        <button
          key={c.id}
          type="button"
          aria-current={active === c.id ? "true" : undefined}
          onClick={() => jump(c.id)}
          className={`shrink-0 rounded-full px-2.5 py-1 text-[11px] transition-colors duration-(--duration-fast) ease-sharp ${
            active === c.id
              ? "bg-[var(--color-accent)] text-[var(--color-accent-fg)]"
              : "text-[var(--color-muted)] hover:bg-[var(--color-surface)] hover:text-[var(--color-fg)]"
          }`}
        >
          {c.label}
        </button>
      ))}
    </nav>
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
  const t = useT();
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
        <span className="shrink-0 text-[11px] font-medium text-emerald-500">{t("set.perm.enabled")}</span>
      ) : (
        <button
          onClick={onOpen}
          className="shrink-0 rounded bg-[var(--color-accent)] px-2.5 py-1 text-[11px] text-[var(--color-accent-fg)] hover:opacity-90"
        >
          {t("common.open")}
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
  const t = useT();
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
      id="timesheet"
      title={t("settings.section.timesheet")}
      subtitle={t("set.ts.subtitle")}
    >
      <Row
        label={t("set.ts.idle")}
        help={t("set.ts.idle_help")}
      >
        <input
          type="number"
          min={10}
          value={cfg.idle_seconds}
          onChange={(e) =>
            setCfg({ ...cfg, idle_seconds: Number(e.target.value) || 300 })
          }
          className="w-28 rounded border border-[var(--color-border)] bg-[var(--color-surface)] px-2 py-1 text-[12px] tabular-nums"
        />
      </Row>
      <Row
        label={t("set.ts.retention")}
        help={t("set.ts.retention_help")}
      >
        <input
          type="number"
          min={0}
          value={cfg.retention_days}
          onChange={(e) =>
            setCfg({ ...cfg, retention_days: Number(e.target.value) || 0 })
          }
          className="w-28 rounded border border-[var(--color-border)] bg-[var(--color-surface)] px-2 py-1 text-[12px] tabular-nums"
        />
      </Row>
      <Row
        label={t("set.ts.claude")}
        help={t("set.ts.claude_help")}
      >
        <label className="flex cursor-pointer items-center gap-2 text-[12px]">
          <input
            type="checkbox"
            checked={cfg.claude_watcher}
            onChange={(e) => setCfg({ ...cfg, claude_watcher: e.target.checked })}
            className="accent-[var(--color-accent)]"
          />
          <span className="text-[var(--color-muted)]">
            {cfg.claude_watcher ? t("set.ts.claude_on") : t("common.off")}
          </span>
        </label>
      </Row>
      <Row
        label={t("set.ts.goal")}
        help={t("set.ts.goal_help")}
      >
        <input
          type="number"
          min={0}
          step={30}
          value={cfg.daily_goal_minutes}
          onChange={(e) =>
            setCfg({ ...cfg, daily_goal_minutes: Number(e.target.value) || 0 })
          }
          className="w-28 rounded border border-[var(--color-border)] bg-[var(--color-surface)] px-2 py-1 text-[12px] tabular-nums"
        />
      </Row>
      <Row
        label={t("set.ts.denylist")}
        help={t("set.ts.denylist_help")}
      >
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
          {busy ? t("common.saving") : t("common.save")}
        </button>
        {dirty && !busy && (
          <span className="text-[11px] text-[var(--color-muted)]">{t("common.unsaved")}</span>
        )}
        {savedOk && !dirty && (
          <span className="confirm-enter text-[11px] text-[var(--color-muted)]">{t("common.saved")}</span>
        )}
      </div>
      <Row
        label={t("set.ts.rules")}
        help={t("set.ts.rules_help")}
      >
        <CategoryRulesList />
      </Row>
    </Section>
  );
}

function CategoryRulesList() {
  const t = useT();
  const [rules, setRules] = useState<[string, string][]>([]);
  const reload = () =>
    trackCategoryRules()
      .then(setRules)
      .catch(() => undefined);
  useEffect(() => {
    void reload();
  }, []);
  if (rules.length === 0) {
    return <p className="text-[12px] text-[var(--color-muted)]">{t("set.ts.no_rules")}</p>;
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
            title={t("set.ts.delete_rule")}
          >
            <Trash2 size={13} />
          </button>
        </div>
      ))}
    </div>
  );
}

/** Popup behavior — top of the panel: does clicking outside close the
 *  overlay? (Persisted `popup.close_on_blur`, live effect via UiState.) */
/** Settings → Pulse (memory pressure / SSD writes sampler). Saved live. */
function PulseSettingsSection() {
  const t = useT();
  const [cfg, setCfg] = useState<PulseConfig | null>(null);
  const [tbw, setTbw] = useState("");

  useEffect(() => {
    void getPulseConfig()
      .then((c) => {
        setCfg(c);
        setTbw(c.tbw_tb > 0 ? String(c.tbw_tb) : "");
      })
      .catch(() => {});
  }, []);

  if (!cfg) return null;

  const save = async (next: PulseConfig) => {
    setCfg(next); // optimistic
    try {
      setCfg(await setPulseConfig(next));
    } catch {
      setCfg(cfg);
    }
  };

  const box = "mt-0.5 accent-[var(--color-accent)]";
  const field =
    "w-20 rounded border border-[var(--color-border)] bg-[var(--color-bg)] px-2 py-1 font-[var(--font-mono)] text-[12px]";

  return (
    <Section
      icon={<Activity size={16} className="text-[var(--color-accent)]" />}
      id="pulse"
      title={t("set.pulse.title")}
      subtitle={t("set.pulse.subtitle")}
    >
      <label className="flex cursor-pointer items-start gap-2 text-[12px]">
        <input
          type="checkbox"
          checked={cfg.enabled}
          onChange={(e) => void save({ ...cfg, enabled: e.target.checked })}
          className={box}
        />
        <span>
          <span className="font-medium">{t("set.pulse.measure")}</span>
          <span className="mt-0.5 block text-[var(--color-muted)]">
            {t("set.pulse.measure_help")}
          </span>
        </span>
      </label>
      <label className="mt-3 flex cursor-pointer items-start gap-2 text-[12px]">
        <input
          type="checkbox"
          checked={cfg.notify}
          disabled={!cfg.enabled}
          onChange={(e) => void save({ ...cfg, notify: e.target.checked })}
          className={box}
        />
        <span>
          <span className="font-medium">{t("set.pulse.notify")}</span>
          <span className="mt-0.5 block text-[var(--color-muted)]">
            {tSplit("set.pulse.notify_help", "n")[0]}
            <input
              type="number"
              min={1}
              max={240}
              value={cfg.notify_minutes}
              onChange={(e) => {
                const n = Number(e.target.value);
                if (Number.isFinite(n) && n >= 1) void save({ ...cfg, notify_minutes: Math.round(n) });
              }}
              className={field}
              aria-label={t("set.pulse.minutes_aria")}
            />
            {tSplit("set.pulse.notify_help", "n")[1]}
          </span>
        </span>
      </label>
      <div className="mt-3 text-[12px]">
        <span className="font-medium">{t("set.pulse.tbw")}</span>
        <div className="mt-1 flex items-center gap-2 text-[var(--color-muted)]">
          <input
            type="number"
            min={0}
            step={50}
            value={tbw}
            placeholder={t("set.pulse.auto")}
            onChange={(e) => setTbw(e.target.value)}
            onBlur={() => {
              const n = Number(tbw);
              void save({ ...cfg, tbw_tb: Number.isFinite(n) && n > 0 ? n : 0 });
            }}
            className={field}
            aria-label={t("set.pulse.tbw_aria")}
          />
          <span>{t("set.pulse.tbw_help")}</span>
        </div>
      </div>
    </Section>
  );
}

function PopupBehaviorSection() {
  const t = useT();
  const [closeOnBlur, setCloseOnBlurState] = useState<boolean | null>(null);

  useEffect(() => {
    void getPopupCloseOnBlur().then(setCloseOnBlurState).catch(() => {});
  }, []);

  const toggle = async (next: boolean) => {
    setCloseOnBlurState(next); // optimistic — the IPC persists + applies live
    try {
      await setPopupCloseOnBlur(next);
    } catch {
      setCloseOnBlurState(!next);
    }
  };

  if (closeOnBlur === null) return null;

  return (
    <Section
      icon={<MousePointerClick size={16} className="text-[var(--color-accent)]" />}
      id="behavior"
      title={t("settings.section.behavior")}
      subtitle={t("set.behavior.subtitle")}
    >
      <label className="flex cursor-pointer items-start gap-2 text-[12px]">
        <input
          type="checkbox"
          checked={closeOnBlur}
          onChange={(e) => void toggle(e.target.checked)}
          className="mt-0.5 accent-[var(--color-accent)]"
        />
        <span>
          <span className="font-medium">{t("set.behavior.blur")}</span>
          <span className="mt-0.5 block text-[var(--color-muted)]">
            <InlineMd text={t("set.behavior.blur_help")} />
          </span>
        </span>
      </label>
    </Section>
  );
}

/** Cloud-Sync (cue): bidirectional snippet sync with the cue web app. */
function PagespeedSection() {
  const t = useT();
  const [stored, setStored] = useState(false);
  const [key, setKey] = useState("");
  const [note, setNote] = useState<string | null>(null);
  useEffect(() => {
    void getPagespeedKey().then(setStored).catch(() => {});
  }, []);
  return (
    <Section
      icon={<Gauge size={16} className="text-[var(--color-accent)]" />}
      title={t("settings.section.pagespeed")}
      subtitle={t("set.psi.subtitle")}
      id="pagespeed"
    >
      <Row
        label={t("set.psi.key")}
        help={t("set.psi.key_help")}
      >
        <div className="flex gap-2">
          <input
            type="password"
            className="w-full rounded border border-[var(--color-border)] bg-transparent px-2 py-1 text-[12px]"
            placeholder={stored ? t("common.stored") : t("set.psi.no_key")}
            value={key}
            onChange={(e) => setKey(e.target.value)}
            spellCheck={false}
          />
          <button
            type="button"
            className="shrink-0 rounded border border-[var(--color-border)] px-2 py-1 text-[12px]"
            onClick={() => {
              void setPagespeedKey(key)
                .then(() => {
                  setKey("");
                  setNote(key.trim() ? t("set.psi.saved") : t("set.psi.removed"));
                  return getPagespeedKey().then(setStored);
                })
                .catch((e) => setNote(String(e)));
            }}
          >
            {t("common.save")}
          </button>
        </div>
      </Row>
      {note && <p className="mt-2 text-[11px] text-[var(--color-muted)]">{note}</p>}
      <p className="mt-3 text-[11px] leading-snug text-[var(--color-muted)]">
        <InlineMd text={t("set.psi.ip_note")} />
      </p>
    </Section>
  );
}

function RepositoriesSection() {
  const t = useT();
  const [cfg, setCfg] = useState<RepoConfig | null>(null);
  const [dir, setDir] = useState("");
  const [token, setToken] = useState("");
  const [note, setNote] = useState<string | null>(null);
  const load = () =>
    getRepoConfig()
      .then((c) => {
        setCfg(c);
        setDir(c.clone_dir);
      })
      .catch(() => {});
  useEffect(() => {
    void load();
  }, []);
  return (
    <Section
      icon={<GitBranch size={16} className="text-[var(--color-accent)]" />}
      title={t("settings.section.repos")}
      subtitle={t("set.repos.subtitle")}
      id="repos"
    >
      <Row label={t("set.repos.dir")} help={t("set.repos.dir_help")}>
        <div className="flex gap-2">
          <input
            className="w-full rounded border border-[var(--color-border)] bg-transparent px-2 py-1 font-[var(--font-mono)] text-[12px]"
            value={dir}
            onChange={(e) => setDir(e.target.value)}
            spellCheck={false}
          />
          <button
            type="button"
            className="shrink-0 rounded border border-[var(--color-border)] px-2 py-1 text-[12px]"
            onClick={() => {
              void setRepoCloneDir(dir)
                .then(() => {
                  setNote(t("set.repos.dir_saved"));
                  return load();
                })
                .catch((e) => setNote(String(e)));
            }}
          >
            {t("common.save")}
          </button>
        </div>
      </Row>
      <Row
        label={t("set.repos.token")}
        help={
          cfg?.gh_available
            ? t("set.repos.gh_help")
            : t("set.repos.token_help")
        }
      >
        <div className="flex gap-2">
          <input
            type="password"
            className="w-full rounded border border-[var(--color-border)] bg-transparent px-2 py-1 text-[12px]"
            placeholder={cfg?.has_token ? t("common.stored") : t("set.repos.no_token")}
            value={token}
            onChange={(e) => setToken(e.target.value)}
            spellCheck={false}
          />
          <button
            type="button"
            className="shrink-0 rounded border border-[var(--color-border)] px-2 py-1 text-[12px]"
            onClick={() => {
              void setGithubToken(token)
                .then(() => {
                  setNote(token.trim() ? t("set.token_saved") : t("set.token_removed"));
                  setToken("");
                  return load();
                })
                .catch((e) => setNote(String(e)));
            }}
          >
            {t("common.save")}
          </button>
        </div>
      </Row>
      {note && <p className="mt-2 text-[11px] text-[var(--color-muted)]">{note}</p>}
    </Section>
  );
}

/** Settings → GitHub: connection, refresh policy, private repositories and
 *  (under "Advanced") a personal access token. No token is ever shown. */
function GithubSettingsSection() {
  const t = useT();
  const [st, setSt] = useState<GithubStatus | null>(null);
  const [pat, setPat] = useState("");
  const [note, setNote] = useState<string | null>(null);
  const [advanced, setAdvanced] = useState(false);
  const load = () => {
    void githubStatus().then(setSt).catch(() => {});
  };
  useEffect(() => {
    load();
    let un: UnlistenFn | undefined;
    let cancelled = false;
    void listen("github-changed", load).then((u) => (cancelled ? u() : (un = u)));
    return () => {
      cancelled = true;
      un?.();
    };
  }, []);
  if (!st) return null;
  const cfg = st.config;
  const save = (next: GithubConfig) => {
    void setGithubConfig(next)
      .then(load)
      .catch((e) => setNote(String(e)));
  };
  const dev = st.device;
  return (
    <Section
      icon={<GitBranch size={16} className="text-[var(--color-accent)]" />}
      title={t("settings.section.github")}
      subtitle={t("set.gh.subtitle")}
      id="github"
    >
      <Row
        label={t("set.gh.connection")}
        help={
          st.connected
            ? `${GITHUB_SOURCE_KEY[st.source ?? "oauth"] ? t(GITHUB_SOURCE_KEY[st.source ?? "oauth"]) : ""}${st.last_ok_ms ? " · " + t("set.gh.last_updated", { time: new Date(st.last_ok_ms).toLocaleString(locale()) }) : ""}`
            : st.client_id_configured
              ? t("set.gh.connect_help")
              : t("set.gh.no_client")
        }
      >
        <div className="flex flex-wrap items-center gap-2 text-[12px]">
          <span data-testid="gh-settings-state">
            {st.connected ? t("set.gh.connected_as", { login: st.login ?? "…" }) : t("set.gh.not_connected")}
          </span>
          {st.connected && st.source === "oauth" && (
            <button
              type="button"
              className="rounded border border-[var(--color-border)] px-2 py-1"
              onClick={() => void githubDisconnect().then(load)}
            >
              {t("set.gh.disconnect")}
            </button>
          )}
          {!st.connected && st.client_id_configured && dev.state !== "waiting" && (
            <button
              type="button"
              className="rounded bg-[var(--color-accent)] px-2 py-1 text-[var(--color-accent-fg)]"
              onClick={() => void githubDeviceStart().then(load).catch((e) => setNote(String(e)))}
            >
              {t("set.gh.connect")}
            </button>
          )}
          {dev.state === "waiting" && (
            <span className="flex items-center gap-2">
              {t("set.gh.code")} <span className="font-mono text-[14px] tracking-widest">{dev.user_code}</span>
              <button
                type="button"
                className="rounded border border-[var(--color-border)] px-2 py-1"
                onClick={() => void openUrl(dev.verification_uri)}
              >
                {t("set.gh.open_github")}
              </button>
              <button type="button" className="text-[var(--color-muted)]" onClick={() => void githubDeviceCancel()}>
                {t("common.cancel")}
              </button>
            </span>
          )}
          {st.connected && st.source === "gh" && (
            <span className="text-[var(--color-muted)]">{t("set.gh.gh_logout")}</span>
          )}
        </div>
      </Row>
      <Row label={t("set.gh.auto")} help={t("set.gh.auto_help")}>
        <div className="flex flex-wrap items-center gap-3 text-[12px]">
          <label className="flex items-center gap-2">
            <input
              type="checkbox"
              checked={cfg.auto_refresh}
              onChange={(e) => save({ ...cfg, auto_refresh: e.target.checked })}
              className="accent-[var(--color-accent)]"
            />
            {t("common.on")}
          </label>
          <label className="flex items-center gap-2">
            {t("set.gh.interval")}
            <select
              value={cfg.interval_h}
              onChange={(e) => save({ ...cfg, interval_h: Number(e.target.value) })}
              className="rounded border border-[var(--color-border)] bg-transparent px-1 py-0.5"
            >
              {[1, 3, 6, 24].map((h) => (
                <option key={h} value={h}>
                  {h === 24 ? t("set.gh.daily") : `${h} h`}
                </option>
              ))}
            </select>
          </label>
        </div>
      </Row>
      <Row
        label={t("set.gh.private")}
        help={t("set.gh.private_help")}
      >
        <label className="flex items-center gap-2 text-[12px]">
          <input
            type="checkbox"
            checked={cfg.private_repos}
            onChange={(e) => save({ ...cfg, private_repos: e.target.checked })}
            className="accent-[var(--color-accent)]"
          />
          {t("set.gh.private_ask")}
        </label>
      </Row>
      {st.rate && (
        <p className="mb-3 text-[11px] text-[var(--color-muted)]">
          {t("set.gh.rate", { remaining: st.rate.remaining, limit: st.rate.limit, time: new Date(st.rate.reset_at_ms).toLocaleTimeString(locale()) })}
          {st.scopes != null && ` · ${t("set.gh.scopes", { scopes: st.scopes || t("set.gh.public_only") })}`}
        </p>
      )}
      <button type="button" className="text-[11px] text-[var(--color-accent)]" onClick={() => setAdvanced((a) => !a)}>
        {advanced ? t("set.gh.hide_advanced") : t("set.gh.advanced")}
      </button>
      {advanced && (
        <Row
          label={t("set.gh.pat")}
          help={t("set.gh.pat_help")}
        >
          <div className="flex gap-2">
            <input
              type="password"
              className="w-full rounded border border-[var(--color-border)] bg-transparent px-2 py-1 text-[12px]"
              placeholder={st.pat_stored ? t("common.stored") : t("set.repos.no_token")}
              value={pat}
              onChange={(e) => setPat(e.target.value)}
              spellCheck={false}
              autoComplete="off"
            />
            <button
              type="button"
              className="shrink-0 rounded border border-[var(--color-border)] px-2 py-1 text-[12px]"
              onClick={() => {
                void setGithubToken(pat)
                  .then(() => {
                    setNote(pat.trim() ? t("set.token_saved") : t("set.token_removed"));
                    setPat("");
                    load();
                    if (pat.trim()) void githubRefresh(true).catch(() => {});
                  })
                  .catch((e) => setNote(String(e)));
              }}
            >
              {t("common.save")}
            </button>
          </div>
        </Row>
      )}
      {note && <p className="mt-2 text-[11px] text-[var(--color-muted)]">{note}</p>}
    </Section>
  );
}

const GITHUB_SOURCE_KEY: Record<string, MessageKey> = {
  oauth: "set.gh.src_oauth",
  pat: "set.gh.pat",
  gh: "set.gh.src_gh",
};

function DeviceSyncSection() {
  const t = useT();
  const [cfg, setCfg] = useState<DeviceSyncConfig | null>(null);
  const [status, setStatus] = useState<DeviceSyncStatus | null>(null);
  const [pass, setPass] = useState("");
  const [busy, setBusy] = useState(false);
  const [note, setNote] = useState<string | null>(null);

  const refresh = () => {
    void getDeviceSyncStatus().then(setStatus).catch(() => {});
  };
  useEffect(() => {
    void getDeviceSyncConfig().then(setCfg).catch(() => {});
    refresh();
  }, []);

  const save = (next: DeviceSyncConfig) => {
    setCfg(next);
    void setDeviceSyncConfig(next).then(refresh).catch(() => {});
  };

  if (!cfg) return null;
  const ready = cfg.enabled && !!status?.has_passphrase && cfg.folder.trim().length > 0;

  return (
    <Section
      icon={<Cloud size={16} className="text-[var(--color-accent)]" />}
      title={t("settings.section.device-sync")}
      subtitle={t("set.ds.subtitle")}
      id="device-sync"
    >
      <Row label={t("set.active")} help={t("set.active_help")}>
        <input
          type="checkbox"
          checked={cfg.enabled}
          onChange={(e) => save({ ...cfg, enabled: e.target.checked })}
        />
      </Row>

      <Row label={t("set.ds.folder")} help={t("set.ds.folder_help")}>
        <input
          className="w-full rounded border border-[var(--color-border)] bg-transparent px-2 py-1 text-[12px]"
          value={cfg.folder}
          onChange={(e) => setCfg({ ...cfg, folder: e.target.value })}
          onBlur={() => save(cfg)}
          spellCheck={false}
        />
      </Row>

      <Row
        label={t("common.password")}
        help={t("set.ds.pass_help")}
      >
        <div className="flex gap-2">
          <input
            type="password"
            className="w-full rounded border border-[var(--color-border)] bg-transparent px-2 py-1 text-[12px]"
            placeholder={status?.has_passphrase ? t("common.stored") : t("set.ds.none_yet")}
            value={pass}
            onChange={(e) => setPass(e.target.value)}
          />
          <button
            type="button"
            className="shrink-0 rounded border border-[var(--color-border)] px-2 py-1 text-[12px]"
            onClick={() => {
              void setDeviceSyncPassphrase(pass).then(() => {
                setPass("");
                refresh();
              });
            }}
          >
            {t("common.save")}
          </button>
        </div>
      </Row>

      <Row
        label={t("set.ds.totp")}
        help={t("set.ds.totp_help")}
      >
        <input
          type="checkbox"
          checked={cfg.include_totp}
          onChange={(e) => save({ ...cfg, include_totp: e.target.checked })}
        />
      </Row>

      <div className="mt-3 flex items-center gap-3 text-[11px] text-[var(--color-muted)]">
        <span>{status?.folder_ok ? t("set.ds.folder_ok") : t("set.ds.folder_missing")}</span>
        <span>{status?.has_passphrase ? t("set.ds.pass_set") : t("set.ds.no_pass")}</span>
        <span>{status ? tn("set.ds.peers", status.peers) : ""}</span>
        <button
          type="button"
          disabled={!ready || busy}
          className="ml-auto rounded border border-[var(--color-border)] px-2 py-1 disabled:opacity-40"
          onClick={() => {
            setBusy(true);
            deviceSyncNow()
              .then((s) => {
                setNote(
                  s.skipped.length
                    ? s.skipped[0]
                    : t("set.ds.applied", { clips: s.clips, snippets: s.snippets, notes: s.notes }),
                );
                refresh();
              })
              .catch((e) => setNote(String(e)))
              .finally(() => setBusy(false));
          }}
        >
          {busy ? t("common.running") : t("set.ds.sync_now")}
        </button>
      </div>
      {note && <p className="mt-2 text-[11px] text-[var(--color-muted)]">{note}</p>}
      {status?.last_error && (
        <p className="mt-1 text-[11px] text-amber-500">{status.last_error}</p>
      )}
      <p className="mt-3 text-[11px] text-[var(--color-muted)]">
        {t("set.ds.no_deletes")}
      </p>
    </Section>
  );
}


function AutoBackupSection({ onRestored }: { onRestored?: () => Promise<void> | void }) {
  const t = useT();
  const [cfg, setCfg] = useState<AutoBackupConfig | null>(null);
  const [status, setStatus] = useState<AutoBackupStatus | null>(null);
  const [pass, setPass] = useState("");
  const [busy, setBusy] = useState(false);
  const [note, setNote] = useState<string | null>(null);
  const [restoreOpen, setRestoreOpen] = useState(false);
  const [snapshots, setSnapshots] = useState<AutoBackupSnapshot[] | null>(null);
  const [restorePw, setRestorePw] = useState("");
  const [replaceMode, setReplaceMode] = useState(false);

  const refresh = () => {
    void getAutoBackupStatus().then(setStatus).catch(() => {});
  };
  useEffect(() => {
    void getAutoBackupConfig().then(setCfg).catch(() => {});
    refresh();
    // Unmount-before-resolve guard (SettingsPanel remounts on every tab
    // switch; a bare `.then(u => unlisten = u)` orphaned the listener).
    let cancelled = false;
    let unlisten: UnlistenFn | undefined;
    void listen("auto-backup-status-changed", refresh).then((u) => {
      if (cancelled) u();
      else unlisten = u;
    });
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, []);

  const save = (next: AutoBackupConfig) => {
    setCfg(next);
    void setAutoBackupConfig(next).then(refresh).catch(() => {});
  };

  const pickFolder = async () => {
    if (!cfg) return;
    const path = await openDialog({ directory: true, multiple: false });
    if (typeof path === "string") save({ ...cfg, folder: path });
  };

  const loadSnapshots = () => {
    void autoBackupListSnapshots().then(setSnapshots).catch(() => setSnapshots([]));
  };

  const doRestore = async (snap: AutoBackupSnapshot) => {
    if (replaceMode) {
      const ok = await confirmDialog(
        t("set.ab.replace_confirm"),
        t("set.ab.restore_title"),
      );
      if (!ok) return;
    }
    setBusy(true);
    setNote(null);
    try {
      const r = await autoBackupRestore(snap.path, snap.encrypted ? restorePw : null, replaceMode);
      setNote(
        t("set.ab.restored", { clips: r.history_imported, snippets: r.snippets_imported, notes: r.notes_imported }),
      );
      setRestorePw("");
      if (onRestored) await onRestored();
    } catch (e) {
      setNote(String(e));
    } finally {
      setBusy(false);
    }
  };

  if (!cfg) return null;
  const ready =
    cfg.enabled && cfg.folder.trim().length > 0 && (!cfg.encrypt || !!status?.has_password);

  return (
    <Section
      icon={<HardDrive size={16} className="text-[var(--color-accent)]" />}
      title={t("settings.section.auto-backup")}
      subtitle={t("set.ab.subtitle")}
      id="auto-backup"
    >
      <Row label={t("set.active")} help={t("set.active_help")}>
        <input
          type="checkbox"
          checked={cfg.enabled}
          onChange={(e) => save({ ...cfg, enabled: e.target.checked })}
        />
      </Row>

      <Row label={t("set.ab.folder")} help={t("set.ab.folder_help")}>
        <div className="flex gap-2">
          <input
            className="w-full rounded border border-[var(--color-border)] bg-transparent px-2 py-1 text-[12px]"
            value={cfg.folder}
            onChange={(e) => setCfg({ ...cfg, folder: e.target.value })}
            onBlur={() => save(cfg)}
            spellCheck={false}
            placeholder="/Users/…/Google Drive/InspectorRust"
          />
          <button
            type="button"
            className="shrink-0 rounded border border-[var(--color-border)] px-2 py-1 text-[12px]"
            onClick={() => void pickFolder()}
          >
            {t("common.choose")}
          </button>
        </div>
      </Row>

      <div className="flex gap-3">
        <Row label={t("set.ab.interval")} help={t("set.ab.interval_help")}>
          <input
            type="number"
            min={1}
            className="w-24 rounded border border-[var(--color-border)] bg-transparent px-2 py-1 text-[12px]"
            value={cfg.interval_min}
            onChange={(e) => setCfg({ ...cfg, interval_min: Math.max(1, Number(e.target.value) || 1) })}
            onBlur={() => save(cfg)}
          />
        </Row>
        <Row label={t("set.ab.keep")} help={t("set.ab.keep_help")}>
          <input
            type="number"
            min={1}
            className="w-24 rounded border border-[var(--color-border)] bg-transparent px-2 py-1 text-[12px]"
            value={cfg.keep}
            onChange={(e) => setCfg({ ...cfg, keep: Math.max(1, Number(e.target.value) || 1) })}
            onBlur={() => save(cfg)}
          />
        </Row>
      </div>

      <Row label={t("set.ab.encrypt")} help={t("set.ab.encrypt_help")}>
        <input
          type="checkbox"
          checked={cfg.encrypt}
          onChange={(e) => save({ ...cfg, encrypt: e.target.checked })}
        />
      </Row>

      {cfg.encrypt && (
        <Row
          label={t("set.ab.password")}
          help={t("set.ab.password_help")}
        >
          <div className="flex gap-2">
            <input
              type="password"
              className="w-full rounded border border-[var(--color-border)] bg-transparent px-2 py-1 text-[12px]"
              placeholder={status?.has_password ? t("common.stored") : t("set.ds.none_yet")}
              value={pass}
              onChange={(e) => setPass(e.target.value)}
            />
            <button
              type="button"
              className="shrink-0 rounded border border-[var(--color-border)] px-2 py-1 text-[12px]"
              onClick={() => void setAutoBackupPassword(pass).then(() => { setPass(""); refresh(); })}
            >
              {t("common.save")}
            </button>
          </div>
        </Row>
      )}

      <Row label={t("set.ab.history")} help={t("set.ab.history_help")}>
        <input
          type="checkbox"
          checked={cfg.include_history}
          onChange={(e) => save({ ...cfg, include_history: e.target.checked })}
        />
      </Row>

      <Row label={t("set.ab.timesheet")} help={t("set.ab.timesheet_help")}>
        <input
          type="checkbox"
          checked={cfg.include_timesheet}
          onChange={(e) => save({ ...cfg, include_timesheet: e.target.checked })}
        />
      </Row>

      {!cfg.encrypt && (
        <p className="mb-4 rounded border border-amber-500/50 bg-amber-500/10 p-2 text-[11px] text-amber-600">
          {t("set.ab.totp_warn")}
        </p>
      )}

      <div className="mt-3 flex flex-wrap items-center gap-3 text-[11px] text-[var(--color-muted)]">
        <span>{status?.folder_ok ? t("set.ds.folder_ok") : t("set.ds.folder_missing")}</span>
        {cfg.encrypt && <span>{status?.has_password ? t("set.ds.pass_set") : t("set.ds.no_pass")}</span>}
        <span>{status ? tn("set.ab.snapshots", status.snapshot_count) : ""}</span>
        {status && status.last_ms > 0 && <span>{t("set.ab.last", { time: new Date(status.last_ms).toLocaleString(locale()) })}</span>}
        <button
          type="button"
          disabled={!ready || busy}
          className="ml-auto rounded border border-[var(--color-border)] px-2 py-1 disabled:opacity-40"
          onClick={() => {
            setBusy(true);
            setNote(null);
            autoBackupNow()
              .then((o) => {
                setNote(
                  o.wrote
                    ? t(o.pruned ? "set.ab.saved_pruned" : "set.ab.saved", { size: formatBytes(o.bytes), n: o.pruned })
                    : t("set.ab.unchanged"),
                );
                refresh();
              })
              .catch((e) => setNote(String(e)))
              .finally(() => setBusy(false));
          }}
        >
          {busy ? t("common.running") : t("set.ab.now")}
        </button>
        <button
          type="button"
          className="rounded border border-[var(--color-border)] px-2 py-1"
          onClick={() => {
            const open = !restoreOpen;
            setRestoreOpen(open);
            if (open) loadSnapshots();
          }}
        >
          {restoreOpen ? t("common.close") : t("set.ab.restore")}
        </button>
      </div>

      {restoreOpen && (
        <div className="mt-3 rounded border border-[var(--color-border)] p-3">
          <div className="mb-2 flex items-center gap-3 text-[11px]">
            <span className="font-medium">{t("set.ab.restore_head")}</span>
            <label className="flex items-center gap-1">
              <input type="radio" checked={!replaceMode} onChange={() => setReplaceMode(false)} /> {t("set.ab.merge")}
            </label>
            <label className="flex items-center gap-1">
              <input type="radio" checked={replaceMode} onChange={() => setReplaceMode(true)} /> {t("set.ab.replace")}
            </label>
          </div>
          <p className="mb-2 text-[11px] text-[var(--color-muted)]">
            {t("set.ab.merge_help")}
          </p>
          {snapshots === null ? (
            <p className="text-[11px] text-[var(--color-muted)]">{t("common.loading")}</p>
          ) : snapshots.length === 0 ? (
            <p className="text-[11px] text-[var(--color-muted)]">{t("set.ab.none_found")}</p>
          ) : (
            <div className="flex max-h-[220px] flex-col gap-1 overflow-y-auto">
              {snapshots.map((s) => (
                <div key={s.path} className="flex items-center gap-2 text-[11px]">
                  {s.encrypted && <Lock size={11} className="shrink-0 text-[var(--color-muted)]" />}
                  <span className="min-w-0 flex-1 truncate">{new Date(s.ts_ms).toLocaleString(locale())}</span>
                  <span className="shrink-0 tabular-nums text-[var(--color-muted)]">{formatBytes(s.bytes)}</span>
                  {s.encrypted && (
                    <input
                      type="password"
                      placeholder={t("common.password")}
                      className="w-24 shrink-0 rounded border border-[var(--color-border)] bg-transparent px-1 py-0.5"
                      value={restorePw}
                      onChange={(e) => setRestorePw(e.target.value)}
                    />
                  )}
                  <button
                    type="button"
                    disabled={busy || (s.encrypted && restorePw.length === 0)}
                    className="shrink-0 rounded border border-[var(--color-border)] px-2 py-0.5 disabled:opacity-40"
                    onClick={() => void doRestore(s)}
                  >
                    {t("set.ab.apply")}
                  </button>
                </div>
              ))}
            </div>
          )}
        </div>
      )}

      {note && <p className="mt-2 text-[11px] text-[var(--color-muted)]">{note}</p>}
      {status?.last_error && <p className="mt-1 text-[11px] text-amber-500">{status.last_error}</p>}
    </Section>
  );
}

function CloudSyncSection() {
  const t = useT();
  const [cfg, setCfgState] = useState<SyncConfig | null>(null);
  const [status, setStatus] = useState<SyncStatus | null>(null);
  const [showToken, setShowToken] = useState(false);
  const [syncing, setSyncing] = useState(false);
  // Live probe result (server reachable / token valid) — null = not yet
  // probed, "probing" while the async check runs.
  const [probe, setProbe] = useState<SyncProbe | null>(null);
  const [probing, setProbing] = useState(false);

  const refreshStatus = () => {
    void getSyncStatus().then(setStatus).catch(() => {});
  };
  const runProbe = () => {
    setProbing(true);
    syncTestConnection()
      .then(setProbe)
      .catch(() => setProbe(null))
      .finally(() => setProbing(false));
  };
  useEffect(() => {
    void getSyncConfig().then((c) => {
      setCfgState(c);
      // Auto-probe on open when sync is configured — the checkmarks show the
      // live truth without the user having to press anything.
      if (c.enabled && c.token.trim() !== "") runProbe();
    }).catch(() => {});
    refreshStatus();
    // The worker emits after EVERY cycle (success or failure) → chips update
    // live instead of blind-polling.
    let cancelled = false;
    let unlisten: UnlistenFn | undefined;
    void listen("sync-status-changed", () => {
      refreshStatus();
      setSyncing(false);
    }).then((u) => {
      if (cancelled) u();
      else unlisten = u;
    });
    return () => {
      cancelled = true;
      unlisten?.();
    };
     
  }, []);

  const save = async (patch: Partial<SyncConfig>) => {
    if (!cfg) return;
    const next = { ...cfg, ...patch };
    setCfgState(next);
    try {
      const applied = await setSyncConfig(next);
      setCfgState(applied);
      // URL/token changed → the old probe verdict is stale; re-check.
      if (applied.enabled && applied.token.trim() !== "") runProbe();
      else setProbe(null);
    } catch {
      /* keep the optimistic state; the next load reconciles */
    }
  };

  const runNow = async () => {
    setSyncing(true);
    try {
      await syncNow();
    } catch {
      setSyncing(false);
    }
    // Completion arrives via the sync-status-changed event; this timeout is
    // only the safety net if the worker never answers.
    window.setTimeout(() => setSyncing(false), 20000);
  };

  const lastLabel =
    status && status.last_ms > 0
      ? new Date(status.last_ms).toLocaleString(locale())
      : t("set.sync.never");

  return (
    <Section
      icon={<Cloud size={16} className="text-[var(--color-accent)]" />}
      id="cloud-sync"
      title={t("settings.section.cloud-sync")}
      subtitle={t("set.sync.subtitle")}
    >
      {cfg === null ? (
        <p className="text-[12px] text-[var(--color-muted)]">{t("common.loading")}</p>
      ) : (
        <div className="flex flex-col gap-3">
          <label className="flex cursor-pointer items-center gap-2 text-[12px]">
            <input
              type="checkbox"
              checked={cfg.enabled}
              onChange={(e) => void save({ enabled: e.target.checked })}
              className="accent-[var(--color-accent)]"
            />
            <span className="font-medium">{t("set.sync.enable")}</span>
            <span className="text-[var(--color-muted)]">{t("set.sync.enable_help")}</span>
          </label>

          <Row label={t("set.sync.url")}>
            <input
              type="text"
              value={cfg.url}
              placeholder="https://cue.celox.io"
              onChange={(e) => setCfgState({ ...cfg, url: e.target.value })}
              onBlur={(e) => void save({ url: e.target.value })}
              className="w-full rounded border border-[var(--color-border)] bg-transparent px-2 py-1.5 text-[12px] outline-none focus:border-[var(--color-accent)]"
            />
          </Row>

          <Row
            label={t("set.sync.token")}
            help={t("set.sync.token_help")}
          >
            <div className="flex items-center gap-2">
              <input
                type={showToken ? "text" : "password"}
                value={cfg.token}
                placeholder={t("set.sync.token_ph")}
                onChange={(e) => setCfgState({ ...cfg, token: e.target.value })}
                onBlur={(e) => void save({ token: e.target.value })}
                className="w-full rounded border border-[var(--color-border)] bg-transparent px-2 py-1.5 font-mono text-[11px] outline-none focus:border-[var(--color-accent)]"
              />
              <button
                type="button"
                onClick={() => setShowToken((v) => !v)}
                className="shrink-0 rounded border border-[var(--color-border)] px-2 py-1.5 text-[11px] text-[var(--color-muted)] hover:text-[var(--color-fg)]"
              >
                {showToken ? t("common.hide") : t("common.show")}
              </button>
            </div>
          </Row>

          {/* Live status chips: server reachable · token valid · last sync. */}
          <div className="flex flex-wrap items-center gap-2">
            <SyncChip
              state={probing ? "busy" : probe === null ? "idle" : probe.reachable ? "ok" : "fail"}
              okLabel={t("set.sync.server_ok")}
              failLabel={t("set.sync.server_fail")}
              idleLabel={t("set.sync.server_idle")}
            />
            <SyncChip
              state={
                probing
                  ? "busy"
                  : probe === null || !probe.reachable
                    ? "idle"
                    : probe.authorized
                      ? "ok"
                      : "fail"
              }
              okLabel={t("set.sync.token_ok")}
              failLabel={t("set.sync.token_fail")}
              idleLabel={t("set.sync.token_idle")}
            />
            <SyncChip
              state={
                syncing
                  ? "busy"
                  : status === null || status.last_ms === 0
                    ? "idle"
                    : status.last_error === ""
                      ? "ok"
                      : "fail"
              }
              okLabel={t("set.sync.last", { time: lastLabel })}
              failLabel={t("set.sync.last_fail")}
              idleLabel={t("set.sync.never_synced")}
            />
          </div>

          <div className="flex items-center gap-3">
            <button
              type="button"
              disabled={!cfg.enabled || cfg.token.trim() === "" || syncing}
              onClick={() => void runNow()}
              className="rounded border border-[var(--color-border)] px-3 py-1.5 text-[12px] font-medium enabled:hover:border-[var(--color-accent)] disabled:opacity-50"
            >
              {syncing ? t("set.sync.syncing") : t("set.sync.now")}
            </button>
            <button
              type="button"
              disabled={probing || cfg.url.trim() === ""}
              onClick={runProbe}
              className="rounded border border-[var(--color-border)] px-3 py-1.5 text-[12px] font-medium enabled:hover:border-[var(--color-accent)] disabled:opacity-50"
            >
              {probing ? t("set.sync.checking") : t("set.sync.test")}
            </button>
          </div>
          {probe !== null && probe.message !== "" && !probing && (
            <div className="flex items-center gap-2 rounded border border-amber-500/40 bg-amber-500/10 px-3 py-2 text-[12px]">
              <AlertTriangle size={14} className="shrink-0 text-amber-500" />
              <span>{probe.message}</span>
            </div>
          )}
          {status && status.last_error !== "" && (
            <div className="flex items-center gap-2 rounded border border-red-500/40 bg-red-500/10 px-3 py-2 text-[12px]">
              <AlertTriangle size={14} className="shrink-0 text-red-500" />
              <span>{status.last_error}</span>
            </div>
          )}
        </div>
      )}
    </Section>
  );
}

/** One ✓/✗/…-chip for the Cloud-Sync status row. `idle` = grey (not yet
 *  checked), `busy` = pulsing while a probe/sync runs. */
function SyncChip({
  state,
  okLabel,
  failLabel,
  idleLabel,
}: {
  state: "ok" | "fail" | "idle" | "busy";
  okLabel: string;
  failLabel: string;
  idleLabel: string;
}) {
  const t = useT();
  const styles: Record<typeof state, string> = {
    ok: "border-emerald-500/40 bg-emerald-500/10 text-emerald-500",
    fail: "border-red-500/40 bg-red-500/10 text-red-500",
    idle: "border-[var(--color-border)] text-[var(--color-muted)]",
    busy: "border-[var(--color-border)] text-[var(--color-muted)] animate-pulse",
  };
  const label =
    state === "ok" ? okLabel : state === "fail" ? failLabel : state === "busy" ? t("set.sync.checking") : idleLabel;
  return (
    <span
      className={
        "inline-flex items-center gap-1.5 rounded-full border px-2.5 py-1 text-[11px] font-medium " +
        styles[state]
      }
    >
      {state === "ok" ? (
        <CheckCircle2 size={13} className="shrink-0" />
      ) : state === "fail" ? (
        <AlertTriangle size={13} className="shrink-0" />
      ) : (
        <span className="inline-block h-2 w-2 shrink-0 rounded-full bg-current opacity-60" />
      )}
      {label}
    </span>
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
  const t = useT();
  const cmd = IS_MAC ? "⌘" : "Ctrl";
  const shift = IS_MAC ? "⇧" : "Shift";
  const alt = IS_MAC ? "⌥" : "Alt";
  const join = IS_MAC ? "" : "+";
  const k = (...parts: string[]) => parts.join(join);

  const groups: Array<{ heading: string; rows: Array<[string, string, string?]> }> = [
    {
      heading: t("set.keys.global"),
      rows: [
        [IS_MAC ? "⌃Space" : "Ctrl+Space", t("set.keys.open_popup"), t("set.keys.configurable")],
        [k("Ctrl", shift, "V"), t("set.keys.open_history"), t("set.keys.configurable")],
        [
          k("Ctrl", shift, "O"),
          t("set.keys.ocr"),
          IS_MAC ? t("set.keys.ocr_hint") : t("set.keys.mac_only"),
        ],
        [
          k("Ctrl", shift, "S"),
          t("set.keys.shot"),
          IS_MAC ? t("set.keys.shot_hint") : t("set.keys.mac_only"),
        ],
        [k("Ctrl", shift, "C"), t("set.keys.color"), t("set.keys.color_hint")],
        [k(alt, "1"), t("set.keys.expander"), t("set.keys.expander_hint")],
      ],
    },
    {
      heading: t("set.keys.nav"),
      rows: [
        ["⏎", t("set.keys.paste"), t("set.keys.paste_hint")],
        [k(shift, "⏎"), t("set.keys.paste_fmt"), t("set.keys.paste_fmt_hint")],
        ["↑ / ↓", t("set.keys.navigate")],
        [k(shift, "↑ / ↓"), t("set.keys.volume"), t("set.keys.volume_hint")],
        ["Esc", t("set.keys.close")],
      ],
    },
    {
      heading: t("set.keys.image"),
      rows: [
        [k(cmd, "B"), t("set.keys.cutout"), t("set.keys.cutout_hint")],
        [k(cmd, "S"), t("set.keys.save_image"), t("set.keys.save_image_hint")],
      ],
    },
    {
      heading: t("set.keys.text"),
      rows: [
        [`${cmd}1 … ${cmd}9`, t("set.keys.transform"), t("set.keys.transform_hint")],
        [k(cmd, "1"), t("set.keys.vowels")],
        [k(cmd, "2"), t("set.keys.upper"), t("set.keys.upper_hint", { k3: `${cmd}3`, k4: `${cmd}4` })],
        [k(cmd, "5"), "camelCase", `${cmd}6 snake_case · ${cmd}7 kebab-case`],
        [k(cmd, "8"), t("set.keys.base64"), t("set.keys.base64_hint", { k9: `${cmd}9` })],
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
  const t = useT();
  const [hotkey, setHotkey] = useState<string>("");
  const [defaultHotkey, setDefaultHotkey] = useState<string>("");
  const [stored, setStored] = useState<string>("");
  const [busy, setBusy] = useState(false);
  const [status, setStatus] = useState<{ kind: "ok" | "err"; message: string } | null>(
    null,
  );

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
      setStatus({ kind: "ok", message: t("set.popup_hk.armed", { hotkey: prettyHotkey(applied) }) });
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
        { label: t("set.default_suffix", { value: "⌃Space" }), value: "Ctrl+Space" },
        { label: "⌃⇧V", value: "Ctrl+Shift+KeyV" },
        { label: "⌘⇧Space", value: "Cmd+Shift+Space" },
        { label: "⌘J", value: "Cmd+KeyJ" },
      ]
    : IS_LINUX
      ? [
          { label: t("set.default_suffix", { value: "Ctrl+Space" }), value: "Ctrl+Space" },
          { label: "Ctrl+Shift+V", value: "Ctrl+Shift+KeyV" },
          { label: "Super+Space", value: "Super+Space" },
          { label: "Alt+Space", value: "Alt+Space" },
        ]
      : [
          { label: t("set.default_suffix", { value: "Ctrl+Space" }), value: "Ctrl+Space" },
          { label: "Ctrl+Shift+V", value: "Ctrl+Shift+KeyV" },
          { label: "Win+Shift+V", value: "Win+Shift+KeyV" },
          { label: "Alt+Space", value: "Alt+Space" },
        ];

  const platformModifier = IS_MAC ? "⌘" : IS_LINUX ? "Super" : "Win";

  return (
    <Section
      icon={<Keyboard size={16} className="text-[var(--color-accent)]" />}
      id="popup-hotkey"
      title={t("settings.section.popup-hotkey")}
      subtitle={t("set.popup_hk.subtitle")}
    >
      <Row
        label={t("set.expander.hotkey")}
        help={t("set.popup_hk.help", { modifier: platformModifier })}
      >
        <div className="flex flex-wrap items-center gap-2">
          <HotkeyCapture value={hotkey} onChange={setHotkey} disabled={busy} />
          <button
            onClick={() => defaultHotkey && setHotkey(defaultHotkey)}
            disabled={busy || !defaultHotkey || hotkey === defaultHotkey}
            className="rounded border border-[var(--color-border)] px-2 py-1 text-[11px] text-[var(--color-muted)] hover:border-[var(--color-accent)] hover:text-[var(--color-fg)] disabled:opacity-40"
            title={defaultHotkey ? t("set.reset_to", { value: prettyHotkey(defaultHotkey) }) : ""}
          >
            {t("common.reset")}
          </button>
          <span className="text-[11px] text-[var(--color-muted)]">{t("set.expander.presets")}</span>
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
          {busy ? t("common.saving") : t("common.save")}
        </button>
        {!dirty && stored && (
          <span className="text-[11px] text-[var(--color-muted)]">
            {t("set.popup_hk.current")}{" "}
            <code className="rounded bg-[var(--color-surface)] px-1 font-[var(--font-mono)]">
              {prettyHotkey(stored)}
            </code>
          </span>
        )}
      </div>

      {status && (
        <div
          className={
            // amber ↔ green flips ON this one element — the colour glide is
            // the Etappe-6 "the user is looking right now" moment.
            "mt-3 rounded border px-3 py-2 text-[12px] transition-colors duration-(--duration-slow) ease-sharp " +
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
  const t = useT();
  const [hotkey, setHotkey] = useState<string>("");
  const [defaultHotkey, setDefaultHotkey] = useState<string>("");
  const [stored, setStored] = useState<string>("");
  const [busy, setBusy] = useState(false);
  const [status, setStatus] = useState<{ kind: "ok" | "err"; message: string } | null>(
    null,
  );

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
          ? t("set.history_hk.armed", { hotkey: prettyHotkey(applied) })
          : t("set.history_hk.disabled"),
      });
    } catch (e) {
      setStatus({ kind: "err", message: String(e) });
    } finally {
      setBusy(false);
    }
  };

  const PRESETS: ReadonlyArray<{ label: string; value: string }> = IS_MAC
    ? [
        { label: t("set.default_suffix", { value: "⌃⇧V" }), value: "Ctrl+Shift+KeyV" },
        { label: "⌘⇧V", value: "Cmd+Shift+KeyV" },
        { label: "⌘⇧C", value: "Cmd+Shift+KeyC" },
      ]
    : [
        { label: t("set.default_suffix", { value: "Ctrl+Shift+V" }), value: "Ctrl+Shift+KeyV" },
        { label: "Ctrl+Shift+H", value: "Ctrl+Shift+KeyH" },
        { label: "Alt+V", value: "Alt+KeyV" },
      ];

  return (
    <Section
      icon={<Keyboard size={16} className="text-[var(--color-accent)]" />}
      id="history-hotkey"
      title={t("settings.section.history-hotkey")}
      subtitle={t("set.history_hk.subtitle")}
    >
      <Row
        label={t("set.expander.hotkey")}
        help={t("set.history_hk.help")}
      >
        <div className="flex flex-wrap items-center gap-2">
          <HotkeyCapture value={hotkey} onChange={setHotkey} disabled={busy} />
          <button
            onClick={() => setHotkey("")}
            disabled={busy || hotkey === ""}
            className="rounded border border-[var(--color-border)] px-2 py-1 text-[11px] text-[var(--color-muted)] hover:border-[var(--color-accent)] hover:text-[var(--color-fg)] disabled:opacity-40"
            title={t("set.history_hk.disable_title")}
          >
            {t("set.history_hk.disable")}
          </button>
          <button
            onClick={() => defaultHotkey && setHotkey(defaultHotkey)}
            disabled={busy || !defaultHotkey || hotkey === defaultHotkey}
            className="rounded border border-[var(--color-border)] px-2 py-1 text-[11px] text-[var(--color-muted)] hover:border-[var(--color-accent)] hover:text-[var(--color-fg)] disabled:opacity-40"
            title={defaultHotkey ? t("set.reset_to", { value: prettyHotkey(defaultHotkey) }) : ""}
          >
            {t("common.reset")}
          </button>
          <span className="text-[11px] text-[var(--color-muted)]">{t("set.expander.presets")}</span>
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
          {busy ? t("common.saving") : t("common.save")}
        </button>
        {!dirty && (
          <span className="text-[11px] text-[var(--color-muted)]">
            {stored.trim() ? (
              <>
                {t("set.popup_hk.current")}{" "}
                <code className="rounded bg-[var(--color-surface)] px-1 font-[var(--font-mono)]">
                  {prettyHotkey(stored)}
                </code>
              </>
            ) : (
              t("set.disabled")
            )}
          </span>
        )}
      </div>

      {status && (
        <div
          className={
            // amber ↔ green flips ON this one element — the colour glide is
            // the Etappe-6 "the user is looking right now" moment.
            "mt-3 rounded border px-3 py-2 text-[12px] transition-colors duration-(--duration-slow) ease-sharp " +
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
  const t = useT();
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
      Math.abs(saved.health_add - defs.health_add) > 1e-6 ||
      saved.kv_type !== defs.kv_type ||
      Math.abs(saved.pkv_monthly - defs.pkv_monthly) > 1e-6 ||
      saved.kv_sick_pay !== defs.kv_sick_pay ||
      saved.business_type !== defs.business_type ||
      saved.hebesatz !== defs.hebesatz ||
      saved.self_married !== defs.self_married);

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
      id="bruno"
      title={t("set.bruno.title")}
      subtitle={t("set.bruno.subtitle")}
    >
      <Row label={t("set.bruno.class")}>
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

      <Row
        label={t("set.bruno.state")}
        help={t("set.bruno.state_help")}
      >
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

      <Row
        label={t("set.bruno.children")}
        help={t("set.bruno.children_help")}
      >
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

      <Row label={t("set.bruno.church")}>
        <label className="flex cursor-pointer items-center gap-2 text-[12px]">
          <input
            type="checkbox"
            checked={defs.is_church_member}
            onChange={(e) => setDefs({ ...defs, is_church_member: e.target.checked })}
          />
          <span>{t("set.bruno.church_yes")}</span>
        </label>
      </Row>

      <Row
        label={t("set.bruno.health_add")}
        help={t("set.bruno.health_add_help")}
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

      <div className="mt-4 mb-1 text-[11px] font-semibold uppercase tracking-wide text-[var(--color-muted)]">
        <InlineMd text={t("set.bruno.self_head")} />
      </div>

      <Row
        label={t("set.bruno.business")}
        help={t("set.bruno.business_help")}
      >
        <div className="flex gap-1">
          {(["freiberufler", "gewerbe"] as const).map((b) => (
            <button
              key={b}
              onClick={() => setDefs({ ...defs, business_type: b })}
              className={
                "h-7 rounded px-3 text-[12px] font-medium " +
                (defs.business_type === b
                  ? "bg-[var(--color-accent)] text-[var(--color-accent-fg)]"
                  : "border border-[var(--color-border)] hover:bg-[var(--color-surface)]")
              }
            >
              {b === "freiberufler" ? t("set.bruno.freelance") : t("set.bruno.trade")}
            </button>
          ))}
        </div>
      </Row>

      {defs.business_type === "gewerbe" && (
        <Row
          label={t("set.bruno.hebesatz")}
          help={t("set.bruno.hebesatz_help")}
        >
          <input
            type="number"
            min={0}
            max={1200}
            step={5}
            value={defs.hebesatz}
            onChange={(e) =>
              setDefs({ ...defs, hebesatz: Math.max(0, parseInt(e.target.value, 10) || 0) })
            }
            className="w-24 rounded border border-[var(--color-border)] bg-[var(--color-bg)] px-2 py-1 text-[12px] tabular-nums"
          />
        </Row>
      )}

      <Row
        label={t("set.bruno.kv")}
        help={t("set.bruno.kv_help")}
      >
        <div className="flex gap-1">
          {(["gkv", "pkv"] as const).map((k) => (
            <button
              key={k}
              onClick={() => setDefs({ ...defs, kv_type: k })}
              className={
                "h-7 rounded px-3 text-[12px] font-medium " +
                (defs.kv_type === k
                  ? "bg-[var(--color-accent)] text-[var(--color-accent-fg)]"
                  : "border border-[var(--color-border)] hover:bg-[var(--color-surface)]")
              }
            >
              {k === "gkv" ? t("set.bruno.gkv") : t("set.bruno.pkv")}
            </button>
          ))}
        </div>
      </Row>

      {defs.kv_type === "gkv" ? (
        <Row
          label={t("set.bruno.sickpay")}
          help={t("set.bruno.sickpay_help")}
        >
          <label className="flex cursor-pointer items-center gap-2 text-[12px]">
            <input
              type="checkbox"
              checked={defs.kv_sick_pay}
              onChange={(e) => setDefs({ ...defs, kv_sick_pay: e.target.checked })}
            />
            <span>{t("set.bruno.sickpay_yes")}</span>
          </label>
        </Row>
      ) : (
        <Row label={t("set.bruno.pkv_amount")} help={t("set.bruno.pkv_amount_help")}>
          <input
            type="number"
            min={0}
            max={5000}
            step={10}
            value={defs.pkv_monthly}
            onChange={(e) =>
              setDefs({ ...defs, pkv_monthly: Math.max(0, parseFloat(e.target.value) || 0) })
            }
            className="w-24 rounded border border-[var(--color-border)] bg-[var(--color-bg)] px-2 py-1 text-[12px] tabular-nums"
          />
        </Row>
      )}

      <Row label={t("set.bruno.joint")} help={t("set.bruno.joint_help")}>
        <label className="flex cursor-pointer items-center gap-2 text-[12px]">
          <input
            type="checkbox"
            checked={defs.self_married}
            onChange={(e) => setDefs({ ...defs, self_married: e.target.checked })}
          />
          <span>{t("set.bruno.splitting")}</span>
        </label>
      </Row>

      <div className="mt-2 flex items-center gap-2">
        <button
          onClick={save}
          disabled={!dirty || savingBruno}
          className="rounded bg-[var(--color-accent)] px-3 py-1 text-[12px] font-medium text-[var(--color-accent-fg)] hover:opacity-90 disabled:opacity-40"
        >
          {savingBruno ? t("common.saving") : t("common.save")}
        </button>
        {dirty && !savingBruno && (
          <span className="text-[11px] text-[var(--color-muted)]">
            {t("common.unsaved")}
          </span>
        )}
      </div>
    </Section>
  );
}

// ── Faker defaults — fake-data generator ───────────────────────────────
// Persisted defaults applied to every `faker` call + `{faker:…}` snippet
// placeholder. Mirrors BrunoSection; the faker-defaults-changed event (fired
// by the setter) refreshes the popup without a restart.
function FakerSection() {
  const t = useT();
  const [defs, setDefs] = useState<FakerDefaults | null>(null);
  const [saved, setSaved] = useState<FakerDefaults | null>(null);
  const [locales, setLocales] = useState<FakerLocaleOption[]>([]);
  const [genNames, setGenNames] = useState<string[]>([]);
  const [saving, setSaving] = useState(false);

  useEffect(() => {
    fakerGetDefaults()
      .then((d) => {
        setDefs(d);
        setSaved(d);
      })
      .catch(() => undefined);
    fakerLocales().then(setLocales).catch(() => undefined);
    fakerCatalog()
      .then((c) => setGenNames(c.map((e) => e.name)))
      .catch(() => undefined);
  }, []);

  if (!defs) return null;

  const dirty =
    saved !== null &&
    (saved.locale !== defs.locale ||
      saved.count !== defs.count ||
      saved.format !== defs.format ||
      saved.save_history !== defs.save_history ||
      saved.pinned.join(",") !== defs.pinned.join(","));

  const save = async () => {
    setSaving(true);
    try {
      await fakerSetDefaults(defs);
      setSaved(defs);
    } catch (e) {
      console.error("save faker defaults", e);
    } finally {
      setSaving(false);
    }
  };

  const FORMATS: FakerDefaults["format"][] = ["plain", "json", "csv", "sql", "ts"];

  return (
    <Section
      icon={<Dices size={16} className="text-[var(--color-accent)]" />}
      id="faker"
      title={t("set.faker.title")}
      subtitle={t("set.faker.subtitle")}
    >
      <Row label={t("set.faker.locale")} help={t("set.faker.locale_help")}>
        <select
          value={defs.locale}
          onChange={(e) => setDefs({ ...defs, locale: e.target.value })}
          className="w-full rounded border border-[var(--color-border)] bg-[var(--color-bg)] px-2 py-1.5 text-[13px]"
        >
          {locales.map((l) => (
            <option key={l.code} value={l.code}>
              {l.label} ({l.code})
            </option>
          ))}
        </select>
      </Row>

      <Row label={t("set.faker.count")} help={t("set.faker.count_help")}>
        <input
          type="number"
          min={1}
          max={10000}
          value={defs.count}
          onChange={(e) =>
            setDefs({ ...defs, count: Math.max(1, Math.min(10000, parseInt(e.target.value, 10) || 1)) })
          }
          className="w-28 rounded border border-[var(--color-border)] bg-[var(--color-bg)] px-2 py-1.5 text-[13px]"
        />
      </Row>

      <Row label={t("set.faker.format")}>
        <div className="flex flex-wrap gap-1">
          {FORMATS.map((f) => (
            <button
              key={f}
              onClick={() => setDefs({ ...defs, format: f })}
              className={
                "rounded px-2.5 py-1 text-[12px] font-medium uppercase " +
                (defs.format === f
                  ? "bg-[var(--color-accent)] text-[var(--color-accent-fg)]"
                  : "bg-[var(--color-surface)] text-[var(--color-muted)]")
              }
            >
              {f}
            </button>
          ))}
        </div>
      </Row>

      <Row label={t("set.faker.pinned")} help={t("set.faker.pinned_help")}>
        <input
          type="text"
          value={defs.pinned.join(", ")}
          onChange={(e) =>
            setDefs({
              ...defs,
              pinned: e.target.value
                .split(",")
                .map((s) => s.trim().toLowerCase())
                .filter((s) => s && genNames.includes(s)),
            })
          }
          placeholder="person, email, uuid"
          className="w-full rounded border border-[var(--color-border)] bg-[var(--color-bg)] px-2 py-1.5 text-[13px]"
        />
      </Row>

      <Row label={t("set.faker.history")} help={t("set.faker.history_help")}>
        <label className="flex items-center gap-2 text-[13px]">
          <input
            type="checkbox"
            checked={defs.save_history}
            onChange={(e) => setDefs({ ...defs, save_history: e.target.checked })}
          />
          {t("set.faker.history_toggle")}
        </label>
      </Row>

      <button
        onClick={save}
        disabled={!dirty || saving}
        className={
          "mt-2 rounded px-3 py-1.5 text-[13px] font-medium " +
          (dirty && !saving
            ? "bg-[var(--color-accent)] text-[var(--color-accent-fg)]"
            : "cursor-default bg-[var(--color-surface)] text-[var(--color-muted)]")
        }
      >
        {saving ? t("common.saving") : dirty ? t("common.save") : t("common.saved")}
      </button>
    </Section>
  );
}

// ── Figlet — ASCII-art banner defaults ─────────────────────────────────
// Mirrors FakerSection; the figlet-defaults-changed event (fired by
// figletSetDefaults) makes the running popup re-read without a restart.
function FigletSection() {
  const t = useT();
  const [defs, setDefs] = useState<FigletDefaults | null>(null);
  const [saved, setSaved] = useState<FigletDefaults | null>(null);
  const [fonts, setFonts] = useState<FigletFontMeta[]>([]);
  const [saving, setSaving] = useState(false);

  useEffect(() => {
    figletGetDefaults()
      .then((d) => {
        setDefs(d);
        setSaved(d);
      })
      .catch(() => undefined);
    figletFonts().then(setFonts).catch(() => undefined);
  }, []);

  if (!defs) return null;

  const dirty =
    saved !== null &&
    (saved.font !== defs.font ||
      saved.width !== defs.width ||
      saved.align !== defs.align ||
      saved.trim !== defs.trim ||
      saved.comment !== defs.comment ||
      saved.boxed !== defs.boxed ||
      saved.save_history !== defs.save_history ||
      saved.pinned.join(",") !== defs.pinned.join(","));

  const fontNames = new Set(fonts.map((f) => f.name));
  const save = async () => {
    setSaving(true);
    try {
      await figletSetDefaults(defs);
      setSaved(defs);
    } catch (e) {
      console.error("save figlet defaults", e);
    } finally {
      setSaving(false);
    }
  };

  const ALIGNS: FigletAlign[] = ["left", "center", "right"];
  const COMMENTS: { v: FigletComment; label: string }[] = [
    { v: "none", label: t("set.figlet.plain") },
    { v: "slashes", label: "//" },
    { v: "hash", label: "#" },
    { v: "block", label: "/* */" },
    { v: "html", label: "<!--" },
  ];

  return (
    <Section
      icon={<Type size={16} className="text-[var(--color-accent)]" />}
      id="figlet"
      title={t("set.figlet.title")}
      subtitle={t("set.figlet.subtitle")}
    >
      <Row label={t("set.figlet.font")} help={t("set.figlet.font_help")}>
        <input
          list="figlet-font-list"
          value={defs.font}
          onChange={(e) => setDefs({ ...defs, font: e.target.value })}
          className="w-full rounded border border-[var(--color-border)] bg-[var(--color-bg)] px-2 py-1.5 text-[13px]"
        />
        <datalist id="figlet-font-list">
          {fonts.map((f) => (
            <option key={f.name} value={f.name} />
          ))}
        </datalist>
      </Row>

      <Row label={t("set.figlet.width")} help={t("set.figlet.width_help")}>
        <input
          type="number"
          min={0}
          max={400}
          value={defs.width}
          onChange={(e) =>
            setDefs({ ...defs, width: Math.max(0, Math.min(400, parseInt(e.target.value, 10) || 0)) })
          }
          className="w-28 rounded border border-[var(--color-border)] bg-[var(--color-bg)] px-2 py-1.5 text-[13px]"
        />
      </Row>

      <Row label={t("set.figlet.align")}>
        <div className="flex flex-wrap gap-1">
          {ALIGNS.map((a) => (
            <button
              key={a}
              onClick={() => setDefs({ ...defs, align: a })}
              className={
                "rounded px-2.5 py-1 text-[12px] font-medium capitalize " +
                (defs.align === a
                  ? "bg-[var(--color-accent)] text-[var(--color-accent-fg)]"
                  : "bg-[var(--color-surface)] text-[var(--color-muted)]")
              }
            >
              {t(`set.figlet.align_${a}` as MessageKey)}
            </button>
          ))}
        </div>
      </Row>

      <Row label={t("set.figlet.comment")} help={t("set.figlet.comment_help")}>
        <div className="flex flex-wrap gap-1">
          {COMMENTS.map((c) => (
            <button
              key={c.v}
              onClick={() => setDefs({ ...defs, comment: c.v })}
              className={
                "rounded px-2.5 py-1 text-[12px] font-medium " +
                (defs.comment === c.v
                  ? "bg-[var(--color-accent)] text-[var(--color-accent-fg)]"
                  : "bg-[var(--color-surface)] text-[var(--color-muted)]")
              }
            >
              {c.label}
            </button>
          ))}
        </div>
      </Row>

      <Row label={t("set.figlet.trim")} help={t("set.figlet.trim_help")}>
        <label className="flex items-center gap-2 text-[13px]">
          <input
            type="checkbox"
            checked={defs.trim}
            onChange={(e) => setDefs({ ...defs, trim: e.target.checked })}
          />
          {t("set.figlet.trim_toggle")}
        </label>
      </Row>

      <Row label={t("set.figlet.box")}>
        <label className="flex items-center gap-2 text-[13px]">
          <input
            type="checkbox"
            checked={defs.boxed}
            onChange={(e) => setDefs({ ...defs, boxed: e.target.checked })}
          />
          {t("set.figlet.box_toggle")}
        </label>
      </Row>

      <Row label={t("set.figlet.pinned")} help={t("set.figlet.pinned_help")}>
        <input
          type="text"
          value={defs.pinned.join(", ")}
          onChange={(e) =>
            setDefs({
              ...defs,
              pinned: e.target.value
                .split(",")
                .map((s) => s.trim().toLowerCase())
                .filter((s) => s && fontNames.has(s)),
            })
          }
          placeholder="standard, slant, doom"
          className="w-full rounded border border-[var(--color-border)] bg-[var(--color-bg)] px-2 py-1.5 text-[13px]"
        />
      </Row>

      <Row label={t("set.faker.history")} help={t("set.figlet.history_help")}>
        <label className="flex items-center gap-2 text-[13px]">
          <input
            type="checkbox"
            checked={defs.save_history}
            onChange={(e) => setDefs({ ...defs, save_history: e.target.checked })}
          />
          {t("set.figlet.history_toggle")}
        </label>
      </Row>

      <button
        onClick={save}
        disabled={!dirty || saving}
        className={
          "mt-2 rounded px-3 py-1.5 text-[13px] font-medium " +
          (dirty && !saving
            ? "bg-[var(--color-accent)] text-[var(--color-accent-fg)]"
            : "cursor-default bg-[var(--color-surface)] text-[var(--color-muted)]")
        }
      >
        {saving ? t("common.saving") : dirty ? t("common.save") : t("common.saved")}
      </button>
    </Section>
  );
}

// ── Security builders — pentest command builders ───────────────────────
// Persisted defaults for the `sec` cluster. auto_enter defaults OFF; sharp
// presets always confirm (not toggleable). Mirrors FakerSection.
function SecuritySection() {
  const t = useT();
  const [defs, setDefs] = useState<SecDefaults | null>(null);
  const [saved, setSaved] = useState<SecDefaults | null>(null);
  const [saving, setSaving] = useState(false);
  // Common wordlist paths (for the datalist) + whether the configured one exists.
  const [wordlists, setWordlists] = useState<string[]>([]);
  const [wlExists, setWlExists] = useState<boolean | null>(null);

  useEffect(() => {
    secGetDefaults()
      .then((d) => {
        setDefs(d);
        setSaved(d);
      })
      .catch(() => undefined);
    secCatalog()
      .then((c) => setWordlists(c.common_wordlists))
      .catch(() => undefined);
  }, []);

  // Debounced existence check for the configured default wordlist.
  const wl = defs?.wordlist ?? "";
  useEffect(() => {
    if (!wl.trim()) {
      setWlExists(null);
      return;
    }
    const timer = window.setTimeout(() => {
      secPathExists(wl)
        .then(setWlExists)
        .catch(() => setWlExists(null));
    }, 250);
    return () => window.clearTimeout(timer);
  }, [wl]);

  if (!defs) return null;

  const dirty = saved !== null && JSON.stringify(saved) !== JSON.stringify(defs);
  const save = async () => {
    setSaving(true);
    try {
      await secSetDefaults(defs);
      setSaved(defs);
    } catch (e) {
      console.error("save sec defaults", e);
    } finally {
      setSaving(false);
    }
  };

  const seg = <T extends string>(cur: T, options: [T, string][], onPick: (v: T) => void) => (
    <div className="flex gap-1">
      {options.map(([v, label]) => (
        <button
          key={v}
          onClick={() => onPick(v)}
          className={
            "rounded px-2.5 py-1 text-[12px] font-medium " +
            (cur === v
              ? "bg-[var(--color-accent)] text-[var(--color-accent-fg)]"
              : "bg-[var(--color-surface)] text-[var(--color-muted)]")
          }
        >
          {label}
        </button>
      ))}
    </div>
  );

  const textInput = (val: string, ph: string, onChange: (v: string) => void) => (
    <input
      type="text"
      value={val}
      placeholder={ph}
      onChange={(e) => onChange(e.target.value)}
      className="w-full rounded border border-[var(--color-border)] bg-[var(--color-bg)] px-2 py-1.5 text-[13px]"
    />
  );

  return (
    <Section
      icon={<ShieldAlert size={16} className="text-[var(--color-accent)]" />}
      id="security"
      title={t("settings.section.security")}
      subtitle={t("set.sec.subtitle")}
    >
      <Row label={t("set.sec.scope")} help={t("set.sec.scope_help")}>
        <textarea
          value={defs.scope_note}
          placeholder={t("set.sec.scope_ph")}
          onChange={(e) => setDefs({ ...defs, scope_note: e.target.value })}
          rows={2}
          className="w-full rounded border border-[var(--color-border)] bg-[var(--color-bg)] px-2 py-1.5 text-[13px]"
        />
      </Row>

      <Row label={t("set.sec.wordlist")} help={t("set.sec.wordlist_help")}>
        <div className="flex items-center gap-2">
          <input
            type="text"
            list="sec-wordlists"
            value={defs.wordlist}
            placeholder="/usr/share/wordlists/rockyou.txt"
            onChange={(e) => setDefs({ ...defs, wordlist: e.target.value })}
            className="w-full rounded border border-[var(--color-border)] bg-[var(--color-bg)] px-2 py-1.5 text-[13px]"
          />
          <datalist id="sec-wordlists">
            {wordlists.map((w) => (
              <option key={w} value={w} />
            ))}
          </datalist>
          {wlExists != null && (
            <span
              className={"shrink-0 text-[11px] " + (wlExists ? "text-emerald-500" : "text-amber-500")}
              title={wlExists ? t("set.sec.path_exists") : t("set.sec.path_missing")}
            >
              {wlExists ? t("set.sec.found") : t("set.sec.not_found")}
            </span>
          )}
        </div>
      </Row>

      <Row label={t("set.sec.output")} help={t("set.sec.output_help")}>
        {textInput(defs.output_dir, "~/scans", (v) => setDefs({ ...defs, output_dir: v }))}
      </Row>

      <Row label={t("set.sec.timing")} help={t("set.sec.timing_help")}>
        <div className="flex gap-2">
          <input type="text" value={defs.timing} placeholder="-T" onChange={(e) => setDefs({ ...defs, timing: e.target.value })} className="w-16 rounded border border-[var(--color-border)] bg-[var(--color-bg)] px-2 py-1.5 text-[13px]" />
          <input type="number" min={0} max={500} value={defs.threads} onChange={(e) => setDefs({ ...defs, threads: Math.max(0, Math.min(500, parseInt(e.target.value, 10) || 0)) })} className="w-20 rounded border border-[var(--color-border)] bg-[var(--color-bg)] px-2 py-1.5 text-[13px]" placeholder={t("set.sec.threads")} />
          <input type="number" min={0} value={defs.rate} onChange={(e) => setDefs({ ...defs, rate: Math.max(0, parseInt(e.target.value, 10) || 0) })} className="w-20 rounded border border-[var(--color-border)] bg-[var(--color-bg)] px-2 py-1.5 text-[13px]" placeholder={t("set.sec.rate")} />
        </div>
      </Row>

      <Row label={t("set.sec.john")} help={t("set.sec.john_help")}>
        {seg(defs.john_line, [["jumbo", "Jumbo"], ["core", "Core"]], (v) => setDefs({ ...defs, john_line: v }))}
      </Row>

      <Row label={t("set.sec.terminal")} help={t("set.sec.terminal_help")}>
        {seg(defs.terminal, [["iterm", "iTerm2"], ["terminal", "Terminal.app"]], (v) => setDefs({ ...defs, terminal: v }))}
      </Row>

      <Row label={t("set.sec.behaviour")}>
        <div className="space-y-1.5">
          <label className="flex items-center gap-2 text-[13px]">
            <input type="checkbox" checked={defs.auto_enter} onChange={(e) => setDefs({ ...defs, auto_enter: e.target.checked })} />
            {t("set.sec.auto_enter")}
          </label>
          <label className="flex items-center gap-2 text-[13px] text-[var(--color-muted)]">
            <input type="checkbox" checked disabled />
            {t("set.sec.confirm_sharp")}
          </label>
        </div>
      </Row>

      <Row label={t("set.sec.history")}>
        <label className="flex items-center gap-2 text-[13px]">
          <input type="checkbox" checked={defs.save_history} onChange={(e) => setDefs({ ...defs, save_history: e.target.checked })} />
          {t("set.sec.history_toggle")}
        </label>
      </Row>

      <button
        onClick={save}
        disabled={!dirty || saving}
        className={
          "mt-2 rounded px-3 py-1.5 text-[13px] font-medium " +
          (dirty && !saving ? "bg-[var(--color-accent)] text-[var(--color-accent-fg)]" : "cursor-default bg-[var(--color-surface)] text-[var(--color-muted)]")
        }
      >
        {saving ? t("common.saving") : dirty ? t("common.save") : t("common.saved")}
      </button>
    </Section>
  );
}

// ── Meme library directory ─────────────────────────────────────────────
// Where the `meme [query]` picker scans for GIFs/images. Defaults to a
// home-relative `My Drive/media/memes`; on Windows with Google Drive in
// streaming mode the library lives under a drive letter (e.g.
// `G:\My Drive\media\memes`), so it's overridable here.
/**
 * Appearance → "Popup animation": the CRT power-on duration in ms (0 = off).
 *
 * Only the power-ON is user-settable — the dismiss is derived from it
 * (`crtOffMs`), so it can never end up outlasting the summon. The bounds come
 * from the backend (`CrtAnimation.min/max`) rather than being duplicated here,
 * so the input can't drift out of sync with the clamp that actually persists.
 */
function CrtAnimationRow() {
  const t = useT();
  const [cfg, setCfg] = useState<CrtAnimation | null>(null);
  const [value, setValue] = useState("");
  const [busy, setBusy] = useState(false);
  const [savedOk, setSavedOk] = useState(false);

  useEffect(() => {
    getCrtAnimation()
      .then((c) => {
        setCfg(c);
        setValue(String(c.ms));
      })
      .catch(() => undefined);
  }, []);

  if (cfg === null) return null;

  const parsed = parseInt(value, 10);
  // 0 is a valid value that deliberately sits BELOW `min` — it means "off",
  // not "as short as possible".
  const valid =
    Number.isFinite(parsed) && (parsed === 0 || (parsed >= cfg.min && parsed <= cfg.max));
  const dirty = valid && parsed !== cfg.ms;

  const apply = async (ms: number) => {
    setBusy(true);
    setSavedOk(false);
    try {
      const stored = await setCrtAnimation(ms);
      setCfg({ ...cfg, ms: stored });
      setValue(String(stored));
      setSavedOk(true);
    } catch (e) {
      console.error("save crt animation", e);
    } finally {
      setBusy(false);
    }
  };

  const presets = [
    { ms: 0, label: t("common.off") },
    { ms: 150, label: "150" },
    { ms: cfg.default_ms, label: `${cfg.default_ms}` },
    { ms: 400, label: "400" },
    { ms: 650, label: "650" },
  ];

  return (
    <>
      <Row label={t("set.crt.label")}>
        <div className="flex w-full flex-wrap items-center gap-2">
          <div className="flex overflow-hidden rounded-lg border border-[var(--color-border)]">
            {presets.map((p) => {
              const active = cfg.ms === p.ms;
              return (
                <button
                  key={p.ms}
                  type="button"
                  disabled={busy}
                  onClick={() => void apply(p.ms)}
                  title={p.ms === 0 ? t("set.crt.off_title") : `${p.ms} ms`}
                  className={
                    "border-r border-[var(--color-border)] px-2.5 py-1.5 text-[12px] transition-colors last:border-r-0 " +
                    (active
                      ? "bg-[var(--color-accent)] text-[var(--color-accent-fg)]"
                      : "bg-[var(--color-surface)] text-[var(--color-muted)] hover:text-[var(--color-fg)]")
                  }
                >
                  {p.label}
                  {p.ms === cfg.default_ms && <span className="ml-1 opacity-60">★</span>}
                </button>
              );
            })}
          </div>
          <input
            type="number"
            min={0}
            max={cfg.max}
            step={10}
            value={value}
            aria-label={t("set.crt.aria")}
            onChange={(e) => {
              setValue(e.target.value);
              setSavedOk(false);
            }}
            onKeyDown={(e) => {
              if (e.key === "Enter" && dirty) void apply(parsed);
              e.stopPropagation();
            }}
            className="w-24 rounded border border-[var(--color-border)] bg-[var(--color-bg)] px-2 py-1 font-mono text-[12px]"
          />
          <span className="text-[12px] text-[var(--color-muted)]">ms</span>
          <button
            onClick={() => void apply(parsed)}
            disabled={busy || !dirty}
            className="rounded-md bg-[var(--color-accent)] px-3 py-1.5 text-[12px] font-medium text-[var(--color-accent-fg)] disabled:opacity-40"
          >
            {busy ? t("common.saving") : t("common.save")}
          </button>
          {savedOk && <span className="confirm-enter text-[12px] text-emerald-500">{t("common.saved")} ✓</span>}
          {!valid && value.trim() !== "" && (
            <span className="text-[12px] text-amber-500">
              {t("set.crt.range", { min: cfg.min, max: cfg.max })}
            </span>
          )}
        </div>
      </Row>
      <p className="mt-2 text-[11px] leading-snug text-[var(--color-muted)]">
        {cfg.ms === 0 ? (
          <>{t("set.crt.off_help")}</>
        ) : (
          <>
            {t("set.crt.help", { off: crtOffMs(cfg.ms) })}{" "}
            {cfg.ms > 400 && t("set.crt.slow") + " "}
            {t("set.crt.applies")}
          </>
        )}
      </p>
    </>
  );
}

/**
 * Settings → "Android (adb)" (v0.119.0): live status for the `adb` command
 * (binary found? devices? authorization state?) plus the one-time setup guide
 * as an expandable section — the in-app documentation the command's setup
 * card points to. Deep-link id `adb` (`settings adb`).
 */
function AdbSection() {
  const t = useT();
  const [status, setStatus] = useState<AdbStatus | null>(null);
  useEffect(() => {
    let alive = true;
    adbStatus()
      .then((st) => {
        if (alive) setStatus(st);
      })
      .catch(() => undefined);
    return () => {
      alive = false;
    };
  }, []);
  const chip = (ok: boolean, label: string) => (
    <span
      className={
        "rounded-full px-2 py-0.5 text-[11px] " +
        (ok ? "bg-emerald-500/15 text-emerald-500" : "bg-amber-500/15 text-amber-500")
      }
    >
      {ok ? "✓" : "✗"} {label}
    </span>
  );
  return (
    <Section
      icon={<Smartphone size={16} className="text-[var(--color-accent)]" />}
      id="adb"
      title={t("settings.section.adb")}
      subtitle={t("set.adb.subtitle")}
    >
      <div className="flex flex-wrap items-center gap-1.5">
        {status === null ? (
          <span className="text-[12px] text-[var(--color-muted)]">{t("set.sync.checking")}</span>
        ) : (
          <>
            {chip(status.found, status.found ? t("set.adb.installed") : t("set.adb.missing"))}
            {chip(
              status.devices.some((d) => d.state === "device"),
              status.devices.length === 0
                ? t("set.adb.no_device")
                : status.devices.some((d) => d.state === "device")
                  ? tn("set.adb.ready", status.devices.filter((d) => d.state === "device").length)
                  : t("set.adb.unauthorized"),
            )}
          </>
        )}
      </div>
      {status !== null && !status.found && (
        <p className="mt-2 text-[12px] text-[var(--color-muted)]">
          <InlineMd text={t("set.adb.install")} />
        </p>
      )}
      <details className="mt-3 text-[12px]">
        <summary className="cursor-pointer text-[var(--color-fg)]">
          {t("set.adb.setup")}
        </summary>
        <ol className="mt-2 list-decimal space-y-1.5 pl-5 text-[12px] leading-snug text-[var(--color-muted)]">
          <li><InlineMd text={t("set.adb.step1")} /></li>
          <li>{t("set.adb.step2")}</li>
          <li><InlineMd text={t("set.adb.step3")} /></li>
          <li><InlineMd text={t("set.adb.step4")} /></li>
          <li><InlineMd text={t("set.adb.step5")} /></li>
        </ol>
      </details>
    </Section>
  );
}

function HistoryLimitSection() {
  const t = useT();
  const [limit, setLimit] = useState<HistoryLimit | null>(null);
  const [value, setValue] = useState("");
  const [busy, setBusy] = useState(false);
  const [savedOk, setSavedOk] = useState(false);
  // DB size / dead space + on-demand compaction (PERFORMANCE-PLAN A1).
  const [space, setSpace] = useState<DbSpace | null>(null);
  const [compacting, setCompacting] = useState(false);
  const [compactResult, setCompactResult] = useState<string | null>(null);
  useEffect(() => {
    void getDbSpace().then(setSpace).catch(() => setSpace(null));
  }, []);
  const compactNow = async () => {
    if (!space) return;
    setCompacting(true);
    setCompactResult(null);
    const before = space.bytes;
    try {
      const after = await compactDb();
      setSpace(after);
      setCompactResult(`${formatBytes(before)} → ${formatBytes(after.bytes)}`);
    } catch (e) {
      console.error("db compact failed", e);
      setCompactResult(null);
    } finally {
      setCompacting(false);
    }
  };

  useEffect(() => {
    getHistoryMax()
      .then((l) => {
        setLimit(l);
        setValue(String(l.max));
      })
      .catch(() => setLimit({ max: 1000, min: 50, ceiling: 100000 }));
  }, []);

  if (limit === null) return null;

  const parsed = parseInt(value, 10);
  const valid = Number.isFinite(parsed) && parsed >= limit.min && parsed <= limit.ceiling;
  const dirty = valid && parsed !== limit.max;

  const save = async () => {
    if (!valid) return;
    setBusy(true);
    setSavedOk(false);
    try {
      const stored = await setHistoryMax(parsed);
      setLimit({ ...limit, max: stored });
      setValue(String(stored));
      setSavedOk(true);
    } catch (e) {
      console.error("save history max", e);
    } finally {
      setBusy(false);
    }
  };

  return (
    <Section
      icon={<History size={16} className="text-[var(--color-accent)]" />}
      id="clipboard-history"
      title={t("settings.section.clipboard-history")}
      subtitle={t("set.hist.subtitle", { min: limit.min.toLocaleString(locale()), max: limit.ceiling.toLocaleString(locale()), def: (1000).toLocaleString(locale()) })}
    >
      <Row label={t("set.hist.max")} help={t("set.hist.max_help")}>
        <div className="flex w-full items-center gap-2">
          <input
            type="number"
            min={limit.min}
            max={limit.ceiling}
            step={100}
            value={value}
            onChange={(e) => {
              setValue(e.target.value);
              setSavedOk(false);
            }}
            onKeyDown={(e) => {
              if (e.key === "Enter" && dirty) void save();
              e.stopPropagation();
            }}
            className="w-36 rounded border border-[var(--color-border)] bg-[var(--color-bg)] px-2 py-1 font-mono text-[12px]"
          />
          <button
            onClick={() => void save()}
            disabled={busy || !dirty}
            className="rounded-md bg-[var(--color-accent)] px-3 py-1.5 text-[12px] font-medium text-[var(--color-accent-fg)] disabled:opacity-40"
          >
            {busy ? t("common.saving") : t("common.save")}
          </button>
          {savedOk && <span className="confirm-enter text-[12px] text-emerald-500">{t("common.saved")} ✓</span>}
          {!valid && value.trim() !== "" && (
            <span className="text-[12px] text-amber-500">
              {limit.min.toLocaleString(locale())}–{limit.ceiling.toLocaleString(locale())}
            </span>
          )}
        </div>
      </Row>
      <Row
        label={t("set.hist.db")}
        help={t("set.hist.db_help")}
      >
        <div className="flex w-full items-center gap-2 text-[12px]">
          <span className="font-mono text-[var(--color-muted)]">
            {space === null
              ? "…"
              : t("set.hist.dead", {
                  size: formatBytes(space.bytes),
                  pct:
                    space.page_count > 0
                      ? Math.round((100 * space.freelist_pages) / space.page_count)
                      : 0,
                })}
          </span>
          <button
            onClick={() => void compactNow()}
            disabled={compacting || space === null}
            className="rounded-md border border-[var(--color-border)] px-3 py-1.5 text-[12px] font-medium hover:border-[var(--color-accent)] hover:text-[var(--color-accent)] disabled:opacity-40"
          >
            {compacting ? t("set.hist.compacting") : t("set.hist.compact")}
          </button>
          {compactResult && <span className="text-emerald-500">{compactResult}</span>}
        </div>
      </Row>
    </Section>
  );
}

/** Spotify (macOS): record the local play history, show covers, clear it. */
function SpotifySection() {
  const t = useT();
  const [cfg, setCfg] = useState<SpotifyConfig | null>(null);
  const [armed, setArmed] = useState(false);
  const [note, setNote] = useState<string | null>(null);

  useEffect(() => {
    getSpotifyConfig()
      .then(setCfg)
      .catch(() => setCfg({ history: true, artwork: false }));
  }, []);

  if (cfg === null) return null;

  const update = (next: SpotifyConfig) => {
    setCfg(next);
    void setSpotifyConfig(next).catch((e) => console.error("save spotify config", e));
  };

  const clear = async () => {
    if (!armed) {
      setArmed(true);
      return;
    }
    setArmed(false);
    try {
      const n = await spotifyHistoryClear();
      setNote(t("set.spotify.cleared", { n }));
    } catch (e) {
      setNote(String(e));
    }
  };

  return (
    <Section
      icon={<Music size={16} className="text-[var(--color-accent)]" />}
      id="spotify"
      title={t("settings.section.spotify")}
      subtitle={t("set.spotify.subtitle")}
    >
      <Row label={t("set.spotify.record")} help={t("set.spotify.record_help")}>
        <label className="flex cursor-pointer items-center gap-2 text-[12px]">
          <input
            type="checkbox"
            checked={cfg.history}
            onChange={(e) => update({ ...cfg, history: e.target.checked })}
            className="accent-[var(--color-accent)]"
          />
          <span className="text-[var(--color-muted)]">{cfg.history ? t("set.spotify.record_on") : t("common.off")}</span>
        </label>
      </Row>
      <Row label={t("set.spotify.covers")} help={t("set.spotify.covers_help")}>
        <label className="flex cursor-pointer items-center gap-2 text-[12px]">
          <input
            type="checkbox"
            checked={cfg.artwork}
            onChange={(e) => update({ ...cfg, artwork: e.target.checked })}
            className="accent-[var(--color-accent)]"
          />
          <span className="text-[var(--color-muted)]">{cfg.artwork ? t("common.on") : t("set.spotify.covers_off")}</span>
        </label>
      </Row>
      <Row label={t("set.spotify.clear")} help={t("set.spotify.clear_help")}>
        <button
          type="button"
          onClick={() => void clear()}
          onBlur={() => setArmed(false)}
          className={`rounded-md px-2 py-1 text-[12px] ${armed ? "bg-red-600 text-white" : "border border-[var(--color-border)] text-[var(--color-fg)] hover:bg-[var(--color-surface)]"}`}
        >
          {armed ? t("set.spotify.clear_confirm") : t("set.spotify.clear")}
        </button>
        {note && <span className="text-[12px] text-[var(--color-muted)]">{note}</span>}
      </Row>
    </Section>
  );
}

function WeatherSection() {
  const t = useT();
  const [cfg, setCfg] = useState<WeatherConfig | null>(null);
  const [keyInput, setKeyInput] = useState("");
  const [units, setUnits] = useState("metric");
  const [busy, setBusy] = useState(false);
  const [savedOk, setSavedOk] = useState(false);

  useEffect(() => {
    getWeatherConfig()
      .then((c) => {
        setCfg(c);
        setUnits(c.units);
      })
      .catch(() => setCfg({ has_key: false, units: "metric" }));
  }, []);

  if (cfg === null) return null;

  const save = async () => {
    setBusy(true);
    setSavedOk(false);
    try {
      // Empty input leaves an existing key untouched (null = don't change);
      // typing a value replaces it.
      await setWeatherConfig(keyInput.trim() ? keyInput.trim() : null, units);
      setKeyInput("");
      const c = await getWeatherConfig();
      setCfg(c);
      setUnits(c.units);
      setSavedOk(true);
    } catch (e) {
      console.error("save weather config", e);
    } finally {
      setBusy(false);
    }
  };

  return (
    <Section
      icon={<CloudSun size={16} className="text-[var(--color-accent)]" />}
      id="weather"
      title={t("settings.section.weather")}
      subtitle={t("set.weather.subtitle")}
    >
      <Row label={t("set.weather.key")} help={cfg.has_key ? t("set.weather.key_stored") : t("set.weather.key_get")}>
        <input
          type="password"
          value={keyInput}
          spellCheck={false}
          autoComplete="off"
          placeholder={cfg.has_key ? t("set.weather.key_ph_stored") : t("set.weather.key")}
          onChange={(e) => {
            setKeyInput(e.target.value);
            setSavedOk(false);
          }}
          className="min-w-0 flex-1 rounded border border-[var(--color-border)] bg-[var(--color-bg)] px-2 py-1 font-mono text-[12px]"
        />
      </Row>
      <Row label={t("set.weather.units")} help={t("set.weather.units_help")}>
        <select
          value={units}
          onChange={(e) => {
            setUnits(e.target.value);
            setSavedOk(false);
          }}
          className="rounded border border-[var(--color-border)] bg-[var(--color-bg)] px-2 py-1 text-[12px]"
        >
          <option value="metric">{t("set.weather.metric")}</option>
          <option value="imperial">{t("set.weather.imperial")}</option>
        </select>
      </Row>
      <div className="mt-3 flex items-center gap-3">
        <button
          onClick={() => void save()}
          disabled={busy}
          className="rounded-md bg-[var(--color-accent)] px-3 py-1.5 text-[12px] font-medium text-[var(--color-accent-fg)] disabled:opacity-40"
        >
          {busy ? t("common.saving") : t("common.save")}
        </button>
        {savedOk && <span className="confirm-enter text-[12px] text-emerald-500">{t("common.saved")} ✓</span>}
        {!cfg.has_key && !savedOk && (
          <button
            onClick={() => void openUrl("https://home.openweathermap.org/api_keys")}
            className="text-[11px] text-[var(--color-accent)] underline"
          >
            {t("set.weather.get_key")}
          </button>
        )}
      </div>
    </Section>
  );
}

function MemeSection() {
  const t = useT();
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
        title: t("set.meme.pick"),
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
      id="meme"
      title={t("settings.section.meme")}
      subtitle={t("set.meme.subtitle")}
    >
      <Row label={t("set.ds.folder")}>
        <div className="flex w-full items-center gap-2">
          <input
            type="text"
            value={dir}
            spellCheck={false}
            placeholder={t("set.meme.default_ph")}
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
            {t("set.meme.browse")}
          </button>
        </div>
      </Row>

      <div className="mt-2 flex items-center gap-2">
        <button
          onClick={save}
          disabled={!dirty || busy}
          className="rounded bg-[var(--color-accent)] px-3 py-1 text-[12px] font-medium text-[var(--color-accent-fg)] hover:opacity-90 disabled:opacity-40"
        >
          {busy ? t("common.saving") : t("common.save")}
        </button>
        <button
          onClick={() => {
            setDir("");
            setSavedOk(false);
          }}
          className="rounded border border-[var(--color-border)] px-3 py-1 text-[12px] hover:bg-[var(--color-surface)]"
        >
          {t("set.meme.reset")}
        </button>
        {dirty && !busy && (
          <span className="text-[11px] text-[var(--color-muted)]">{t("common.unsaved")}</span>
        )}
        {savedOk && !dirty && (
          <span className="confirm-enter text-[11px] text-[var(--color-muted)]">{t("common.saved")}</span>
        )}
      </div>
      <p className="mt-1 text-[11px] text-[var(--color-muted)]">
        {t("set.meme.preview_note")}
      </p>
    </Section>
  );
}

function SnippetsSection() {
  const t = useT();
  const [stats, setStats] = useState<SnippetStorage | null>(null);

  // Re-read on every mount. The Settings content subtree is keyed on the
  // active tab in App.tsx, so it remounts each time Settings is opened → the
  // figure is always current without event plumbing.
  useEffect(() => {
    let alive = true;
    getSnippetStorage()
      .then((s) => alive && setStats(s))
      .catch(() => alive && setStats({ count: 0, bytes: 0 }));
    return () => {
      alive = false;
    };
  }, []);

  return (
    <Section
      icon={<Zap size={16} className="text-[var(--color-accent)]" />}
      id="snippets"
      title={t("settings.section.snippets")}
      subtitle={t("set.snip.subtitle")}
    >
      <div className="flex items-baseline gap-8">
        <div>
          <div className="text-[22px] font-semibold tabular-nums">
            {stats ? stats.count.toLocaleString(locale()) : "…"}
          </div>
          <div className="text-[11px] text-[var(--color-muted)]">
            {stats && stats.count === 1 ? t("set.snip.one") : t("set.snip.many")}
          </div>
        </div>
        <div>
          <div className="text-[22px] font-semibold tabular-nums">
            {stats ? formatBytes(stats.bytes) : "…"}
          </div>
          <div className="text-[11px] text-[var(--color-muted)]">{t("set.snip.on_disk")}</div>
        </div>
      </div>
    </Section>
  );
}
