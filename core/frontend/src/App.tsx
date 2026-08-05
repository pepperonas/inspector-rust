import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { getVersion } from "@tauri-apps/api/app";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { useTauriEvent } from "./hooks/useTauriEvent";
import { FeaturesPanel } from "./components/FeaturesPanel";
import { TimesheetPanel } from "./components/TimesheetPanel";
import { Footer } from "./components/Footer";
import { HistoryList } from "./components/HistoryList";
import { NotesPanel } from "./components/NotesPanel";
import { PreviewPanel } from "./components/PreviewPanel";
import { SpaceInvadersGame } from "./components/SpaceInvadersGame";
import { BrightnessPanel } from "./components/BrightnessPanel";
import { SoundPanel } from "./components/SoundPanel";
import { HuePanel } from "./components/HuePanel";
import { StatsPanel } from "./components/StatsPanel";
import { IrisPanel } from "./components/IrisPanel";
import { BoomPanel } from "./components/BoomPanel";
import { CalendarPanel } from "./components/CalendarPanel";
import { CleanPanel } from "./components/CleanPanel";
import { SnitchPanel } from "./components/SnitchPanel";
import { SnitchMapPanel } from "./components/SnitchMapPanel";
import { ShazamPanel } from "./components/ShazamPanel";
import { UptimePanel } from "./components/UptimePanel";
import { parseIrisArg, irisRowLabel, irisAction } from "./lib/iris";
import { irisStart, irisStop, irisStatus, irisSetThreshold } from "./lib/ipc";
import { WeatherPanel } from "./components/WeatherPanel";
import { TokensPanel } from "./components/TokensPanel";
import { RandomPanel } from "./components/RandomPanel";
import CommandHelp from "./components/CommandHelp";
import {
  parseFigletCommand,
  galleryFonts,
  optsFromDefaults,
  type FigletFontMeta,
  type FigletDefaults,
  type FigletOpts,
} from "./lib/figlet";
import { bannerPngBase64, themePngColors } from "./lib/figlet-png";
import { discoEngine } from "./lib/disco-engine";
import { SearchBar } from "./components/SearchBar";
import { SettingsPanel } from "./components/SettingsPanel";
import { SnippetsPanel } from "./components/SnippetsPanel";
import { useClipboardHistory } from "./hooks/useClipboardHistory";
import { useFuzzySearch } from "./hooks/useFuzzySearch";
import { useKeyboardNav } from "./hooks/useKeyboardNav";
import { useNotes } from "./hooks/useNotes";
import { useSnippets } from "./hooks/useSnippets";
import { playCrtOn, playCrtOff, primeCrtHidden, CRT_OFF_MS } from "./lib/md3-motion";
import { detectSocial } from "./lib/social";
import { pinnedClips } from "./lib/history-filter";
import { tryEvaluate } from "./lib/calc";
import { tryConvert } from "./lib/convert";
import { tryParseColor } from "./lib/colors";
import {
  COMMANDS,
  commandSuggestions,
  isCommandAvailable,
  isFlappyTrigger,
  isSpaceInvadersTrigger,
  isGetShakyTrigger,
  is2faTrigger,
  isBpmTrigger,
  isEqualizerTrigger,
  isOpenerTrigger,
  parseOtpQuery,
  rockTheBoxMode,
  DEFAULT_PWGEN_LENGTH,
  parseCommand,
  parseKillArg,
  parsePwgenArg,
  parseResizeArg,
  parseTimerArg,
  parseAlarmArg,
  parseShotDelay,
  parseRandomArg,
  randomInt,
  parseWakelockArg,
  parseTrackArg,
  parseDiscoArg,
  resizePresetSuggestions,
  translateUrl,
  isTranslateKind,
  TRANSLATE_LANGS,
  SEARCH_BANGS,
  fuzzyScore,
  formatBytes,
  type CommandKind,
  type ParsedCommand,
} from "./lib/commands";
import { filterKillProcesses, KILL_LIST_CAP } from "./lib/kill-filter";
import { parseHelpQuery, searchDocs, type HelpTarget } from "./lib/commandHelp";
import { lookupDoc } from "./lib/commandDocs";
import { matchCities } from "./lib/cities";
import { slugify, generateUuids, sha256Hex, formatJson, decodeJwt } from "./lib/devtools";
import { qrPngBase64 } from "./lib/qr";
import { TOP_OPENERS, pickOpenerIndex } from "./lib/openers";
import { PongGame } from "./components/PongGame";
import { SnakeGame } from "./components/SnakeGame";
import { FlappyGame } from "./components/FlappyGame";
import { matchMemes, type MemeEntry } from "./lib/meme";
import { BpmDetector } from "./components/BpmDetector";
import { EqualizerVisualizer } from "./components/EqualizerVisualizer";
import { TotpOverlay } from "./components/TotpOverlay";
import {
  clearHistory,
  deleteEntry,
  setClipPinned,
  findSnippets,
  hidePopup as hidePopupRaw,
  killProcess,
  listProcesses,
  optimizeClipboardImage,
  pasteEntry,
  pasteEntryFormatted,
  pasteSnippet,
  pasteText,
  figletFonts,
  figletGallery,
  figletRender,
  figletGetDefaults,
  figletCopyPng,
  figletSavePng,
  removeVowelsToClipboard,
  resizeClipboardImage,
  saveClipAsNote,
  systemLock,
  adjustVolume,
  setSuppressHide,
  toggleMute,
  startInputLock,
  systemReboot,
  systemShutdown,
  wakelockGet,
  wakelockSet,
  launchApp,
  listApps,
  type AppEntry,
  resizeFile,
  optimizeFile,
  getFinderSelection,
  finderTouch,
  finderMkdir,
  finderOpenTerminal,
  mdToPdfRun,
  screenshotCapture,
  screenshotRepeatLast,
  listMemes,
  copyMeme,
  trimOpenOverlay,
  qrCopyPng,
  showStatusToast,
  type ShazamDone,
  type CleanDone,
  trackStart,
  trackStop,
  trackStatus,
  brunoGetDefaults,
  fakerCatalog,
  fakerGenerate,
  fakerGetDefaults,
  pasteGenerated,
  secCatalog,
  secGetDefaults,
  secOpenInTerminal,
  startTimer,
  listTimers,
  getThemePreference,
  getBoomConfig,
  translateText,
  getLineageHighlight,
  type BrunoDefaults,
  type ProcessInfo,
} from "./lib/ipc";
import {
  parseFakerCommand,
  matchCatalog,
  formatValues,
  type CatalogEntry,
  type FakerDefaults,
} from "./lib/faker";
import {
  parseSecCommand,
  visiblePresets,
  matchPresets,
  type SecCatalog,
  type SecDefaults,
} from "./lib/sec";
import { confirmDialog } from "./lib/confirm";
import { computeBruno, computeBrunoSelf, formatBrunoBreakdown, formatBrunoSelfBreakdown, parseBrunoCommand, type GermanState } from "./lib/bruno";
import { matchSettingsSection } from "./lib/settings-sections";
import { IS_MAC } from "./lib/platform";
import { generatePassword, type PwgenMode } from "./lib/pwgen";
import { matchTotpEntries } from "./lib/totp";
import { applyTheme, normaliseTheme } from "./lib/theme";
import { openUrl } from "@tauri-apps/plugin-opener";
import type { FinderFileView, ListEntry, Snippet } from "./lib/types";

type Tab = "history" | "snippets" | "notes" | "timesheet" | "features" | "settings";

/** Max font rows rendered in the figlet gallery at once (the long tail is
 *  reached by the `@font` fuzzy filter — search is the primary nav). */
const FIGLET_MAX_ROWS = 140;
/** Above this text length the gallery shows name-only rows (skips per-font
 *  sample rendering); the big preview still renders the selected font. */
const FIGLET_SAMPLE_TEXT_CAP = 28;

function App() {
  const { entries, refresh: refreshHistory } = useClipboardHistory();
  const { snippets, categories: snippetCategories, refresh: refreshSnippets } = useSnippets();
  const { notes, categories: noteCategories, refresh: refreshNotes } = useNotes();
  const [query, setQuery] = useState("");
  const [selected, setSelected] = useState(0);
  const [activeTab, setActiveTab] = useState<Tab>("history");
  // `settings <section>` deep-link: SettingsPanel scrolls to this id +
  // briefly highlights the section. The nonce retriggers on repeat jumps.
  const [settingsJump, setSettingsJump] = useState<{ id: string; nonce: number } | null>(null);
  /** True briefly after each popup open — drives the list-entrance cascade. */
  const [listEntrance, setListEntrance] = useState(true);
  /** Preview crossfade (v0.88.0): restart the enter animation on selection
   *  change WITHOUT remounting the panel (a remount would refetch images). */
  const previewFadeRef = useRef<HTMLDivElement | null>(null);
  // Hidden game easter eggs — when non-null, the whole popup is replaced
  // by the matching game. `"pong"` ← typing `getshaky`; the two snake
  // modes ← `rockthebox` (walls kill) / `rockthabox` (wrap-around).
  // Exited only with Esc (handled inside the game).
  const [gameMode, setGameMode] = useState<
    "pong" | "snake-classic" | "snake-wrap" | "space" | "flappy" | null
  >(null);
  // BPM detector overlay state. Separate from `gameMode` because it
  // has different lifecycle semantics: audio teardown, mic
  // permission, and Enter-to-start (vs gameMode's type-to-start).
  const [bpmMode, setBpmMode] = useState(false);
  // Equalizer overlay state — mirrors bpmMode; `equalizer`/`eq` → spectrum viz.
  const [equalizerMode, setEqualizerMode] = useState(false);
  // Brightness mode — Enter on the `brightness` command row renders the
  // sliders in the right preview column (no separate window). `brightnessFocus`
  // is whether the arrow keys currently drive the sliders (true) or the left
  // list (false); Enter toggles between the two.
  const [brightnessMode, setBrightnessMode] = useState(false);
  const [brightnessFocus, setBrightnessFocus] = useState(false);
  // Sound mode — Enter on the `sound` command row renders the audio
  // output-device picker in the right preview column; `soundFocus` is whether
  // the arrow keys drive the picker (true) or the left list (false).
  const [soundMode, setSoundMode] = useState(false);
  const [soundFocus, setSoundFocus] = useState(false);
  // Hue mode — Enter on the `hue` command row renders the Philips Hue lamp
  // controls in the right preview column; `hueFocus` is whether the arrow keys
  // drive the panel (true) or the left list (false).
  const [hueMode, setHueMode] = useState(false);
  const [hueFocus, setHueFocus] = useState(false);
  // Stats mode — Enter on the `stats` command row renders the live
  // system-stats panel in the right preview column. Read-only, so `statsFocus`
  // just routes Esc to the panel; there's no selection model.
  const [statsMode, setStatsMode] = useState(false);
  const [statsFocus, setStatsFocus] = useState(false);
  // Iris mode — the mic-triggered red screen vignette. Unlike the other inline
  // panels this one is a TOGGLE: Enter arms it (and opens the calibration
  // panel), Enter on an already-armed session disarms it. The panel is only
  // ever shown while armed, because the live level it plots streams solely
  // during capture — typing `iris` must never open the microphone by itself.
  const [irisMode, setIrisMode] = useState(false);
  const [irisActive, setIrisActive] = useState(false);
  // boom mode — Enter on the `boom` row renders the audio-enhancement controller
  // (EQ / presets / boost) in the right preview column.
  const [boomMode, setBoomMode] = useState(false);
  const [boomFocus, setBoomFocus] = useState(false);
  // Uptime mode — Enter on the `uptime` row renders the live, microsecond-
  // animated uptime odometer in the right preview column.
  const [uptimeMode, setUptimeMode] = useState(false);
  const [uptimeFocus, setUptimeFocus] = useState(false);
  // Weather mode — Enter on the `weather` row renders the current-conditions +
  // 5-day forecast panel (animated) in the right preview column. The command
  // arg (`weather berlin`) is a city override; empty = IP-located.
  const [weatherMode, setWeatherMode] = useState(false);
  const [weatherFocus, setWeatherFocus] = useState(false);
  const [tokensMode, setTokensMode] = useState(false);
  const [tokensFocus, setTokensFocus] = useState(false);
  // Calendar mode — typing `calendar`/`cal` renders the month view in the
  // right preview column directly (like `sound`); Enter hands the arrow keys
  // to it (←→ month, ↑↓ year).
  const [calendarMode, setCalendarMode] = useState(false);
  const [calendarFocus, setCalendarFocus] = useState(false);
  // Clean mode — Enter on `clean` renders the category picker in the right
  // preview column (scan → checkbox selection → two-stage delete). Enter-only
  // (not while-typing) because the scan walks the whole cache dir.
  const [cleanMode, setCleanMode] = useState(false);
  const [cleanFocus, setCleanFocus] = useState(false);
  // Mirror of `cleanMode` for the global `clean-done` listener (scan/execute
  // survive Esc — toast when the panel is closed).
  const cleanModeRef = useRef(cleanMode);
  useEffect(() => {
    cleanModeRef.current = cleanMode;
  }, [cleanMode]);
  // Snitch mode — `snitch` (app blocker) / `snitch map` (world map). Enter
  // opens; the arg picks which view. macOS-only command.
  const [snitchMode, setSnitchMode] = useState<null | "apps" | "map">(null);
  const [snitchFocus, setSnitchFocus] = useState(false);
  // Shazam mode — `shazam` records ~10 s + identifies the song; `shazam
  // history` opens the recognized-songs history. null = closed.
  const [shazamMode, setShazamMode] = useState<null | "listen" | "history">(null);
  const [shazamFocus, setShazamFocus] = useState(false);
  // Mirror of `shazamMode` for the global `shazam-done` listener (reads the
  // live value without re-subscribing). null = the Shazam panel is closed, so
  // a background recognition that finished should surface as a status toast.
  const shazamModeRef = useRef(shazamMode);
  useEffect(() => {
    shazamModeRef.current = shazamMode;
  }, [shazamMode]);
  // Bumped on EVERY shazam dispatch. The mounted panel reacts to this — a
  // mode change alone can't cover the click on the row matching the current
  // mode (after the panel's internal Listen⇄History toggle the prop wouldn't
  // change at all), which left the click silently ignored.
  const [shazamSignal, setShazamSignal] = useState(0);
  // 2FA management overlay state (separate from `bpmMode`/`gameMode`
  // — same fullscreen-takeover pattern but with its own polling
  // lifecycle for live TOTP codes).
  const [totpMode, setTotpMode] = useState(false);
  // Cached TOTP entries + live codes for the `otp <query>` autocomplete.
  // Polled while the popup is visible so codes stay current in the list.
  const [totpEntries, setTotpEntries] = useState<import("./lib/totp").TotpEntry[]>([]);
  const [totpCodes, setTotpCodes] = useState<Map<number, import("./lib/totp").TotpCode>>(
    new Map(),
  );
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
  // Finder selection (macOS only) — populated by the Ctrl+Shift+F
  // hotkey via the `finder-selection-loaded` event. When non-null,
  // the popup is in "finder-mode": Finder files take over the list
  // and `rz <W>x<H>` runs against them instead of the clipboard.
  // `null` (not `[]`) is the inactive marker — `[]` means
  // "selection loaded, but Finder had nothing selected" → we still
  // want to show the empty-state hint.
  const [finderFiles, setFinderFiles] = useState<FinderFileView[] | null>(null);
  const [finderAutomationDenied, setFinderAutomationDenied] = useState(false);
  // App launcher (v0.37.0+). Indexed list of installed apps; the
  // backend scans once at startup. We fetch the list on popup mount
  // (cheap — already in memory) and run a fuzzy match per keystroke
  // entirely client-side. No icons until the row is selected.
  const [installedApps, setInstalledApps] = useState<AppEntry[]>([]);
  useEffect(() => {
    void listApps()
      .then(setInstalledApps)
      .catch(() => undefined);
  }, []);

  // Wakelock state for the footer LED (v0.36.0+). Loaded once on
  // mount; subsequently refreshed by the `wakelock-changed` event
  // emitted by the backend after every `wakelock_set` call. No
  // polling — purely event-driven.
  const [wakelockActive, setWakelockActive] = useState(false);
  // v0.39.0+: count of active timers, displayed in the footer next to
  // the wakelock LED. Refreshed by `timers-changed` + `timer-fired`
  // events the backend emits. Also fetched once on mount.
  const [activeTimerCount, setActiveTimerCount] = useState(0);
  // Timesheet tracking footer indicator — updated on mount + the
  // `track-status-changed` event the tracker emits.
  const [trackStatusState, setTrackStatusState] = useState<{
    active: boolean;
    paused: boolean;
  }>({
    active: false,
    paused: false,
  });
  // Brief banner when a timer fires (4 s dwell) — separate from the
  // OS-native macOS notification so the user gets feedback even if
  // the popup is open at fire time.
  const [timerFiredLabel, setTimerFiredLabel] = useState<string | null>(null);
  // pwgen mode toggle (v0.40.0+). Persists across keystrokes so the
  // user can type `pwgen 16`, click `dict` once in the preview pane,
  // then re-type lengths without losing the mode. Re-generation is
  // driven by query + mode + a `pwgenSeed` bump that activates on
  // mode-switch and on Enter (for explicit re-rolls).
  const [pwgenMode, setPwgenMode] = useState<PwgenMode>("all");
  const [pwgenSeed, setPwgenSeed] = useState(0);
  // User-edited password (v0.65.0). Keyed by the generation it belongs to
  // (seed/query/mode) so any regenerate / mode-switch / length-change
  // automatically discards a stale edit — no manual clearing needed.
  const [pwgenEdit, setPwgenEdit] = useState<{
    seed: number;
    query: string;
    mode: PwgenMode;
    value: string;
  } | null>(null);
  // True while the password field has focus — disables the global keyboard
  // nav so typing edits the password instead of moving the selection.
  const [pwgenEditing, setPwgenEditing] = useState(false);
  // Editing the selected snippet inline in the preview column (v0.84.265).
  // Holds the snippet id so a selection change can't leave a stale editor open.
  const [snippetEditingId, setSnippetEditingId] = useState<number | null>(null);
  // Bumped after an inline snippet edit — the snippet-match effect below keys on
  // `query`, which does NOT change when you save, so without this the preview
  // would keep showing the pre-edit body.
  const [snippetRev, setSnippetRev] = useState(0);
  const searchRef = useRef<HTMLInputElement>(null);
  const shellRef = useRef<HTMLDivElement>(null);

  // Dismiss = the reverse of the summon (§ signature transition): play the CRT
  // TV power-OFF on the shell, *then* hide the OS window — so closing mirrors
  // the power-on instead of snapping out. Under reduced motion / no WAAPI,
  // `playCrtOff` resolves instantly, so there's no added dismiss latency. (The
  // Rust focus-loss path hides without this on purpose — clicking away should
  // be immediate.) All `hidePopup()` callers route through here.
  // Hide-race hardening (v0.88.1): Esc key-repeat / fast close-reopen used to
  // leave STALE hide calls in flight — each awaits the 130 ms exit animation
  // before the IPC, so a straggler could fire AFTER the popup was re-shown and
  // close the fresh popup instantly. Guards: one hide in flight at a time, and
  // a show-generation check after the animation — re-shown meanwhile → the
  // stale hide is dropped. The animation await is capped at 200 ms so a hung
  // `finished` promise can never block hiding.
  const showGenRef = useRef(0);
  const hidingRef = useRef(false);
  const hidePopup = useCallback(async () => {
    if (hidingRef.current) return;
    hidingRef.current = true;
    const gen = showGenRef.current;
    try {
      await Promise.race([
        playCrtOff(shellRef.current),
        // Cap the await at the CRT power-off duration + margin so a hung
        // `finished` can't block hiding; the show-generation guard below drops
        // a straggler if the popup was re-shown meanwhile.
        new Promise((r) => window.setTimeout(r, CRT_OFF_MS + 90)),
      ]);
      if (showGenRef.current !== gen) return; // re-shown → stale hide, drop it
      await hidePopupRaw();
    } finally {
      hidingRef.current = false;
    }
  }, []);

  // Pulled once from tauri.conf.json via the core:app permission set.
  // Failure (e.g. browser dev preview without Tauri context) is silent —
  // the footer just hides the version chip.
  useEffect(() => {
    getVersion()
      .then(setVersion)
      .catch(() => undefined);
  }, []);

  // Pre-warm the shared audio context shortly after startup (popup window is
  // created hidden at launch, so this runs at a harmless moment). Without it
  // the context + its output unit spun up lazily on the FIRST bpm/disco start
  // — a CoreAudio device reconfiguration right while the user was listening
  // to music, which through the boom bridge audibly stuttered the playback.
  // Delayed a beat so it never competes with startup work. **Gated on boom
  // being enabled (v0.84.240):** the ring-drain the warm context protects
  // against only exists through boom's bridge — and a permanently running
  // silent output unit makes coreaudiod hold a PreventUserIdleSystemSleep
  // assertion (the Mac never idle-sleeps → overnight battery drain). Without
  // boom the context stays lazy, like pre-v0.84.238.
  useEffect(() => {
    const id = window.setTimeout(() => {
      void getBoomConfig()
        .then((cfg) => {
          if (!cfg.enabled) return;
          return import("./lib/warm-audio").then((m) => m.prewarmAudio());
        })
        .catch(() => undefined);
    }, 2000);
    return () => window.clearTimeout(id);
  }, []);

  // boom's idle gate (Rust) parks/wakes the warm context alongside its own
  // bridge: a suspended context releases the webview's sleep assertion AND
  // frees boom Audio of our own silent client so the gate's no-other-clients
  // probe can pass. `suspendWarmIfIdle` refuses while a mic is live (bpm/disco
  // running), which makes boom's probe fail and its bridge keep running.
  useEffect(() => {
    let cancelled = false;
    const unlistens: UnlistenFn[] = [];
    const hook = (event: string, run: (m: typeof import("./lib/warm-audio")) => void) => {
      void listen(event, () => {
        void import("./lib/warm-audio").then(run);
      }).then((u) => {
        if (cancelled) u();
        else unlistens.push(u);
      });
    };
    hook("warm-audio-suspend", (m) => m.suspendWarmIfIdle());
    hook("warm-audio-resume", (m) => m.resumeWarm());
    return () => {
      cancelled = true;
      unlistens.forEach((u) => u());
    };
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

  // Pinned-only view: a toggle in the history toolbar collapses the list to
  // just the pinned clips (★). While on, the whole custom-command/snippet/
  // launcher stack is suppressed — it's a pure "my pinned clips" browser (the
  // query still filters within them). `pinnedCount` (total pins, query-
  // independent) drives the toolbar badge.
  const [pinnedOnly, setPinnedOnly] = useState(false);
  const pinnedCount = useMemo(
    () => entries.reduce((n, e) => (e.pinned ? n + 1 : n), 0),
    [entries],
  );
  const togglePinnedOnly = useCallback(() => {
    setPinnedOnly((v) => !v);
    setSelected(0);
  }, []);

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
    if (snake) {
      setGameMode(snake === "wrap" ? "snake-wrap" : "snake-classic");
      return;
    }
    if (isSpaceInvadersTrigger(query)) {
      setGameMode("space");
      return;
    }
    if (isFlappyTrigger(query)) setGameMode("flappy");
  }, [query]);

  // Inline calculator: when the query parses as a math expression with at
  // least one operator/function/constant, surface the result as the top
  // list item (Alfred-style).
  const calcResult = useMemo(() => tryEvaluate(query), [query]);
  // Inline unit / number-base / epoch converter — renders as a calc row too.
  // Only consulted when the calculator itself didn't match (they're mutually
  // exclusive in practice: `5 km in mi` has no operator, `2+2` has no unit).
  const convertResult = useMemo(
    () => (calcResult ? null : tryConvert(query)),
    [query, calcResult],
  );
  // Inline hex-color preview: same idea — when the query parses as a
  // hex color (#RGB, #RGBA, RRGGBB, RRGGBBAA, …) prepend a color row.
  // Calc and color are mutually exclusive in practice (a math expression
  // can't also be a valid hex literal because of operator characters).
  const colorResult = useMemo(() => tryParseColor(query), [query]);

  // Power-command palette: parse the query into either a complete
  // command (runnable on Enter) or autocomplete suggestions.
  // Gate commands by OS: a command whose backend doesn't exist on this
  // platform (e.g. `freeze` off macOS) is dropped here so it never surfaces as
  // a runnable row or a suggestion — the user can't trigger a guaranteed
  // failure. On macOS every command is available, so this is a no-op there.
  const parsedCommand: ParsedCommand | null = useMemo(() => {
    const parsed = parseCommand(query);
    return parsed && isCommandAvailable(parsed.spec) ? parsed : null;
  }, [query]);
  const commandSuggestionList = useMemo(
    () => commandSuggestions(query).filter((c) => isCommandAvailable(c)),
    [query],
  );

  // ── Live translation preview (v0.93.0) ────────────────────────────────────
  // When a `tr*` command is being typed, fetch the translation in the
  // background (debounced + cached) and show it in the preview — no need to
  // press Enter. Enter copies the translation to the clipboard; Shift+Enter
  // opens Google Translate in the browser. A failed fetch (timeout/rate-limit/
  // network) leaves Shift+Enter as the browser fallback. Backed by
  // `translate_text` (Google gtx → MyMemory, both keyless).
  type LiveTranslation =
    | { status: "loading" }
    | { status: "ok"; text: string; provider: string; detectedSource: string }
    | { status: "error" };
  const [liveTranslation, setLiveTranslation] = useState<LiveTranslation | null>(null);

  // Lineage rails in the history list (v0.93.1). Re-read whenever the History
  // tab comes back into view — the only place the setting can be changed is the
  // Settings tab, so returning here is exactly when a fresh value is needed
  // (no event plumbing, and it can't go stale).
  const [lineageHighlight, setLineageHighlight] = useState(true);
  useEffect(() => {
    if (activeTab !== "history") return;
    void getLineageHighlight().then(setLineageHighlight).catch(() => undefined);
  }, [activeTab]);
  const translateCacheRef = useRef<Map<string, { text: string; provider: string; detectedSource: string }>>(
    new Map(),
  );
  const translateSeqRef = useRef(0);
  // The active translate request as a stable signature (kind + arg), null when
  // the current input isn't a translate command.
  const translateReq = useMemo(() => {
    const cmd = parsedCommand;
    if (!cmd || !isTranslateKind(cmd.spec.kind)) return null;
    const pair = TRANSLATE_LANGS[cmd.spec.kind];
    if (!pair) return null;
    const text = cmd.arg.trim();
    if (!text) return null;
    return { sl: pair.sl, tl: pair.tl, text };
  }, [parsedCommand]);
  const translateKey = translateReq
    ? `${translateReq.sl}|${translateReq.tl}|${translateReq.text}`
    : null;
  useEffect(() => {
    if (!translateReq || translateKey === null) {
      setLiveTranslation(null);
      return;
    }
    // Cache hit → show instantly (identical repeated queries never re-fetch).
    const cached = translateCacheRef.current.get(translateKey);
    if (cached) {
      setLiveTranslation({ status: "ok", ...cached });
      return;
    }
    const seq = ++translateSeqRef.current;
    setLiveTranslation({ status: "loading" });
    const timer = window.setTimeout(() => {
      translateText(translateReq.text, translateReq.sl, translateReq.tl)
        .then((r) => {
          if (seq !== translateSeqRef.current) return; // a newer query superseded this
          const entry = { text: r.text, provider: r.provider, detectedSource: r.detected_source };
          translateCacheRef.current.set(translateKey, entry);
          setLiveTranslation({ status: "ok", ...entry });
        })
        .catch(() => {
          if (seq === translateSeqRef.current) setLiveTranslation({ status: "error" });
        });
    }, 350);
    return () => window.clearTimeout(timer);
    // translateReq is derived from translateKey; depend on the stable key only.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [translateKey]);

  // ── Inline help (`?`): render command docs in the preview column ──────
  // A trailing `?` on a command/prefix (or `?` alone) asks for help; the
  // parser rejects a literal `?` (quotes/globs/URLs/args). When active, the
  // preview shows <CommandHelp> (highest precedence) and the left list is
  // blanked for the whole-index case so the docs stand alone.
  const helpTarget: HelpTarget | null = useMemo(() => parseHelpQuery(query), [query]);
  // `?`-index rows for the LEFT list: every documented command (optionally
  // full-text-filtered via `? <term>`), navigable with ↑/↓ — the preview
  // renders the selected row's full doc live (keyboard-first help, v0.87.2).
  const helpEntries: ListEntry[] = useMemo(() => {
    if (helpTarget?.kind !== "index") return [];
    return searchDocs(helpTarget.filter).map((d) => ({
      kind: "help",
      data: { command: d.command, tagline: d.tagline, category: d.category },
    }));
  }, [helpTarget]);

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
    // Cap at KILL_LIST_CAP — anything more is noise; refine the pattern.
    return filterKillProcesses(processSnapshot, killArgs.pattern)
      .slice(0, KILL_LIST_CAP)
      .map(
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

  // Meme mode — `meme [query]` takes over the list with the meme library,
  // fuzzy-filtered by the query. The library is fetched once on entering the
  // mode (a recursive folder scan; cheap but no point per-keystroke). The
  // selected row previews (animated) on the right; Enter copies the file.
  const isMemeMode = parsedCommand?.spec.kind === "meme";
  const memeArg = isMemeMode ? parsedCommand!.arg : "";
  const [memeLibrary, setMemeLibrary] = useState<MemeEntry[]>([]);
  useEffect(() => {
    if (!isMemeMode) {
      setMemeLibrary([]);
      return;
    }
    let cancelled = false;
    listMemes()
      .then((m) => {
        if (!cancelled) setMemeLibrary(m);
      })
      .catch((e) => {
        console.error("list_memes failed", e);
        if (!cancelled) setMemeLibrary([]);
      });
    return () => {
      cancelled = true;
    };
  }, [isMemeMode]);

  const memeEntries: ListEntry[] = useMemo(() => {
    if (!isMemeMode) return [];
    return matchMemes(memeArg, memeLibrary).map(
      (m): ListEntry => ({ kind: "meme", data: m }),
    );
  }, [isMemeMode, memeArg, memeLibrary]);

  // ── Figlet-mode: whole-list font gallery (like meme/kill) ────────────
  // The left list becomes font rows (each a live sample of the text); the big
  // banner of the selected font renders in the preview. Enter copies it.
  const isFigletMode = parsedCommand?.spec.kind === "figlet";
  const figletArg = isFigletMode ? parsedCommand!.arg : "";
  const figletParsed = useMemo(() => parseFigletCommand(figletArg), [figletArg]);

  const [figletFontList, setFigletFontList] = useState<FigletFontMeta[]>([]);
  const [figletDefaults, setFigletDefaults] = useState<FigletDefaults | null>(null);
  // Load the catalogue + defaults on first entry into figlet mode; refresh on
  // the defaults-changed event (Settings → Figlet).
  useEffect(() => {
    if (!isFigletMode || figletFontList.length > 0) return;
    void figletFonts().then(setFigletFontList).catch(() => undefined);
    void figletGetDefaults().then(setFigletDefaults).catch(() => undefined);
  }, [isFigletMode, figletFontList.length]);
  useEffect(() => {
    let cancelled = false;
    let unlisten: UnlistenFn | undefined;
    void listen("figlet-defaults-changed", () => {
      void figletFonts().then(setFigletFontList).catch(() => undefined);
      void figletGetDefaults().then(setFigletDefaults).catch(() => undefined);
    }).then((u) => {
      if (cancelled) u();
      else unlisten = u;
    });
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, []);

  // Effective render opts = defaults ← command --flags ← chip overrides.
  const [figletOptsOverride, setFigletOptsOverride] = useState<Partial<FigletOpts>>({});
  const figletOpts: FigletOpts = useMemo(() => {
    const base: FigletOpts = figletDefaults
      ? optsFromDefaults(figletDefaults)
      : { width: 80, align: "left", trim: true, comment: "none", boxed: false };
    return { ...base, ...figletParsed.opts, ...figletOptsOverride };
  }, [figletDefaults, figletParsed.opts, figletOptsOverride]);
  useEffect(() => {
    if (!isFigletMode) setFigletOptsOverride({});
  }, [isFigletMode]);

  // Ordered gallery: pinned→popular→rest (or fuzzy matches for a @font query);
  // the default font floats to the top when there's no query. Capped for perf.
  const figletOrderedFonts = useMemo(() => {
    const ordered = galleryFonts(figletParsed.fontQuery, figletFontList);
    if (!figletParsed.fontQuery && figletDefaults) {
      const i = ordered.findIndex((f) => f.name === figletDefaults.font);
      if (i > 0) {
        const [d] = ordered.splice(i, 1);
        ordered.unshift(d);
      }
    }
    return ordered.slice(0, FIGLET_MAX_ROWS);
  }, [figletParsed.fontQuery, figletFontList, figletDefaults]);

  // Batched, debounced gallery samples for the visible fonts (cached per text).
  // Long text → name-only rows (the big preview still renders the selection).
  const [figletSamples, setFigletSamples] = useState<Map<string, string>>(new Map());
  useEffect(() => {
    if (!isFigletMode) return;
    const text = figletParsed.text;
    if (!text.trim() || text.length > FIGLET_SAMPLE_TEXT_CAP) {
      setFigletSamples(new Map());
      return;
    }
    const names = figletOrderedFonts.map((f) => f.name);
    if (names.length === 0) return;
    let alive = true;
    const t = window.setTimeout(() => {
      figletGallery(text, names, 3, 44)
        .then((samples) => {
          if (!alive) return;
          const m = new Map<string, string>();
          for (const s of samples) m.set(s.font, s.sample);
          setFigletSamples(m);
        })
        .catch(() => undefined);
    }, 120);
    return () => {
      alive = false;
      window.clearTimeout(t);
    };
  }, [isFigletMode, figletParsed.text, figletOrderedFonts]);

  const figletEntries: ListEntry[] = useMemo(() => {
    if (!isFigletMode) return [];
    return figletOrderedFonts.map(
      (f): ListEntry => ({
        kind: "figlet-font",
        data: { ...f, sample: figletSamples.get(f.name) ?? "" },
      }),
    );
  }, [isFigletMode, figletOrderedFonts, figletSamples]);

  // Leave brightness mode automatically once the query is no longer the
  // `brightness` command (e.g. the user cleared / retyped the search field).
  const isBrightnessCmd = parsedCommand?.spec.kind === "brightness";
  useEffect(() => {
    if (!isBrightnessCmd && brightnessMode) {
      setBrightnessMode(false);
      setBrightnessFocus(false);
    }
  }, [isBrightnessCmd, brightnessMode]);

  // Same auto-exit for sound mode.
  const isSoundCmd = parsedCommand?.spec.kind === "sound";
  useEffect(() => {
    // Show the audio panel in the preview *directly* when `sound`/`audio` is
    // typed (no Enter needed to see it); Enter only hands it keyboard focus.
    if (isSoundCmd && !soundMode) {
      setSoundMode(true);
    } else if (!isSoundCmd && soundMode) {
      setSoundMode(false);
      setSoundFocus(false);
    }
  }, [isSoundCmd, soundMode]);

  // Same auto-exit for hue mode.
  const isHueCmd = parsedCommand?.spec.kind === "hue";
  useEffect(() => {
    if (!isHueCmd && hueMode) {
      setHueMode(false);
      setHueFocus(false);
    }
  }, [isHueCmd, hueMode]);

  // Same auto-exit for stats mode.
  const isStatsCmd = parsedCommand?.spec.kind === "stats";
  useEffect(() => {
    if (!isStatsCmd && statsMode) {
      setStatsMode(false);
      setStatsFocus(false);
    }
  }, [isStatsCmd, statsMode]);

  // Same auto-exit for iris mode (the panel, not the monitoring — that keeps
  // running in the background on purpose).
  const isIrisCmd = parsedCommand?.spec.kind === "iris";
  useEffect(() => {
    if (!isIrisCmd && irisMode) {
      setIrisMode(false);
    }
  }, [isIrisCmd, irisMode]);

  // While armed, the number typed in the search bar retunes the live session
  // (debounced) — the same "the argument drives it" model as `weather`. This
  // replaces the old ←/→ retune, which required giving the panel keyboard
  // focus and was exactly what broke the toggle.
  const irisArg = isIrisCmd ? (parsedCommand?.arg ?? "") : "";
  useEffect(() => {
    if (!isIrisCmd || !irisActive) return;
    const a = parseIrisArg(irisArg);
    if (a.kind !== "on") return;
    const t = setTimeout(() => {
      void irisSetThreshold(a.threshold).catch(() => undefined);
    }, 300);
    return () => clearTimeout(t);
  }, [isIrisCmd, irisActive, irisArg]);

  // Keep the armed/disarmed flag honest so the command row can say which way
  // Enter will flip. Re-read whenever the `iris` command comes into view.
  useEffect(() => {
    if (!isIrisCmd) return;
    let alive = true;
    irisStatus()
      .then((st) => {
        if (alive) setIrisActive(!!st?.active);
      })
      .catch(() => {});
    return () => {
      alive = false;
    };
  }, [isIrisCmd]);

  // Same auto-exit for boom mode.
  const isBoomCmd = parsedCommand?.spec.kind === "boom";
  useEffect(() => {
    if (!isBoomCmd && boomMode) {
      setBoomMode(false);
      setBoomFocus(false);
    }
  }, [isBoomCmd, boomMode]);

  // Same auto-exit for uptime mode.
  const isUptimeCmd = parsedCommand?.spec.kind === "uptime";
  useEffect(() => {
    if (!isUptimeCmd && uptimeMode) {
      setUptimeMode(false);
      setUptimeFocus(false);
    }
  }, [isUptimeCmd, uptimeMode]);

  // Same auto-exit for weather mode (Enter-activated — it does network I/O, so
  // it must not fire on every keystroke; once open, the city arg drives refetch).
  const isWeatherCmd = parsedCommand?.spec.kind === "weather";
  useEffect(() => {
    if (!isWeatherCmd && weatherMode) {
      setWeatherMode(false);
      setWeatherFocus(false);
    }
  }, [isWeatherCmd, weatherMode]);
  const isTokensCmd = parsedCommand?.spec.kind === "tokens";
  useEffect(() => {
    if (!isTokensCmd && tokensMode) {
      setTokensMode(false);
      setTokensFocus(false);
    }
  }, [isTokensCmd, tokensMode]);

  // Random mode shows the rolled number directly in the preview while `rnd`/
  // `random` is typed (like `calendar`); a Roll-again button re-rolls, Enter
  // pastes the shown number. `randomValueRef` holds what the panel currently
  // shows so the dispatch pastes exactly it.
  const [randomMode, setRandomMode] = useState(false);
  const randomValueRef = useRef<number | null>(null);
  const isRandomCmd = parsedCommand?.spec.kind === "random";
  useEffect(() => {
    if (isRandomCmd && !randomMode) setRandomMode(true);
    else if (!isRandomCmd && randomMode) setRandomMode(false);
  }, [isRandomCmd, randomMode]);

  // Calendar mode shows directly while `calendar`/`cal` is typed (no Enter
  // needed — like `sound`); Enter only hands it keyboard focus.
  const isCalendarCmd = parsedCommand?.spec.kind === "calendar";
  useEffect(() => {
    if (isCalendarCmd && !calendarMode) {
      setCalendarMode(true);
    } else if (!isCalendarCmd && calendarMode) {
      setCalendarMode(false);
      setCalendarFocus(false);
    }
  }, [isCalendarCmd, calendarMode]);

  // Clean mode auto-exits when the query is no longer the `clean` command
  // (it is entered via Enter, not while typing — the scan is heavy).
  const isCleanCmd = parsedCommand?.spec.kind === "clean";
  useEffect(() => {
    if (!isCleanCmd && cleanMode) {
      setCleanMode(false);
      setCleanFocus(false);
    }
  }, [isCleanCmd, cleanMode]);

  // Snitch mode auto-exits when the query is no longer the `snitch` command.
  const isSnitchCmd = parsedCommand?.spec.kind === "snitch";
  useEffect(() => {
    if (!isSnitchCmd && snitchMode) {
      setSnitchMode(null);
      setSnitchFocus(false);
    }
  }, [isSnitchCmd, snitchMode]);

  // Shazam mode auto-exits when the query is no longer the `shazam` command
  // (Enter-activated — it opens the mic, not shown while typing).
  const isShazamCmd = parsedCommand?.spec.kind === "shazam";
  useEffect(() => {
    if (!isShazamCmd && shazamMode) {
      setShazamMode(null);
      setShazamFocus(false);
    }
  }, [isShazamCmd, shazamMode]);

  // Faker: catalogue (fetched once, cached for pure-TS parsing) + defaults
  // (re-fetched on the settings-changed event) + a reroll signal (⌘/Ctrl+R).
  // Declared before `commandEntry` because its faker case reads them.
  const [fakerCat, setFakerCat] = useState<CatalogEntry[]>([]);
  const [fakerDefaults, setFakerDefaults] = useState<FakerDefaults | null>(null);
  const [fakerReroll, setFakerReroll] = useState(0);
  useEffect(() => {
    void fakerCatalog().then(setFakerCat).catch(() => undefined);
    void fakerGetDefaults().then(setFakerDefaults).catch(() => undefined);
    let cancelled = false;
    let unlisten: UnlistenFn | undefined;
    void listen("faker-defaults-changed", () => {
      void fakerGetDefaults().then(setFakerDefaults).catch(() => undefined);
      void fakerCatalog().then(setFakerCat).catch(() => undefined);
    }).then((u) => {
      if (cancelled) u();
      else unlisten = u;
    });
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, []);
  const fakerDef: FakerDefaults = useMemo(
    () =>
      fakerDefaults ?? {
        locale: "DE_DE",
        count: 1,
        format: "plain",
        pinned: [],
        save_history: true,
      },
    [fakerDefaults],
  );

  // Security builders: catalogue (fetched once) + defaults (event-refreshed).
  const [secCat, setSecCat] = useState<SecCatalog>({ tools: [], common_wordlists: [], john_formats: [] });
  const [secDefaultsRaw, setSecDefaultsRaw] = useState<SecDefaults | null>(null);
  useEffect(() => {
    void secCatalog().then(setSecCat).catch(() => undefined);
    void secGetDefaults().then(setSecDefaultsRaw).catch(() => undefined);
    let cancelled = false;
    let unlisten: UnlistenFn | undefined;
    void listen("sec-defaults-changed", () => {
      void secGetDefaults().then(setSecDefaultsRaw).catch(() => undefined);
    }).then((u) => {
      if (cancelled) u();
      else unlisten = u;
    });
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, []);
  const secDef: SecDefaults = useMemo(
    () =>
      secDefaultsRaw ?? {
        wordlist: "",
        output_dir: "",
        timing: "",
        threads: 0,
        rate: 0,
        john_line: "jumbo",
        terminal: "iterm",
        auto_enter: false,
        scope_note: "",
        save_history: true,
      },
    [secDefaultsRaw],
  );

  const commandEntry: ListEntry | null = useMemo(() => {
    if (!parsedCommand) return null;
    // kill / meme take over the whole list, not a single command row.
    if (parsedCommand.spec.kind === "kill") return null;
    if (parsedCommand.spec.kind === "meme") return null;
    const { spec, arg } = parsedCommand;
    let label: string;
    let hint: string;
    switch (spec.kind) {
      case "translate-en":
      case "translate-de":
      case "translate-auto":
      case "translate-de-it":
      case "translate-it-de":
      case "translate-de-es":
      case "translate-es-de":
      case "translate-de-pl":
      case "translate-pl-de": {
        const pair = TRANSLATE_LANGS[spec.kind]!;
        label = `Translate to ${pair.target}: "${arg}"`;
        hint = "⏎ copies translation · ⇧⏎ opens Google Translate";
        break;
      }
      case "websearch": {
        const bang = SEARCH_BANGS[spec.keyword];
        label = `Search ${bang?.name ?? spec.keyword}: "${arg}"`;
        hint = `Opens ${bang?.name ?? "the search"} results in your browser`;
        break;
      }
      case "uuid": {
        const n = Math.max(1, Math.min(parseInt(arg, 10) || 1, 100));
        label = `Generate ${n} UUID${n > 1 ? "s" : ""} → clipboard`;
        hint = "Random v4 UUIDs (CSPRNG), one per line";
        break;
      }
      case "slug":
        label = `Slugify: "${arg}" → "${slugify(arg)}"`;
        hint = "URL-safe lowercase slug → clipboard";
        break;
      case "hash":
        label = `SHA-256: "${arg}" → clipboard`;
        hint = "Hex SHA-256 digest of the text";
        break;
      case "json":
        label = "Pretty-print clipboard JSON → clipboard";
        hint = "Parses + re-indents the JSON on your clipboard (2 spaces)";
        break;
      case "jwt":
        label = "Decode clipboard JWT → clipboard";
        hint = "Base64url-decodes the header + payload (no signature check)";
        break;
      case "qr":
        label = `QR code: "${arg}"`;
        hint = "Preview on the right · Enter copies the PNG to the clipboard";
        break;
      case "faker": {
        // A complete generator → runnable command row (+ FakerPreview on the
        // right). Bare `faker` / a partial / unknown name yields no command row
        // — the catalogue suggestions (fakerCatalogEntries) cover those.
        const parsed = parseFakerCommand(arg, fakerCat, fakerDef);
        if (parsed.kind !== "spec") return null;
        const s = parsed.spec;
        label =
          s.mode === "template"
            ? `Template · ${s.n}×`
            : `${s.n} × ${s.generator} · ${s.format}`;
        hint = "Preview on the right · Enter → clipboard + paste · ⌘/Ctrl+R rerolls";
        break;
      }
      case "sec": {
        // A complete tool+preset → runnable command row (+ SecPreview). Bare
        // `sec`/`nmap`, a preset list, or non-command prose yield no row (the
        // preset suggestions / history cover those).
        const parsed = parseSecCommand(spec.keyword, arg, secCat, secDef);
        if (parsed.kind !== "built") return null;
        label = parsed.command;
        hint = parsed.sharp
          ? "⏎ Copy · ⌘⏎ Run in terminal (confirms — sharp preset)"
          : "⏎ Copy · ⌘⏎ Run in terminal";
        break;
      }
      case "resize": {
        const dims = parseResizeArg(arg);
        label = dims
          ? `Resize selected image(s) → ${dims.width}×${dims.height}`
          : `rz: invalid dimensions ("${arg}" — expected W×H, e.g. 1200x800)`;
        hint = dims
          ? "Finder/Explorer selection → <name>-WxH (Lanczos3); else the clipboard image"
          : "Use format like 1200x800";
        break;
      }
      case "optim":
        label = "Compress the selected image(s)";
        hint =
          "Finder/Explorer selection → <name>-optim (PNG lossless · JPEG re-encoded); else the clipboard PNG";
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
      case "freeze":
        label = "Block all input — unlock with the chord";
        hint =
          "Press the configured chord (Settings → Input Lock, default i+r) to unlock";
        break;
      case "wakelock": {
        const on = parseWakelockArg(arg);
        if (on === null) {
          label = `wakelock: use "on" or "off" (got "${arg}")`;
          hint = "e.g. wakelock on · wakelock off · caffeine on";
        } else {
          label = on
            ? "Keep awake: ON — pause sleep & screen lock"
            : "Keep awake: OFF — normal sleep behaviour resumes";
          hint = on ? "Stays awake until you run wakelock off" : "";
        }
        break;
      }
      case "touch": {
        const gt = arg.indexOf(">");
        const tname = (gt >= 0 ? arg.slice(0, gt) : arg).trim();
        const tcontent = gt >= 0 ? arg.slice(gt + 1).trim() : "";
        label = tcontent
          ? `Create "${tname}" with content in the open folder`
          : `Create file "${tname}" in the open folder`;
        hint =
          "Frontmost Explorer (Windows) / Finder (macOS) folder · `touch name > text` writes content";
        break;
      }
      case "mkdir":
        label = `Create folder "${arg}" in the open folder`;
        hint = "In the frontmost Explorer (Windows) / Finder (macOS) window's directory";
        break;
      case "terminal":
        label = "Open a terminal in the open folder";
        hint =
          "Windows Terminal/PowerShell (Windows) · iTerm2/Terminal (macOS) · frontmost folder";
        break;
      case "timer": {
        const t = parseTimerArg(arg);
        if (!t) {
          label = `Timer: invalid format ("${arg}") — use 12, 30s, 12 min, 2h`;
          hint = "Default unit is minutes when none given";
        } else {
          label = `Start timer · ${t.label}`;
          hint = "macOS native notification + Glass sound when it fires";
        }
        break;
      }
      case "alarm": {
        const a = parseAlarmArg(arg);
        if (!a) {
          label = `Alarm: invalid time ("${arg}") — use HH:MM, e.g. 3:00 or 15:15`;
          hint = "24-hour clock · fires at the next occurrence";
        } else {
          label = `Set alarm · ${a.label}`;
          hint = "Fires at the next occurrence (today or tomorrow)";
        }
        break;
      }
      case "md2pdf":
        label = arg
          ? `Markdown → PDF: "${arg}"`
          : "Markdown → PDF (file-manager selection)";
        hint = "Same as Ctrl+Shift+M · PDF lands next to the source";
        break;
      case "shot-region": {
        const delay = parseShotDelay(arg);
        label = delay > 0 ? `Screenshot a region in ${delay}s` : "Screenshot a region";
        hint = "Drag a marquee → floating preview (save / edit / pin)";
        break;
      }
      case "shot-full": {
        const delay = parseShotDelay(arg);
        label =
          delay > 0
            ? `Screenshot the whole screen in ${delay}s`
            : "Screenshot the whole screen";
        hint = "Full screen → floating preview";
        break;
      }
      case "shot-window": {
        const delay = parseShotDelay(arg);
        label =
          delay > 0
            ? `Screenshot the active window in ${delay}s`
            : "Screenshot the active window";
        hint = "Active window → floating preview";
        break;
      }
      case "shot-last":
        label = "Repeat the last screenshot mode";
        hint = "Re-runs whichever capture mode you used last";
        break;
      case "clean":
        label = "Clean caches / logs / temp files";
        hint = "Enter opens the picker — choose categories, then delete";
        break;
      case "snitch": {
        const mapMode = /\b(map|conn|show|world)/i.test(arg ?? "");
        label = mapMode ? "Connections world map" : "Network monitor — block apps";
        hint = mapMode
          ? "Live outbound connections plotted on a world map"
          : "Toggle apps' internet access (best-effort) · `snitch map` for the map";
        break;
      }
      case "shazam": {
        const histMode = /\b(hist|list)/i.test(arg ?? "");
        label = histMode ? "Song history" : "Identify the song playing";
        hint = histMode
          ? "Your recognized songs — open in Shazam / Spotify / YouTube"
          : "Records ~10 s from the mic · `shazam history` for past songs";
        break;
      }
      case "brightness":
        label = "Adjust monitor brightness";
        hint = "Opens a slider per DDC monitor (external displays)";
        break;
      case "sound":
        label = "Audio output + volume";
        hint =
          "Shown in the preview — Enter to control: ←→ volume ∓5 · ↑↓ device · Enter switch";
        break;
      case "hue":
        label = "Control Philips Hue lamps";
        hint =
          "Enter → lamp controls in the preview: all-lamps switch + brightness + colour";
        break;
      case "iris": {
        const a = parseIrisArg(arg);
        label = irisRowLabel(a, irisActive);
        hint = irisActive
          ? "Enter → Schwelle ändern · Ränder glimmen rot, wenn das Mikro sie überschreitet"
          : "Enter → Mikro überwachen; Bildschirmränder glimmen rot über der Schwelle";
        break;
      }
      case "stats":
        label = "Live system stats";
        hint =
          "Enter → CPU / memory / disks / network / temps / fans / battery in the preview";
        break;
      case "boom":
        label = "Audio enhancement";
        hint = "Enter → system EQ, presets & volume boost in the preview";
        break;
      case "uptime":
        label = "Live system uptime";
        hint = "Enter → uptime animated to the microsecond in the preview";
        break;
      case "weather":
        label = arg ? `Weather: ${arg}` : "Weather — your location";
        hint = "Enter → current conditions + 5-day forecast, animated in the preview";
        break;
      case "tokens":
        label = "Claude Code token usage";
        hint =
          "Enter → cost, projects, sessions & models from the local Token Tracker";
        break;
      case "calendar":
        label = arg ? `Calendar: ${arg}` : "Calendar — month view";
        hint = "Enter to navigate: ←→ month · ↑↓ year · T today — or type e.g. märz 1990";
        break;
      case "track": {
        const m = parseTrackArg(arg);
        label =
          m === "on"
            ? "Start time tracking"
            : m === "off"
              ? "Stop time tracking"
              : m === "open"
                ? "Time tracking — open the timesheet"
                : `track: use "on" or "off"`;
        hint = m
          ? "Records app usage by focus; idle auto-pauses · encrypted at rest"
          : "e.g. track on · track off";
        break;
      }
      case "settings": {
        const section = matchSettingsSection(arg);
        label = section ? `Open Settings → ${section.label}` : "Open Settings";
        hint = section
          ? "Jumps straight to the section"
          : "Optional: section name — e.g. settings cue · settings hotkeys · settings bruno";
        break;
      }
      case "trim":
        label = arg ? `Trim media: "${arg}"` : "Trim a video / audio file";
        hint = "Opens a file picker → set in/out points (lossless or re-encode)";
        break;
      case "disco": {
        const on = parseDiscoArg(arg);
        const willRun = on === null ? !discoEngine.isRunning() : on;
        label = willRun ? "Beat-sync Hue lamps to the mic" : "Stop the Hue beat-sync";
        hint = willRun
          ? "Pulses your lamps to music; keeps running after the popup closes"
          : "disco 1 = on · disco 0 = off · bare disco = toggle";
        break;
      }
      case "random": {
        const r = parseRandomArg(arg);
        label = r
          ? `Roll a random number · ${r.min}–${r.max}`
          : `random: invalid arg ("${arg}") — a number, two numbers, or a-b`;
        hint = r
          ? "Shown in the preview · Roll again to re-roll · Enter pastes it"
          : "e.g. rnd · rnd 100 · rnd 5 500 · rnd 1-2";
        break;
      }
      case "pwgen":
        // Pwgen is rendered as its own ListEntry kind further down (with `mode`
        // + `password` baked in) — opt out of the generic command row.
        return null;
      case "bruno":
        // Bruno has its own `bruno` ListEntry (live net-pay calc) — opt out.
        return null;
      default:
        // ROBUST FALLBACK (v0.84.63): any command kind WITHOUT a tailored case
        // above still gets a runnable command row, so every custom command keeps
        // its always-highest priority (spliced above fuzzy clips) and its red
        // accent — even if someone forgets to add a tailored case. (This is what
        // the v0.84.59 `stats`/`trim` bug was: `default` used to `return null`,
        // so a forgotten kind produced NO row and a clip won Enter.) Only kinds
        // with a DEDICATED `ListEntry` opt out via an explicit `return null`
        // above (`pwgen`, `bruno`); whole-list takeovers (`kill`, `meme`) are
        // handled before this switch. Tailored cases above just give nicer
        // labels/hints than this generic fallback.
        label = arg ? `${spec.keyword} ${arg}` : spec.syntax;
        hint = spec.description;
        break;
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
  }, [parsedCommand, query, fakerCat, fakerDef, secCat, secDef, irisActive]);

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
  // Last ←/→ switch direction, fed into the preview's directional slide-in.
  const [openerDir, setOpenerDir] = useState<1 | -1>(1);
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
    return { kind: "opener", data: { text: TOP_OPENERS[openerIndex], dir: openerDir } };
  }, [openerIndex, openerDir]);

  const suggestionEntries: ListEntry[] = useMemo(
    () =>
      commandSuggestionList.map((spec) => ({
        kind: "command-suggestion",
        data: {
          keyword: spec.keyword,
          syntax: spec.syntax,
          description: spec.description,
          // Append a trailing space whenever the command takes an argument
          // (required OR optional — detected from the syntax having more than
          // just the keyword), so completing e.g. `pwgen` lands the caret at
          // `pwgen ` ready for the length. Arg-less commands (optim, lock, …)
          // stay bare.
          completion:
            spec.syntax.trim() !== spec.keyword ? `${spec.keyword} ` : spec.keyword,
        },
      })),
    [commandSuggestionList],
  );

  // Context-aware presets — currently just resize (`rz <preset>`).
  // Surface as `command-suggestion` rows so the existing nav UX
  // (highlight, Tab/→ to autocomplete, Enter to run) just works.
  // Enter on a *complete* command-suggestion runs it (see activate
  // handler) rather than just filling the input.
  const resizePresetEntries: ListEntry[] = useMemo(() => {
    const presets = resizePresetSuggestions(query);
    return presets.map(
      (p): ListEntry => ({
        kind: "command-suggestion",
        data: {
          keyword: p.label.split(" · ")[0],
          syntax: p.label,
          description: p.description,
          completion: p.completion,
        },
      }),
    );
  }, [query]);

  // City autocomplete for `weather <partial>` (v0.100.0). While typing a city
  // (before the panel takes focus), matching major cities surface as
  // command-suggestion rows — Tab/→ fills `weather <City>`, Enter completes AND
  // opens the panel (both reuse the generic command-suggestion handlers). They
  // REPLACE the raw `weather <partial>` command row in `combined` when present,
  // so the top (biggest) city is the default selection.
  const weatherArg = isWeatherCmd ? (parsedCommand?.arg ?? "") : "";
  const weatherCitySuggestionEntries: ListEntry[] = useMemo(() => {
    if (!isWeatherCmd || weatherFocus) return [];
    const q = weatherArg.trim();
    if (q.length < 1) return [];
    return matchCities(q, 6).map(
      (c): ListEntry => ({
        kind: "command-suggestion",
        data: {
          keyword: c.name,
          syntax: `${c.name} · ${c.country}`,
          description: `Enter or Tab → weather in ${c.name}`,
          completion: `weather ${c.name}`,
        },
      }),
    );
  }, [isWeatherCmd, weatherFocus, weatherArg]);

  // Faker catalogue rows (bare `faker` list, or fuzzy matches for a partial /
  // unknown generator). Rendered as command-suggestion rows (like the resize
  // presets): each shows the generator + a live sample, Tab/→ fills the name,
  // Enter runs it. Suppressed once the arg is a complete spec (the command row
  // + FakerPreview take over then).
  const fakerCatalogEntries: ListEntry[] = useMemo(() => {
    if (!parsedCommand || parsedCommand.spec.kind !== "faker") return [];
    const arg = parsedCommand.arg;
    const parsed = parseFakerCommand(arg, fakerCat, fakerDef);
    if (parsed.kind === "spec") return []; // complete → command row handles it
    // Fuzzy on the first word (the generator candidate); bare → whole catalogue,
    // pinned generators floated to the top.
    const first = arg.trim().split(/\s+/)[0] ?? "";
    const pinned = new Set(fakerDef.pinned);
    const matches = matchCatalog(first, fakerCat);
    const ordered = [
      ...matches.filter((e) => pinned.has(e.name)),
      ...matches.filter((e) => !pinned.has(e.name)),
    ];
    // Show the whole catalogue for bare `faker` (composites live at the end —
    // the productivity lever — so no low cap that would hide them); the list
    // narrows as the user types.
    return ordered.slice(0, 100).map(
      (e): ListEntry => ({
        kind: "command-suggestion",
        data: {
          keyword: e.name,
          syntax: `${e.name}${e.composite ? " (record)" : ""}`,
          description: `${e.description} — e.g. ${e.sample}`,
          completion: `faker ${e.name} `,
        },
      }),
    );
  }, [parsedCommand, fakerCat, fakerDef]);

  // Security-builder rows: the 4-tool overview (bare `sec`) or a tool's preset
  // list (`nmap`, `sec nmap`, partial preset). Command-suggestion rows — Tab/→
  // fills, Enter opens. Suppressed once a full command is built (SecPreview
  // takes over), or when the parse is a non-command (`nmap output parsen`).
  const secPresetEntries: ListEntry[] = useMemo(() => {
    if (!parsedCommand || parsedCommand.spec.kind !== "sec") return [];
    const parsed = parseSecCommand(parsedCommand.spec.keyword, parsedCommand.arg, secCat, secDef);
    if (parsed.kind === "built" || parsed.kind === "not-command") return [];
    if (parsed.kind === "tool-overview") {
      return secCat.tools
        .filter((t) => t.name !== "gobuster") // gobuster is the extensibility proof, not a headline tool
        .map(
          (t): ListEntry => ({
            kind: "command-suggestion",
            data: {
              keyword: t.name,
              syntax: t.name,
              description: `${t.presets.filter((p) => p.category !== "prepare").length} presets — ${t.binary}`,
              completion: `sec ${t.name} `,
            },
          }),
        );
    }
    if (parsed.kind === "preset-list") {
      const presets = matchPresets(parsed.query, visiblePresets(parsed.tool, secDef, parsed.prepare));
      return presets.map(
        (p): ListEntry => ({
          kind: "command-suggestion",
          data: {
            keyword: `${parsed.tool.name} ${p.name}`,
            syntax: `${parsed.tool.name} ${p.name}${p.sharp ? " ⚠" : ""}`,
            description: p.purpose,
            completion: `${parsed.tool.name} ${p.name} `,
          },
        }),
      );
    }
    return [];
  }, [parsedCommand, secCat, secDef]);

  // In finder-mode (Ctrl+Shift+F just fired), the file list takes the
  // top of the result list. A complete `rz <W>x<H>` command still
  // shows as the runnable command row above the files so the user
  // sees what's about to fire.
  const finderFileEntries: ListEntry[] = useMemo(() => {
    if (!finderFiles) return [];
    return finderFiles.map((f): ListEntry => ({ kind: "finder-file", data: f }));
  }, [finderFiles]);

  // Bruno (Brutto→Netto). User's persisted defaults override the
  // ship defaults; we fetch them once on mount and refresh after the
  // Settings panel saves. `null` while loading — falls back to the
  // pure-TS defaults so the user can still use `bruno` before the
  // IPC round-trip completes.
  const [brunoDefaults, setBrunoDefaults] = useState<BrunoDefaults | null>(null);
  useEffect(() => {
    void brunoGetDefaults()
      .then(setBrunoDefaults)
      .catch(() => undefined);
    let cancelled = false;
    let unlisten: UnlistenFn | undefined;
    void listen("bruno-defaults-changed", () => {
      void brunoGetDefaults()
        .then(setBrunoDefaults)
        .catch(() => undefined);
    }).then((u) => {
      if (cancelled) u();
      else unlisten = u;
    });
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, []);
  const brunoEntry: ListEntry | null = useMemo(() => {
    const parsed = parseBrunoCommand(query);
    if (!parsed) return null;
    const d = brunoDefaults ?? {
      tax_class: 1,
      state: "nw",
      children: 0,
      is_church_member: false,
      health_add: 2.45,
      kv_type: "gkv",
      pkv_monthly: 600,
      kv_sick_pay: false,
      business_type: "freiberufler",
      hebesatz: 400,
      self_married: false,
    };
    if (parsed.self) {
      // Selbständigen-Rechnung (`f`-Suffix): eigener Rechenkern; die
      // employee-förmigen Kopf-Felder spiegeln die Kernzahlen, damit
      // Zeilen-Rendering + Enter-Paste unverändert funktionieren.
      const sr = computeBrunoSelf({
        yearlyProfit: parsed.yearlyGross,
        state: d.state as GermanState,
        children: d.children,
        isChurchMember: d.is_church_member,
        healthAdd: d.health_add,
        kvType: d.kv_type === "pkv" ? "pkv" : "gkv",
        pkvMonthly: d.pkv_monthly,
        kvSickPay: d.kv_sick_pay,
        businessType: d.business_type === "gewerbe" ? "gewerbe" : "freiberufler",
        hebesatz: d.hebesatz,
        married: d.self_married,
      });
      return {
        kind: "bruno",
        data: {
          yearlyGross: sr.yearlyProfit,
          period: parsed.period,
          netYear: sr.netYear,
          netMonth: sr.netMonth,
          totalDeductions: sr.totalDeductions,
          deductionRate: sr.deductionRate,
          marginalRate: sr.marginalRate,
          social: { health: sr.health, care: sr.care, pension: 0, unemployment: 0 },
          incomeTax: sr.incomeTax,
          soli: sr.soli,
          churchTax: sr.churchTax,
          taxClass: d.tax_class,
          state: d.state,
          children: d.children,
          isChurchMember: d.is_church_member,
          self: {
            ...sr,
            businessType: d.business_type === "gewerbe" ? "gewerbe" : "freiberufler",
            hebesatz: d.hebesatz,
            kvType: d.kv_type === "pkv" ? "pkv" : "gkv",
            kvSickPay: d.kv_sick_pay,
            children: d.children,
            isChurchMember: d.is_church_member,
            married: d.self_married,
            state: d.state,
            expenses: parsed.expenses,
          },
        },
      };
    }
    const result = computeBruno({
      yearlyGross: parsed.yearlyGross,
      taxClass: Math.min(6, Math.max(1, d.tax_class)) as 1 | 2 | 3 | 4 | 5 | 6,
      state: d.state as GermanState,
      children: d.children,
      isChurchMember: d.is_church_member,
      healthAdd: d.health_add,
    });
    return {
      kind: "bruno",
      data: {
        yearlyGross: result.yearlyGross,
        period: parsed.period,
        netYear: result.netYear,
        netMonth: result.netMonth,
        totalDeductions: result.totalDeductions,
        deductionRate: result.deductionRate,
        marginalRate: result.marginalRate,
        social: {
          health: result.social.health,
          care: result.social.care,
          pension: result.social.pension,
          unemployment: result.social.unemployment,
        },
        incomeTax: result.incomeTax,
        soli: result.soli,
        churchTax: result.churchTax,
        taxClass: d.tax_class,
        state: d.state,
        children: d.children,
        isChurchMember: d.is_church_member,
      },
    };
  }, [query, brunoDefaults]);

  // App launcher: pick the single best-matching installed app for the
  // current query. Spotlight-like — one app surfaces at the top of
  // the list if and only if it scores above the threshold. Avoids
  // crowding the popup with marginal matches.
  //
  // Heuristic: exact prefix match (case-insensitive) wins; otherwise
  // longest-prefix wins; ties broken alphabetically. Skipping fuse.js
  // here keeps the per-keystroke cost negligible (<1 ms for ~300
  // apps) and the match feel is more "Spotlight" — a typo doesn't
  // surface a random app, only true prefixes do.
  const appEntry: ListEntry | null = useMemo(() => {
    const q = query.trim().toLowerCase();
    if (q.length < 1 || installedApps.length === 0) return null;
    // Don't fight for the top slot with a complete power command
    // (e.g. `kill safari` should kill, not launch Safari). Reuse the already-
    // memoised parse instead of calling parseCommand again.
    if (parsedCommand) return null;
    // Exact prefix → substring → **fuzzy subsequence** (the app launcher is
    // fuzzy on purpose; clipboard history is not — see `useFuzzySearch`). E.g.
    // `vsc` fuzzy-matches "Visual Studio Code", `term` substring-matches
    // Terminal. Abandon only if even the fuzzy pass misses.
    const exactPrefix = installedApps.find((a) => a.name_lower.startsWith(q));
    let match = exactPrefix ?? installedApps.find((a) => a.name_lower.includes(q));
    const score = exactPrefix
      ? 0
      : match
        ? match.name_lower.split(/\s+/).some((w) => w.startsWith(q))
          ? 0.2
          : 0.5
        : 0.7;
    if (!match) {
      // First-char-anchored ordered subsequence (3+ chars); pick the best score.
      let best: (typeof installedApps)[number] | null = null;
      let bestScore = Infinity;
      for (const a of installedApps) {
        const s = fuzzyScore(a.name_lower, q);
        if (s !== null && s < bestScore) {
          bestScore = s;
          best = a;
        }
      }
      if (best) match = best;
    }
    if (!match) return null;
    return {
      kind: "app",
      data: { name: match.name, path: match.path, score },
    };
  }, [query, installedApps, parsedCommand]);

  // Combine: in kill mode, the process picker takes over the entire
  // list (no point mixing clipboard history with process rows — they
  // can't be activated the same way and would just confuse selection).
  // Otherwise: command/suggestion first, then calc / color, then
  // snippets, then history clips.
  // pwgen entry — surfaces when the query parses as `pwgen <N>`.
  // Re-evaluates on query / mode / seed change; password regenerates
  // each render because `generatePassword` calls CSPRNG.
  const pwgenEntry: ListEntry | null = useMemo(() => {
    const parsed = parsedCommand;
    if (!parsed || parsed.spec.kind !== "pwgen") return null;
    // Bare `pwgen` → default length; an explicit `pwgen 16` overrides it.
    // An invalid arg (`pwgen abc`) yields no action row.
    const len =
      parsed.arg.trim() === "" ? DEFAULT_PWGEN_LENGTH : parsePwgenArg(parsed.arg);
    if (!len) return null;
    const generated = generatePassword(pwgenMode, len);
    // Use the user's edited value iff it belongs to THIS generation
    // (same seed/query/mode); otherwise a fresh random. Stale edits
    // (after a reroll / mode-switch / length-change) are ignored.
    const edited =
      pwgenEdit &&
      pwgenEdit.seed === pwgenSeed &&
      pwgenEdit.query === query &&
      pwgenEdit.mode === pwgenMode
        ? pwgenEdit.value
        : null;
    return {
      kind: "pwgen",
      data: {
        length: len,
        mode: pwgenMode,
        password: edited ?? generated,
      },
    };
    // Seed dep so explicit re-rolls (Enter) regenerate even when
    // query + mode are unchanged; pwgenEdit dep folds in user edits.
  }, [parsedCommand, query, pwgenMode, pwgenSeed, pwgenEdit]);

  // bpm trigger — exact `bpm` (whitespace + case tolerant) surfaces
  // a "Detect BPM" row at the top. Enter activates → bpmMode = true →
  // <BpmDetector /> takes over the popup body.
  const bpmEntry: ListEntry | null = useMemo(() => {
    if (!isBpmTrigger(query)) return null;
    return {
      kind: "bpm",
      data: { label: "Detect BPM (microphone)" },
    };
  }, [query]);

  // equalizer trigger — exact `equalizer`/`eq` surfaces an "Equalizer" row.
  // Enter activates → equalizerMode = true → <EqualizerVisualizer /> takes
  // over the popup body (same shape as bpm).
  const equalizerEntry: ListEntry | null = useMemo(() => {
    if (!isEqualizerTrigger(query)) return null;
    return {
      kind: "equalizer",
      data: { label: "Equalizer (microphone spectrum)" },
    };
  }, [query]);

  // 2FA management trigger — exact `2fa` surfaces a "TOTP management"
  // row; Enter takes over the popup with the full <TotpOverlay />.
  const totpManageEntry: ListEntry | null = useMemo(() => {
    if (!is2faTrigger(query)) return null;
    return {
      kind: "totp-manage",
      data: { label: "2FA · Manage TOTP" },
    };
  }, [query]);

  // `otp <query>` autocomplete: fuzzy-match against issuer/account.
  // We poll codes once a second while at least one TOTP row is in
  // the list, so the displayed codes stay current.
  const otpQuery = parseOtpQuery(query);
  useEffect(() => {
    if (otpQuery === null) return;
    let cancelled = false;
    const refresh = async () => {
      try {
        const [{ totpList, totpCurrentCodesAll }] = await Promise.all([
          import("./lib/ipc"),
        ]);
        const [entries, codes] = await Promise.all([totpList(), totpCurrentCodesAll()]);
        if (cancelled) return;
        setTotpEntries(entries);
        setTotpCodes(new Map(codes.map((c) => [c.id, c])));
      } catch (e) {
        if (!cancelled) console.warn("totp poll failed", e);
      }
    };
    void refresh();
    const interval = setInterval(refresh, 1000);
    return () => {
      cancelled = true;
      clearInterval(interval);
    };
  }, [otpQuery !== null]); // eslint-disable-line react-hooks/exhaustive-deps

  const totpAutocompleteEntries: ListEntry[] = useMemo(() => {
    if (otpQuery === null) return [];
    const matched = matchTotpEntries(otpQuery, totpEntries);
    return matched.slice(0, 8).map((e): ListEntry => {
      const c = totpCodes.get(e.id);
      return {
        kind: "totp",
        data: {
          id: e.id,
          issuer: e.issuer,
          account: e.account,
          digits: e.digits,
          period: e.period,
          code: c?.code ?? "…",
          seconds_remaining: c?.seconds_remaining ?? 0,
        },
      };
    });
  }, [otpQuery, totpEntries, totpCodes]);

  // Pasting a social-media URL (YouTube / IG / TikTok / FB) into the search bar
  // surfaces a download suggestion at the top; the preview offers video/audio.
  const socialEntry = useMemo<ListEntry | null>(() => {
    const t = detectSocial(query);
    return t ? { kind: "social", data: t } : null;
  }, [query]);

  // Memoised so unrelated state changes (pwgen editing, brightness/sound focus,
  // toast state, …) don't rebuild the whole list and hand a fresh array
  // reference to HistoryList / the virtualizer every render. Every input below
  // is itself memoised, so this only recomputes when the list truly changes.
  // `snitch` has two views (app blocker + connections map). Surface the OTHER
  // one as a visible, selectable sub-row so both are discoverable the moment
  // you type `snitch` — no need to know the `map` keyword. commandEntry renders
  // the primary (blocker, or the map when the arg already targets it); this is
  // its counterpart, spliced right after it. Enter runs it; Tab autocompletes.
  const snitchSubEntry: ListEntry | null = useMemo(() => {
    if (!isSnitchCmd) return null;
    const mapMode = /\b(map|conn|show|world)/i.test(parsedCommand?.arg ?? "");
    return mapMode
      ? {
          kind: "command-suggestion",
          data: {
            keyword: "snitch",
            syntax: "snitch",
            description: "Block apps' internet access (best-effort)",
            completion: "snitch",
          },
        }
      : {
          kind: "command-suggestion",
          data: {
            keyword: "snitch map",
            syntax: "snitch map",
            description: "Live connections on a world map",
            completion: "snitch map",
          },
        };
  }, [isSnitchCmd, parsedCommand]);

  // `shazam` has two views (recognize + history). Surface the OTHER as a
  // visible sub-row so `shazam history` is discoverable without knowing the
  // keyword (same pattern as snitchSubEntry). Enter runs it; Tab autocompletes.
  const shazamSubEntry: ListEntry | null = useMemo(() => {
    if (!isShazamCmd) return null;
    const histMode = /\b(hist|list)/i.test(parsedCommand?.arg ?? "");
    return histMode
      ? {
          kind: "command-suggestion",
          data: {
            keyword: "shazam",
            syntax: "shazam",
            description: "Identify the song playing (records from the mic)",
            completion: "shazam",
          },
        }
      : {
          kind: "command-suggestion",
          data: {
            keyword: "shazam history",
            syntax: "shazam history",
            description: "Your recognized-songs history",
            completion: "shazam history",
          },
        };
  }, [isShazamCmd, parsedCommand]);

  const combined: ListEntry[] = useMemo(() => {
    if (isKillMode) return killTargetEntries;
    if (isMemeMode) return memeEntries;
    if (helpTarget?.kind === "index") return helpEntries;
    if (isFigletMode) return figletEntries;
    // Pinned-only browser: just the pinned clips (still query-filtered), and
    // nothing else — no commands, snippets, launcher hits or calc rows.
    if (pinnedOnly) {
      return pinnedClips(filteredClips).map(
        (c): ListEntry => ({ kind: "clip", data: c }),
      );
    }
    return [
      // Custom commands have the HIGHEST priority. A complete command
      // (commandEntry) takes the top slot, and partial command *suggestions*
      // rank right below it — both above the app-launcher hit. Otherwise an
      // app with the same name (typing `term` fuzzy-matches Terminal.app)
      // would outrank the `terminal` command and you'd launch the app
      // instead of opening a terminal in the current Finder folder.
      // For `weather <partial>`, the matching-city suggestions REPLACE the raw
      // command row (whose Enter would just fetch the partial and fail), so the
      // top city is the default selection → Enter/Tab completes it. A city not
      // in the list yields no suggestions → the normal command row runs (and
      // OpenWeather tries the typed name).
      ...(weatherCitySuggestionEntries.length > 0
        ? weatherCitySuggestionEntries
        : commandEntry
          ? [commandEntry]
          : []),
      ...(snitchSubEntry ? [snitchSubEntry] : []),
      ...(shazamSubEntry ? [shazamSubEntry] : []),
      ...suggestionEntries,
      ...(socialEntry ? [socialEntry] : []),
      ...(openerEntry ? [openerEntry] : []),
      // EVERY keyword-triggered custom command outranks the app-launcher hit.
      // These dedicated command rows (bruno/pwgen/bpm/2fa+otp/rz) must therefore
      // come BEFORE `appEntry` — otherwise an app whose name fuzzy-matches the
      // keyword wins (e.g. the "OTP Manager" app beating the `otp` command, or
      // Terminal.app beating `terminal`). Custom commands ALWAYS have priority.
      ...(brunoEntry ? [brunoEntry] : []),
      ...(pwgenEntry ? [pwgenEntry] : []),
      ...(bpmEntry ? [bpmEntry] : []),
      ...(equalizerEntry ? [equalizerEntry] : []),
      ...(totpManageEntry ? [totpManageEntry] : []),
      ...totpAutocompleteEntries,
      ...resizePresetEntries,
      ...fakerCatalogEntries,
      ...secPresetEntries,
      ...(appEntry ? [appEntry] : []),
      ...finderFileEntries,
      ...(calcResult ? [{ kind: "calc", data: calcResult } as ListEntry] : []),
      ...(convertResult ? [{ kind: "calc", data: convertResult } as ListEntry] : []),
      ...(colorResult ? [{ kind: "color", data: colorResult } as ListEntry] : []),
      ...matchingSnippets.map((s): ListEntry => ({ kind: "snippet", data: s })),
      ...filteredClips.map((c): ListEntry => ({ kind: "clip", data: c })),
    ];
  }, [
    isKillMode,
    killTargetEntries,
    isMemeMode,
    memeEntries,
    isFigletMode,
    figletEntries,
    helpTarget,
    helpEntries,
    pinnedOnly,
    commandEntry,
    snitchSubEntry,
    shazamSubEntry,
    suggestionEntries,
    socialEntry,
    openerEntry,
    appEntry,
    brunoEntry,
    pwgenEntry,
    bpmEntry,
    equalizerEntry,
    totpManageEntry,
    totpAutocompleteEntries,
    resizePresetEntries,
    weatherCitySuggestionEntries,
    fakerCatalogEntries,
    secPresetEntries,
    finderFileEntries,
    calcResult,
    convertResult,
    colorResult,
    matchingSnippets,
    filteredClips,
  ]);

  // Social download: Tab toggles video↔audio for a selected YouTube `social`
  // row; the global Enter bumps `socialRunSignal` so the preview bar runs the
  // chosen mode (and shows its progress animation). `socialMode` drives the
  // highlighted button in the preview.
  const [socialMode, setSocialMode] = useState<"video" | "audio">("video");
  const [socialRunSignal, setSocialRunSignal] = useState(0);
  const selectedSocial =
    combined[selected]?.kind === "social" ? combined[selected] : null;
  const selectedSocialUrl =
    selectedSocial?.kind === "social" ? selectedSocial.data.url : null;
  const selectedSocialIsYt =
    selectedSocial?.kind === "social" && selectedSocial.data.platform === "youtube";
  // Reset to video whenever the selected social URL changes (new suggestion).
  useEffect(() => {
    setSocialMode("video");
  }, [selectedSocialUrl]);
  // Tab (capture phase, before useKeyboardNav's bubble handler) flips the mode.
  useEffect(() => {
    if (!selectedSocialIsYt || gameMode) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== "Tab") return;
      if (e.metaKey || e.ctrlKey || e.altKey) return;
      e.preventDefault();
      e.stopPropagation();
      setSocialMode((m) => (m === "video" ? "audio" : "video"));
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [selectedSocialIsYt, gameMode]);

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
      setOpenerDir(delta);
      setOpenerIndex((cur) => (cur === null ? 0 : (((cur + delta) % n) + n) % n));
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [selectedIsOpener]);

  // Cmd+1…4 (macOS) / Ctrl+1…4 (Win/Linux) while a `pwgen` entry is
  // selected → switch mode + regenerate (no copy, no popup-hide). The
  // user keeps tapping until they see a password they like, then
  // presses Enter to copy + hide. Reuses the `pwgenMode` + `pwgenSeed`
  // state — the existing useMemo at line ~562 re-derives the password
  // from `(query, pwgenMode, pwgenSeed)`, so bumping the seed *and*
  // the mode triggers a fresh generation.
  //   ⌘/Ctrl+1 → all chars
  //   ⌘/Ctrl+2 → alphanumeric (no symbols)
  //   ⌘/Ctrl+3 → dictionary (English words + digit padding)
  //   ⌘/Ctrl+4 → leetspeak
  // Uses `e.code` (Digit1…4) instead of `e.key` so this still works on
  // German Mac keyboards where Alt+1 would otherwise type `¡`.
  //
  // **0.43.4 change:** moved from Alt+1…4 to Cmd/Ctrl+1…4 specifically
  // to dodge the collision with the default text-expander hotkey
  // (`Alt+Digit1`). Alt+1 was forwarded through a Tauri event since the
  // Carbon dispatcher swallowed the keydown; now there's no collision
  // and the keydown reaches the webview directly for all four digits.
  //
  // No collision with `TransformBar`'s Cmd/Ctrl+1…9 (text transforms)
  // because that listener is only mounted when the selected entry is a
  // *text* row — pwgen rows have no text content, so the transform
  // listener is unmounted and Cmd/Ctrl+1…4 is free.
  const selectedPwgen =
    combined[selected]?.kind === "pwgen"
      ? (combined[selected] as Extract<(typeof combined)[number], { kind: "pwgen" }>)
      : null;
  useEffect(() => {
    if (!selectedPwgen) return;
    const codeToMode: Record<string, PwgenMode> = {
      Digit1: "all",
      Digit2: "alnum",
      Digit3: "dict",
      Digit4: "leet",
    };
    const onKey = (e: KeyboardEvent) => {
      if (e.repeat) return;
      // CmdOrCtrl semantic: Cmd alone on macOS, Ctrl alone elsewhere.
      // Reject the other modifier so we don't fire on accidental
      // Cmd+Ctrl combos, and reject Alt+Shift to keep digit-typing
      // shortcuts free.
      const modOk = IS_MAC ? e.metaKey && !e.ctrlKey : e.ctrlKey && !e.metaKey;
      if (!modOk || e.altKey || e.shiftKey) return;
      const newMode = codeToMode[e.code];
      if (!newMode) return;
      e.preventDefault();
      e.stopPropagation();
      setPwgenMode(newMode);
      setPwgenSeed((s) => s + 1);
      console.info(
        `pwgen → ${IS_MAC ? "Cmd" : "Ctrl"}+${e.code.slice(-1)} switched to ${newMode} + regenerated`,
      );
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [selectedPwgen]);

  // Belt-and-suspenders: if a user has a global expander / direct-slot
  // hotkey on a Cmd/Ctrl+Digit1…4 combo (unusual but possible) the
  // Carbon dispatcher would still swallow the keydown before our JS
  // handler sees it. The Rust handler forwards the hotkey string via
  // `expander-hotkey-forwarded` whenever it fires while we own focus —
  // route Cmd+Digit1…4 / Ctrl+Digit1…4 to the same mode-switch path.
  // The handler reads `selectedPwgen` through a ref: the listener is
  // subscribed once (empty deps), so a direct capture would be a stale
  // closure permanently stuck on the initial `null` (dead code); a deps
  // array would re-subscribe on every keystroke instead.
  const selectedPwgenRef = useRef(selectedPwgen);
  useEffect(() => {
    selectedPwgenRef.current = selectedPwgen;
  }, [selectedPwgen]);
  useTauriEvent<string>("expander-hotkey-forwarded", (e) => {
    if (!selectedPwgenRef.current) return;
    const hotkey = e.payload;
    const m = hotkey.match(/^(Cmd|Ctrl)\+Digit([1-4])$/i);
    if (!m) return;
    const digit = Number(m[2]) as 1 | 2 | 3 | 4;
    const modes: PwgenMode[] = ["all", "alnum", "dict", "leet"];
    const newMode = modes[digit - 1];
    setPwgenMode(newMode);
    setPwgenSeed((s) => s + 1);
    console.info(`pwgen ← forwarded ${hotkey} → ${newMode} + regenerated`);
  });

  // Tab / → autocomplete on a focused `command-suggestion` row. Fills
  // `query` with the suggestion's `completion` (e.g. `rz 1920x1080`)
  // and parks the caret at the end so the user can keep editing
  // *before* hitting Enter to run. → only intercepts when the caret
  // is already at the end of the input — otherwise → still moves
  // the caret within the typed text as normal.
  const selectedSuggestion =
    combined[selected]?.kind === "command-suggestion" ? combined[selected] : null;
  useEffect(() => {
    if (!selectedSuggestion || gameMode) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== "Tab" && e.key !== "ArrowRight") return;
      if (e.shiftKey || e.metaKey || e.ctrlKey || e.altKey) return;
      const input = searchRef.current;
      if (!input) return;
      if (e.key === "ArrowRight") {
        const atEnd =
          input.selectionStart === input.value.length &&
          input.selectionEnd === input.value.length;
        if (!atEnd) return;
      }
      if (selectedSuggestion.kind !== "command-suggestion") return;
      const completion = selectedSuggestion.data.completion;
      if (completion === input.value) return; // nothing to fill
      e.preventDefault();
      e.stopPropagation();
      setQuery(completion);
      requestAnimationFrame(() => {
        searchRef.current?.focus();
        const len = completion.length;
        searchRef.current?.setSelectionRange(len, len);
      });
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [selectedSuggestion, gameMode]);

  // Tab / → on a selected figlet-font row fills `@font` into the search bar
  // (rz-preset semantics) so the user can keep tweaking the text before Enter.
  const selectedFigletFont =
    isFigletMode && combined[selected]?.kind === "figlet-font" ? combined[selected] : null;
  useEffect(() => {
    if (!selectedFigletFont || gameMode) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== "Tab" && e.key !== "ArrowRight") return;
      if (e.shiftKey || e.metaKey || e.ctrlKey || e.altKey) return;
      const input = searchRef.current;
      if (!input) return;
      if (e.key === "ArrowRight") {
        const atEnd =
          input.selectionStart === input.value.length &&
          input.selectionEnd === input.value.length;
        if (!atEnd) return;
      }
      if (selectedFigletFont.kind !== "figlet-font") return;
      const keyword = parsedCommand?.spec.keyword ?? "figlet";
      const completion = [keyword, figletParsed.text, `@${selectedFigletFont.data.name}`]
        .filter(Boolean)
        .join(" ");
      if (completion === input.value) return;
      e.preventDefault();
      e.stopPropagation();
      setQuery(completion);
      requestAnimationFrame(() => {
        searchRef.current?.focus();
        searchRef.current?.setSelectionRange(completion.length, completion.length);
      });
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [selectedFigletFont, gameMode, parsedCommand, figletParsed.text]);

  // Find matching snippets whenever query changes. The cancelled guard keeps
  // an out-of-order older response from clobbering a newer query's result.
  useEffect(() => {
    if (!query.trim()) {
      setMatchingSnippets([]);
      return;
    }
    let cancelled = false;
    findSnippets(query)
      .then((s) => {
        if (!cancelled) setMatchingSnippets(s);
      })
      .catch(() => {
        if (!cancelled) setMatchingSnippets([]);
      });
    return () => {
      cancelled = true;
    };
  }, [query, snippetRev]);

  useEffect(() => {
    setSelected(0);
  }, [query, entries.length]);

  // Selecting a different row (or typing) closes the inline editor. Without
  // this, clicking another row while editing would hide the editor but leave
  // `snippetEditingId` set — and the list navigation gate would stay closed.
  useEffect(() => {
    setSnippetEditingId(null);
  }, [selected, query]);

  // Cmd/Ctrl+E on a selected snippet row → edit it inline in the preview.
  useEffect(() => {
    if (activeTab !== "history" || snippetEditingId !== null) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== "e" || !(e.metaKey || e.ctrlKey)) return;
      const target = combined[selected];
      if (target?.kind !== "snippet") return;
      e.preventDefault();
      e.stopPropagation();
      setSnippetEditingId(target.data.id);
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [activeTab, snippetEditingId, combined, selected]);

  // Cmd/Ctrl+R while a `faker` command row is selected → reroll (new seed).
  // Preventing default also stops the webview from reloading.
  useEffect(() => {
    if (activeTab !== "history") return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== "r" || !(e.metaKey || e.ctrlKey) || e.shiftKey || e.altKey) return;
      const target = combined[selected];
      if (target?.kind !== "command" || target.data.commandKind !== "faker") return;
      e.preventDefault();
      e.stopPropagation();
      setFakerReroll((n) => n + 1);
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [activeTab, combined, selected]);

  // Cmd/Ctrl+Enter on a `sec` command row → terminal hand-off (opt-in). Sharp
  // presets confirm first (always, regardless of auto-enter). Inspector Rust
  // opens the terminal with the command inserted — it runs no tool itself.
  useEffect(() => {
    if (activeTab !== "history") return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== "Enter" || !(e.metaKey || e.ctrlKey)) return;
      const target = combined[selected];
      if (target?.kind !== "command" || target.data.commandKind !== "sec") return;
      e.preventDefault();
      e.stopPropagation();
      const parsed = parseSecCommand(
        target.data.rawInput.split(/\s+/)[0] || "sec",
        target.data.arg,
        secCat,
        secDef,
      );
      if (parsed.kind !== "built") return;
      void (async () => {
        if (parsed.sharp) {
          const ok = await confirmDialog(
            `Run this ${parsed.tool.name} command in your terminal?\n\n${parsed.command}\n\nThis is a sharp preset (${parsed.tags.join(", ")}). Only against authorized targets.`,
            "Run in terminal?",
          );
          if (!ok) return;
        }
        try {
          await secOpenInTerminal(parsed.command, secDef.auto_enter);
          await hidePopup();
        } catch (err) {
          // macOS-only; elsewhere the command is already copyable via Enter.
          setPasteError("other");
          console.error("sec hand-off failed", err);
        }
      })();
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [activeTab, combined, selected, secCat, secDef, hidePopup]);

  // Enter-while-unfocused (close_on_blur OFF): the native `esc_watch` tap emits
  // this because the webview can't see the key while another app holds focus.
  // Activate the currently-selected row (paste the clip/snippet), exactly as a
  // focused Enter would — the paste path hides the popup, so the overlay closes.
  // Reads the LATEST activate(selected) via a ref so the `[]`-dep listener never
  // goes stale as the selection changes.
  const activateSelectedRef = useRef<() => void>(() => {});
  useTauriEvent("popup-activate-selected", () => activateSelectedRef.current(), []);

  // Handle window-shown (hotkey): reset to history tab. v0.38.2+:
  // these use `useTauriEvent` instead of the inline let-then-async
  // pattern — the hook guards against the unmount-before-listen-
  // resolves race that would otherwise leak listeners across the
  // app's lifetime.
  useTauriEvent("window-shown", () => {
    showGenRef.current += 1; // invalidate any in-flight hide (see hidePopup)
    setActiveTab("history");
    setQuery("");
    setSelected(0);
    // Staggered list entrance (v0.88.0): cascade the visible rows in on each
    // popup OPEN (never per keystroke). The class is removed after the
    // animation so later virtualizer updates render instantly.
    setListEntrance(true);
    window.setTimeout(() => setListEntrance(false), 650);
    // Reconcile the footer keep-awake LED to the true state on every open.
    // `wakelock on` / `caffeine on` hide the popup before the footer can
    // observe the `wakelock-changed` event, so re-fetch here — guarantees
    // the status shows next time the popup opens, regardless of the keyword
    // used (or an external change). Same robustness as the timer count.
    void wakelockGet()
      .then((v) => setWakelockActive(v))
      .catch(() => undefined);
    requestAnimationFrame(() => {
      searchRef.current?.focus();
      searchRef.current?.select();
      // CRT TV power-ON — the popup's signature open: a bright dot blooms out
      // of the tube, snaps into a scanline, and the shell stretches out of it.
      // Re-played on every open; no-op under prefers-reduced-motion.
      playCrtOn(shellRef.current);
    });
  });

  // Handle tray "Manage Snippets": switch to snippets tab.
  useTauriEvent(
    "open-snippets-tab",
    () => {
      setActiveTab("snippets");
      void refreshSnippets();
    },
    [refreshSnippets],
  );

  // Cloud sync applied remote snippet changes in the background: reload the
  // list and bump the search-preview revision so open previews re-read.
  useTauriEvent(
    "snippets-synced",
    () => {
      void refreshSnippets();
      setSnippetRev((r) => r + 1);
    },
    [refreshSnippets],
  );

  // Handle tray "Manage Notes": switch to notes tab.
  useTauriEvent(
    "open-notes-tab",
    () => {
      setActiveTab("notes");
      void refreshNotes();
    },
    [refreshNotes],
  );

  // Ctrl+Shift+T global hotkey: open the Timesheet (time-tracking) tab.
  useTauriEvent("open-timesheet-tab", () => {
    setActiveTab("timesheet");
  });

  // Tray → "Settings" quick action.
  useTauriEvent("open-settings-tab", () => {
    setActiveTab("settings");
  });

  // A Shazam recognition finished. It runs in the backend, so it keeps going
  // even after the overlay is closed — when that happens (panel closed), a
  // match surfaces as a status toast so you see it kept listening. If the
  // panel is open it shows the result itself (no toast).
  useTauriEvent<ShazamDone>("shazam-done", (e) => {
    if (shazamModeRef.current !== null) return; // panel open → it handles it
    const m = e.payload?.matched;
    if (!m) return; // background no-match / error: stay silent
    void showStatusToast("shazam", true, `🎵 ${m.title}`, m.artist);
  });

  // Clean scan/execute finished. Jobs run in the backend and survive Esc —
  // when the panel is closed, surface a status toast (scan ready / cleaned /
  // failed). Open panel handles the result itself (no toast duplication).
  useTauriEvent<CleanDone>("clean-done", (e) => {
    if (cleanModeRef.current) return;
    const d = e.payload;
    if (!d) return;
    if (d.kind === "execute" && d.result) {
      const skipped = d.result.errors.length
        ? ` · ${d.result.errors.length} skipped`
        : "";
      void showStatusToast(
        "clean",
        true,
        "Cleaned",
        `${formatBytes(d.result.freed_bytes)} freed${skipped}`,
      );
    } else if (d.kind === "scan" && d.view) {
      void showStatusToast(
        "clean",
        true,
        "Scan ready",
        `${formatBytes(d.view.total_bytes)} · type clean to review`,
      );
    } else if (d.error && d.kind === "execute") {
      void showStatusToast("clean", false, "Clean failed", d.error);
    }
  });

  // Backend fires this when the OCR shortcut is pressed but the
  // Screen Recording TCC grant is missing. Switch to Settings (which
  // shows the Permissions overview) and surface a banner so the
  // failure isn't silent.
  useTauriEvent("ocr-permission-needed", () => {
    setOcrPermissionMissing(true);
    setActiveTab("settings");
  });

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
  useTauriEvent("expander-permission-needed", () => {
    setExpanderPermissionMissing(true);
    setActiveTab("settings");
  });

  useEffect(() => {
    if (!expanderPermissionMissing) return;
    const id = window.setTimeout(() => setExpanderPermissionMissing(false), 15000);
    return () => window.clearTimeout(id);
  }, [expanderPermissionMissing]);

  // Expander safety-block notifications (v0.34.0+). Backend emits
  // `expander-blocked` with one of:
  //   - "password" : focused field is a password / secure text field;
  //                  we refuse to leak the snippet body into it.
  //   - "secure_input": macOS IsSecureEventInputEnabled is true
  //                     (system-wide secure-input flag — typically a
  //                     sudo prompt). CGEventPost is dropped anyway,
  //                     so we bail loudly instead of failing silently.
  // Surfaced as a 4-second floating toast at the bottom of the popup
  // so the user *knows* the expansion was blocked rather than
  // wondering why nothing happened.
  const [expanderBlocked, setExpanderBlocked] = useState<
    null | "password" | "secure_input" | "terminal"
  >(null);
  useTauriEvent<string>("expander-blocked", (e) => {
    const reason = e.payload;
    if (reason === "password" || reason === "secure_input" || reason === "terminal") {
      setExpanderBlocked(reason);
    }
  });
  useEffect(() => {
    if (!expanderBlocked) return;
    // Terminal banner gets longer dwell time — the popup just opened,
    // user needs to read the hint + decide to search vs. configure
    // a direct-slot. Password / secure-input are dismiss-and-forget.
    const dwell = expanderBlocked === "terminal" ? 8000 : 4000;
    const id = window.setTimeout(() => setExpanderBlocked(null), dwell);
    return () => window.clearTimeout(id);
  }, [expanderBlocked]);

  // Finder selection hotkey (Ctrl+Shift+F) — backend reads the
  // selection, opens the popup, then fires this event with the items.
  // We switch into "finder-mode": the list is replaced by the files,
  // and `rz <W>x<H>` runs against them via the resize_file IPC.
  // Hidden trigger words (`getshaky`, `rockthebox`, …) and the OCR /
  // expander permission banners share the same overlay-event pattern.
  useEffect(() => {
    // v0.38.2: cancelled-flag pattern — sequential `await`s mean two
    // potential leak points (if component unmounts between them), and
    // even the first listener can leak if we unmount before its
    // promise resolves.
    let cancelled = false;
    let unlistenLoaded: UnlistenFn | undefined;
    let unlistenDenied: UnlistenFn | undefined;
    (async () => {
      const u1 = await listen<FinderFileView[]>("finder-selection-loaded", (e) => {
        setFinderFiles(e.payload ?? []);
        setFinderAutomationDenied(false);
        setActiveTab("history");
        setQuery("");
        setSelected(0);
        requestAnimationFrame(() => searchRef.current?.focus());
      });
      if (cancelled) {
        u1();
        return;
      }
      unlistenLoaded = u1;
      const u2 = await listen("finder-automation-needed", () => {
        setFinderAutomationDenied(true);
        setFinderFiles([]);
        setActiveTab("history");
      });
      if (cancelled) {
        u2();
        return;
      }
      unlistenDenied = u2;
    })();
    return () => {
      cancelled = true;
      unlistenLoaded?.();
      unlistenDenied?.();
    };
  }, []);

  // Reset transient UI on popup-hidden — i.e. *while the window is hidden*,
  // not on the next show. The popup window is hidden, not destroyed, so its
  // React state survives between sessions; resetting on show meant the stale
  // query / tab / selection painted for one frame before the reset applied
  // (a visible flash). Doing it here means the window is already clean when
  // it next becomes visible. (The window-shown handler still re-focuses the
  // search bar + reconciles the wakelock LED.)
  useTauriEvent("popup-hidden", () => {
    setFinderFiles(null);
    setFinderAutomationDenied(false);
    setActiveTab("history");
    setQuery("");
    setSelected(0);
    setPinnedOnly(false);
    setPwgenEditing(false);
    setSnippetEditingId(null);
    // The TOTP overlay is transient too — its Enter-copies-top-match hides the
    // popup, and the next open must start on the normal history view.
    setTotpMode(false);
    // The calibration panel is transient like the other view modes. The
    // monitoring itself is NOT stopped here — running while the popup is
    // closed is the entire point of the command.
    setIrisMode(false);
    // Prime the shell to the CRT power-off END state (a collapsed bright dot)
    // WHILE hidden — so the OS `show()` (which reveals the window before the
    // window-shown handler runs) shows the dark tube + dot, never a flash of
    // full-opacity content, before `playCrtOn` blooms it. `playCrtOn` clears it.
    primeCrtHidden(shellRef.current);
  });

  // Wakelock LED state. v0.37.1+: register the `wakelock-changed`
  // event listener BEFORE firing the initial `wakelock_get` IPC, and
  // skip the initial value if an event already arrived. Pre-0.37.1
  // had a ~10 ms race: if the user toggled wakelock while
  // `wakelockGet()` was in flight, the event fired (setState→new),
  // then wakelockGet returned and reverted with the stale value. No
  // polling — wakelock toggles are user-driven, never spontaneous.
  useEffect(() => {
    // v0.38.2: cancelled flag + plus the v0.37.1 eventAlreadyFired
    // flag — two separate concerns that look similar:
    //   cancelled        = "component unmounted, don't touch state"
    //   eventAlreadyFired = "an event-driven update landed, the
    //                        initial-fetch's stale value would
    //                        clobber it"
    let cancelled = false;
    let unlisten: UnlistenFn | undefined;
    let eventAlreadyFired = false;
    void listen<boolean>("wakelock-changed", (e) => {
      if (cancelled) return;
      eventAlreadyFired = true;
      setWakelockActive(Boolean(e.payload));
    }).then((u) => {
      if (cancelled) u();
      else unlisten = u;
    });
    void wakelockGet()
      .then((v) => {
        if (!cancelled && !eventAlreadyFired) setWakelockActive(v);
      })
      .catch(() => undefined);
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, []);

  // Timer state: count active timers (footer badge) + show a banner
  // when one fires. Both via simple useTauriEvent — no special
  // ordering needed since the count is recomputed via list_timers
  // on every change.
  useEffect(() => {
    void listTimers()
      .then((ts) => setActiveTimerCount(ts.length))
      .catch(() => undefined);
  }, []);
  useTauriEvent("timers-changed", () => {
    void listTimers()
      .then((ts) => setActiveTimerCount(ts.length))
      .catch(() => undefined);
  });

  // Timesheet tracking footer indicator — fetch on mount + on every
  // tracker status change (start/stop/idle-pause emit `track-status-changed`).
  useEffect(() => {
    void trackStatus()
      .then((s) =>
        setTrackStatusState({ active: s.active, paused: s.paused || s.manual_paused }),
      )
      .catch(() => undefined);
  }, []);
  useTauriEvent("track-status-changed", () => {
    void trackStatus()
      .then((s) =>
        setTrackStatusState({ active: s.active, paused: s.paused || s.manual_paused }),
      )
      .catch(() => undefined);
  });
  useTauriEvent<{ id: number; label: string }>("timer-fired", (e) => {
    setTimerFiredLabel(e.payload?.label ?? "Timer done");
  });
  useEffect(() => {
    if (!timerFiredLabel) return;
    const id = window.setTimeout(() => setTimerFiredLabel(null), 4000);
    return () => window.clearTimeout(id);
  }, [timerFiredLabel]);

  // Preview crossfade restart — keyed on the selected entry identity.
  const previewCrossfadeKey =
    combined[selected] !== undefined ? `${combined[selected].kind}-${selected}` : "";
  useEffect(() => {
    const el = previewFadeRef.current;
    if (!el) return;
    el.classList.remove("md3-preview-in");
    void el.offsetWidth; // reflow → the re-add restarts the animation
    el.classList.add("md3-preview-in");
  }, [previewCrossfadeKey]);

  const activate = async (i: number, shiftKey = false, altKey = false, metaKey = false) => {
    const target = combined[i];
    if (!target) return;

    // Run a parsed power-command by kind. Returns `true` if it handled the
    // kind — so a fuzzy `command-suggestion` Enter can RUN a complete
    // command in one keystroke (and otherwise fall back to autocompleting
    // the input). Shared by the `command` row and `command-suggestion` row.
    const dispatchCommand = async (
      commandKind: CommandKind,
      arg: string,
      keyword?: string,
    ): Promise<boolean> => {
      // INVARIANT — inline preview-panel commands must canonicalise the query
      // to their keyword the moment they open. Every one of these renders in
      // the preview column and stays open only while the query still parses to
      // that command (a `!isXCmd` auto-exit effect). Entering the mode from a
      // *partial* fuzzy suggestion (Enter on the `snitch`/`clean`/… hint while
      // the field still reads `sni`/`clea`) would otherwise trip that effect
      // instantly and the panel would flash open-then-closed. Canonicalising
      // here — ONCE, for the whole family — guarantees consistent behaviour so
      // a new panel command can't forget it (the bug that hid `snitch`/`clean`
      // behind a partial suggestion). Keep any typed argument for the commands
      // whose arg selects a sub-view (`calendar <date>`, `snitch map`).
      const PANEL_KINDS: CommandKind[] = [
        "brightness", "sound", "hue", "stats", "boom", "uptime", "weather", "tokens", "calendar", "clean", "snitch", "shazam", "iris",
      ];
      if (PANEL_KINDS.includes(commandKind)) {
        const keepArg =
          commandKind === "calendar" ||
          commandKind === "snitch" ||
          commandKind === "shazam" ||
          commandKind === "weather" ||
          commandKind === "iris";
        setQuery(keepArg && arg ? `${commandKind} ${arg}` : commandKind);
      }
      if (isTranslateKind(commandKind)) {
        // Enter → copy the live translation; ⇧Enter → open Google Translate.
        if (shiftKey) {
          await openUrl(translateUrl(commandKind, arg));
          await hidePopup();
          return true;
        }
        const pair = TRANSLATE_LANGS[commandKind];
        const src = arg.trim();
        if (!pair || !src) return true;
        const cacheKey = `${pair.sl}|${pair.tl}|${src}`;
        let out: string | null =
          liveTranslation?.status === "ok" ? liveTranslation.text : null;
        if (!out) {
          out = translateCacheRef.current.get(cacheKey)?.text ?? null;
        }
        if (!out) {
          try {
            const r = await translateText(src, pair.sl, pair.tl);
            out = r.text;
            translateCacheRef.current.set(cacheKey, {
              text: r.text,
              provider: r.provider,
              detectedSource: r.detected_source ?? "",
            });
          } catch (e) {
            setPasteError("other");
            console.error("translate copy failed", e);
            return true;
          }
        }
        try {
          const { writeText } = await import("@tauri-apps/plugin-clipboard-manager");
          await writeText(out);
          await hidePopup();
        } catch (e) {
          setPasteError("other");
          console.error("translate clipboard write failed", e);
        }
      } else if (commandKind === "websearch") {
        const bang = SEARCH_BANGS[keyword ?? ""];
        if (!bang) {
          setPasteError("other");
          return true;
        }
        await openUrl(bang.url(arg));
        await hidePopup();
      } else if (
        commandKind === "uuid" ||
        commandKind === "slug" ||
        commandKind === "hash" ||
        commandKind === "json" ||
        commandKind === "jwt"
      ) {
        // Dev quick-tools: compute a result and put it on the clipboard. uuid /
        // slug / hash transform the argument; json / jwt transform whatever is
        // currently on the clipboard. The result lands in history too.
        try {
          const { writeText, readText } =
            await import("@tauri-apps/plugin-clipboard-manager");
          let out: string;
          if (commandKind === "uuid") {
            out = generateUuids(parseInt(arg, 10) || 1);
          } else if (commandKind === "slug") {
            out = slugify(arg);
          } else if (commandKind === "hash") {
            out = await sha256Hex(arg);
          } else if (commandKind === "json") {
            out = formatJson((await readText()) ?? "");
          } else {
            out = decodeJwt((await readText()) ?? "");
          }
          await writeText(out);
          await hidePopup();
        } catch (e) {
          setPasteError("other");
          console.error(`${commandKind} failed`, e);
          return true;
        }
      } else if (commandKind === "faker") {
        // Parse → generate ALL values (full n) → format → paste into the
        // focused app. Only a complete spec runs; catalogue/partial/unknown
        // yields false so the caller fills the input instead.
        const parsed = parseFakerCommand(arg, fakerCat, fakerDef);
        if (parsed.kind !== "spec") return false;
        try {
          const result = await fakerGenerate(parsed.spec);
          const text = formatValues(result, parsed.spec.format);
          await pasteGenerated(text, fakerDef.save_history);
        } catch (e) {
          setPasteError("other");
          console.error("faker failed", e);
          return true;
        }
      } else if (commandKind === "sec") {
        // Enter = copy the built command + paste into the focused app (the
        // overriding-safe default — never runs anything). Only a fully-built
        // command runs; overview/preset-list/prose yields false → fill input.
        const parsed = parseSecCommand(keyword ?? "sec", arg, secCat, secDef);
        if (parsed.kind !== "built") return false;
        try {
          await pasteGenerated(parsed.command, secDef.save_history);
        } catch (e) {
          setPasteError("other");
          console.error("sec copy failed", e);
          return true;
        }
      } else if (commandKind === "qr") {
        // Render the QR to a PNG and copy it to the clipboard (+ history).
        try {
          const png = qrPngBase64(arg);
          const label = arg.length > 48 ? `${arg.slice(0, 48)}…` : arg;
          await qrCopyPng(png, label);
          await hidePopup();
        } catch (e) {
          setPasteError("other");
          console.error("qr failed", e);
          return true;
        }
      } else if (commandKind === "resize") {
        const dims = parseResizeArg(arg);
        if (!dims) {
          setPasteError("other");
          return true;
        }
        // Resize the image(s) currently selected in Finder/Explorer — writes
        // <name>-WxH.<ext> next to each source. Reads the LIVE selection (you
        // don't need to be in finder-mode). Falls back to the clipboard image
        // when nothing usable is selected / the selection can't be read.
        let didSelection = false;
        try {
          const sel = finderFiles ?? (await getFinderSelection());
          const imgs = sel.filter((f) => f.is_image);
          if (imgs.length > 0) {
            const results = await Promise.all(
              imgs.map((f) =>
                resizeFile(f.path, dims.width, dims.height).catch((e) => {
                  console.error("resize_file failed", f.path, e);
                  return "";
                }),
              ),
            );
            didSelection = results.some((r) => r);
          }
        } catch (e) {
          console.debug("resize: finder selection unavailable", e);
        }
        if (!didSelection) {
          await resizeClipboardImage(dims.width, dims.height);
        }
        await hidePopup();
      } else if (commandKind === "optim") {
        // Compress the image(s) currently selected in Finder/Explorer — writes
        // <stem>-optim.<ext> next to each source (PNG lossless via oxipng; JPEG
        // re-encoded, kept only if smaller). Reads the LIVE selection (you don't
        // need to be in finder-mode). Falls back to the clipboard PNG when
        // nothing usable is selected / the selection can't be read.
        const optimizable = (path: string) => /\.(png|jpe?g)$/i.test(path);
        let didSelection = false;
        try {
          const sel = finderFiles ?? (await getFinderSelection());
          const imgs = sel.filter((f) => f.is_image && optimizable(f.path));
          if (imgs.length > 0) {
            const results = await Promise.all(
              imgs.map((f) =>
                optimizeFile(f.path).catch((e) => {
                  console.error("optimize_file failed", f.path, e);
                  return null;
                }),
              ),
            );
            didSelection = results.some(Boolean);
          }
        } catch (e) {
          // No selection / Automation not granted → fall through to clipboard.
          console.debug("optim: finder selection unavailable", e);
        }
        if (!didSelection) {
          await optimizeClipboardImage();
        }
        await hidePopup();
      } else if (commandKind === "rmvvls") {
        await removeVowelsToClipboard(arg);
        await hidePopup();
      } else if (commandKind === "reboot") {
        // Destructive: real native confirmation (window.confirm is unreliable
        // in the webview — it can auto-pass without showing anything).
        if (
          !(await confirmDialog(
            "Restart the system now?\n\nAll unsaved app data may be lost. macOS will show its own confirmation for apps with unsaved changes.",
            "Restart?",
          ))
        ) {
          return true;
        }
        await systemReboot();
        await hidePopup();
      } else if (commandKind === "shutdown") {
        if (
          !(await confirmDialog(
            "Power off the system now?\n\nAll unsaved app data may be lost. macOS will show its own confirmation for apps with unsaved changes.",
            "Power off?",
          ))
        ) {
          return true;
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
      } else if (commandKind === "freeze") {
        // Input lock — backend hides the popup itself, then blocks all
        // keyboard / mouse input until the unlock chord is pressed.
        try {
          await startInputLock();
        } catch (e) {
          setPasteError("other");
          console.error("input lock failed", e);
        }
      } else if (commandKind === "wakelock") {
        const on = parseWakelockArg(arg);
        if (on === null) {
          setPasteError("other");
          return true;
        }
        // The backend hides the popup itself, then plays the on-screen
        // status flourish (status_toast). Calling hidePopup() here would
        // fire app.hide() on macOS and swallow that toast, so don't.
        // `source` only brands the toast (Caffeine vs Wakelock).
        const source = keyword === "caffeine" ? "caffeine" : "wakelock";
        await wakelockSet(on, source);
      } else if (commandKind === "touch" || commandKind === "mkdir") {
        // Create a file / folder in the active file-manager window's folder
        // (Finder on macOS, Explorer on Windows). For `touch`, an optional
        // `> <text>` writes that content into the new file:
        //   touch hallo.txt > das ist ein test
        try {
          let path: string;
          if (commandKind === "touch") {
            const gt = arg.indexOf(">");
            const name = (gt >= 0 ? arg.slice(0, gt) : arg).trim();
            const content = gt >= 0 ? arg.slice(gt + 1).trim() : "";
            path = await finderTouch(name, content);
          } else {
            path = await finderMkdir(arg);
          }
          console.info("created", path);
        } catch (e) {
          setPasteError("other");
          console.error(`${commandKind} failed`, e);
          return true;
        }
        await hidePopup();
      } else if (commandKind === "terminal") {
        // Open the terminal at the frontmost Finder window's folder.
        try {
          const dir = await finderOpenTerminal();
          console.info("opened terminal at", dir);
        } catch (e) {
          setPasteError("other");
          console.error("terminal failed", e);
          return true;
        }
        await hidePopup();
      } else if (commandKind === "trim") {
        // Open the trim overlay (it has the file picker + timeline). Hide the
        // popup so the overlay window takes focus.
        try {
          await trimOpenOverlay();
        } catch (e) {
          setPasteError("other");
          console.error("trim overlay failed", e);
          return true;
        }
        await hidePopup();
      } else if (commandKind === "track") {
        // Time tracking: `track on` starts a session, `track off` ends it.
        // (Bare `track` opens the timesheet — the tab lands in a later step;
        // for now it shows the current status.)
        const m = parseTrackArg(arg);
        try {
          if (m === "on") {
            await trackStart();
            await showStatusToast("track", true, "Tracking on", "Recording app usage");
          } else if (m === "off") {
            await trackStop();
            await showStatusToast("track", false, "Tracking off", "Session ended");
          } else if (m === "open") {
            // Bare `track` → open the Timesheet tab on today.
            setQuery("");
            setActiveTab("timesheet");
          } else {
            setPasteError("other");
          }
        } catch (e) {
          setPasteError("other");
          console.error("track command failed", e);
        }
        return true;
      } else if (commandKind === "settings") {
        // Open the Settings tab; an argument deep-links to a section
        // (fuzzy: `settings cue`, `settings hotkeys`, `settings gesten`).
        const section = matchSettingsSection(arg);
        setQuery("");
        setSettingsJump(section ? { id: section.id, nonce: Date.now() } : null);
        setActiveTab("settings");
        return true;
      } else if (commandKind === "timer") {
        const t = parseTimerArg(arg);
        if (!t) {
          setPasteError("other");
          return true;
        }
        await startTimer(t.seconds, t.label);
        // showStatusToast hides the popup + plays the flourish (don't also
        // call hidePopup — that would swallow the toast on macOS).
        await showStatusToast("timer", true, "Timer set", t.label);
      } else if (commandKind === "alarm") {
        const a = parseAlarmArg(arg);
        if (!a) {
          setPasteError("other");
          return true;
        }
        await startTimer(a.seconds, `Alarm ${a.label}`);
        await showStatusToast("alarm", true, "Alarm set", a.label);
      } else if (commandKind === "md2pdf") {
        // Same action as Ctrl+Shift+M; arg = optional path (else selection).
        try {
          await mdToPdfRun(arg || undefined);
        } catch (e) {
          setPasteError("other");
          console.error("md2pdf failed", e);
          return true;
        }
        await hidePopup();
      } else if (
        commandKind === "shot-region" ||
        commandKind === "shot-full" ||
        commandKind === "shot-window" ||
        commandKind === "shot-last"
      ) {
        // Screenshot capture modes (v0.57.0). The backend hides the popup +
        // runs the same staging → clipboard → floating-preview flow.
        const delay = parseShotDelay(arg);
        try {
          if (commandKind === "shot-last") {
            await screenshotRepeatLast();
          } else {
            const mode =
              commandKind === "shot-full"
                ? "fullscreen"
                : commandKind === "shot-window"
                  ? "window"
                  : "region";
            await screenshotCapture(mode, delay);
          }
        } catch (e) {
          setPasteError("other");
          console.error("screenshot failed", e);
          return true;
        }
        // Backend hides the popup; region mode also drives its own UI.
      } else if (commandKind === "brightness") {
        // Render the sliders inline in the right preview column and give the
        // arrow keys to them — the popup stays open. A repeated Enter (handled
        // inside BrightnessPanel) hands the arrows back to the left list.
        // (Query canonicalisation is centralised at the top of dispatchCommand.)
        setBrightnessMode(true);
        setBrightnessFocus(true);
        return true;
      } else if (commandKind === "sound") {
        // Inline output-device picker in the preview column (like brightness).
        setSoundMode(true);
        setSoundFocus(true);
        return true;
      } else if (commandKind === "hue") {
        // Inline Philips Hue lamp controls in the preview column.
        setHueMode(true);
        setHueFocus(true);
        return true;
      } else if (commandKind === "iris") {
        // Resolve against the LIVE backend state, never against React state:
        // the popup may have been reopened long after arming, and an external
        // change must not desync the toggle. (v0.102.2: the previous version
        // also handed the panel keyboard focus on arm, which disabled the list
        // nav — so the second Enter never even reached here and `iris 55`
        // followed by `iris` left it armed.)
        const live = await irisStatus().catch(() => null);
        const action = irisAction(parseIrisArg(arg), !!live?.active);
        try {
          switch (action.kind) {
            case "disarm":
              await irisStop();
              setIrisActive(false);
              setIrisMode(false);
              setQuery("");
              // Hides the popup + plays the flourish, so no separate hidePopup().
              await showStatusToast("iris", false, "Iris aus", "Mikrofon-Überwachung beendet");
              break;
            case "retune":
              await irisSetThreshold(action.threshold);
              setIrisActive(true);
              setIrisMode(true);
              break;
            case "arm":
              await irisStart(action.threshold);
              setIrisActive(true);
              setIrisMode(true);
              break;
            case "none":
              setIrisActive(false);
              setIrisMode(false);
              break;
          }
        } catch (e) {
          await showStatusToast("iris", false, "Iris fehlgeschlagen", String(e));
        }
        return true;
      } else if (commandKind === "stats") {
        // Inline live system-stats panel in the preview column (read-only).
        setStatsMode(true);
        setStatsFocus(true);
        return true;
      } else if (commandKind === "boom") {
        // Inline audio-enhancement controller in the preview column.
        setBoomMode(true);
        setBoomFocus(true);
        return true;
      } else if (commandKind === "uptime") {
        // Inline live animated uptime odometer in the preview column.
        setUptimeMode(true);
        setUptimeFocus(true);
        return true;
      } else if (commandKind === "weather") {
        // Inline animated weather panel (current + 5-day) in the preview column.
        setWeatherMode(true);
        setWeatherFocus(true);
        return true;
      } else if (commandKind === "tokens") {
        // Inline Claude Code usage from the local Token Tracker.
        setTokensMode(true);
        setTokensFocus(true);
        return true;
      } else if (commandKind === "calendar") {
        // Inline month-view calendar in the preview column.
        setCalendarMode(true);
        setCalendarFocus(true);
        return true;
      } else if (commandKind === "disco") {
        // Toggle the persistent beat-sync engine (runs even after the popup
        // closes). `disco 1`/`0` force on/off; bare `disco` toggles. Fire-and-
        // forget; the engine surfaces its own error (e.g. no bridge) in the
        // hue panel. Close the popup so the lights take the stage.
        const on = parseDiscoArg(arg);
        const want = on === null ? !discoEngine.isRunning() : on;
        if (want) void discoEngine.start();
        else discoEngine.stop();
        await hidePopup();
        return true;
      } else if (commandKind === "random") {
        // The RandomPanel shows the rolled number live in the preview; Enter
        // pastes EXACTLY that number (fall back to a fresh roll if the panel
        // hasn't reported one yet). Re-rolling is the panel's button.
        const r = parseRandomArg(arg);
        if (!r) {
          setPasteError("other");
          return true;
        }
        const n = randomValueRef.current ?? randomInt(r.min, r.max);
        await pasteText(String(n));
        await hidePopup();
      } else if (commandKind === "clean") {
        // Open the interactive category picker in the preview column (scan →
        // checkbox selection → two-stage delete). Replaces the old
        // all-or-nothing window.confirm, which offered no choice (and native
        // confirm is unreliable in the Tauri webview — the TOTP-delete lesson).
        setCleanMode(true);
        setCleanFocus(true);
      } else if (commandKind === "snitch") {
        // `snitch` → app blocker; `snitch map`/`connections`/`show` → world map.
        const mapMode = /\b(map|conn|show|world)/i.test(arg ?? "");
        setSnitchMode(mapMode ? "map" : "apps");
        setSnitchFocus(true);
      } else if (commandKind === "shazam") {
        // `shazam` → record + identify; `shazam history` → the history list.
        const histMode = /\b(hist|list)/i.test(arg ?? "");
        setShazamMode(histMode ? "history" : "listen");
        setShazamSignal((n) => n + 1);
        setShazamFocus(true);
      } else {
        // Not dispatched here (e.g. pwgen has its own preview ListEntry,
        // kill runs in kill-mode). Let the caller decide what to do.
        return false;
      }
      return true;
    };

    try {
      // bpm trigger — Enter swaps the popup body for the BPM
      // detector overlay (asks for mic permission, starts listening).
      // No paste, no clipboard write; the popup stays open until the
      // user hits Esc inside the detector.
      if (target.kind === "bpm") {
        setBpmMode(true);
        return;
      }
      // equalizer trigger — Enter swaps the popup body for the live
      // spectrum visualizer (same mic + pin/Esc behaviour as bpm).
      if (target.kind === "equalizer") {
        setEqualizerMode(true);
        return;
      }
      // 2FA management overlay — Enter swaps the popup body for the
      // TOTP manager (list / add / import / export). Esc returns.
      if (target.kind === "totp-manage") {
        setTotpMode(true);
        return;
      }
      // TOTP autocomplete row — Enter copies the 6-digit code to
      // the clipboard + hides the popup (analog to pwgen Enter).
      if (target.kind === "totp") {
        const { writeText } = await import("@tauri-apps/plugin-clipboard-manager");
        await writeText(target.data.code);
        await hidePopup();
        console.info(`totp → copied ${target.data.issuer} code`);
        return;
      }
      // pwgen has special Enter semantics: copies the password (no
      // paste — explicit, since the user is rarely typing into a
      // focused-app password field directly from Inspector Rust's
      // popup). Alt+Enter switches mode to alphanumeric + re-rolls
      // + copies, all in one shot.
      if (target.kind === "pwgen") {
        let modeToCopy: PwgenMode = target.data.mode;
        let passwordToCopy = target.data.password;
        if (altKey) {
          modeToCopy = "alnum";
          setPwgenMode("alnum");
          // Regenerate with the new mode so the user actually gets
          // alnum on their clipboard, not whatever was on screen.
          passwordToCopy = generatePassword("alnum", target.data.length);
          setPwgenSeed((s) => s + 1);
        }
        const { writeText } = await import("@tauri-apps/plugin-clipboard-manager");
        await writeText(passwordToCopy);
        await hidePopup();
        // Acknowledge in console for the user's debugging convenience.
        console.info(`pwgen → copied ${target.data.length}-char ${modeToCopy} password`);
        return;
      }
      if (target.kind === "snippet") {
        await pasteSnippet(target.data.id);
      } else if (target.kind === "calc") {
        await pasteText(target.data.display);
      } else if (target.kind === "color") {
        await pasteText(target.data.pasteValue);
      } else if (target.kind === "opener") {
        // Easter-egg paste — drop the German opener into the focused app.
        await pasteText(target.data.text);
      } else if (target.kind === "app") {
        // Spotlight-like launch. macOS Launch Services activates an
        // already-running instance instead of spawning a duplicate.
        await launchApp(target.data.path);
        await hidePopup();
      } else if (target.kind === "bruno") {
        // SHIFT+Enter copies the COMPLETE breakdown (assumptions + every
        // deduction row + net, exactly what the preview shows) as aligned
        // plain text to the clipboard — for mails/notes.
        if (shiftKey) {
          const { writeText } = await import("@tauri-apps/plugin-clipboard-manager");
          const text = target.data.self
            ? formatBrunoSelfBreakdown(target.data.self)
            : formatBrunoBreakdown(target.data);
          await writeText(text);
          await hidePopup();
          return;
        }
        // Plain Enter: paste the net amount — period-matched. Typing
        // `bruno 5000m` → user is thinking monthly → paste monthly net.
        // Typing `bruno 60000` → yearly. German number format (de-DE Intl).
        const v =
          target.data.period === "monthly" ? target.data.netMonth : target.data.netYear;
        const formatted = new Intl.NumberFormat("de-DE", {
          style: "currency",
          currency: "EUR",
          maximumFractionDigits: 2,
        }).format(v);
        await pasteText(formatted);
      } else if (target.kind === "command-suggestion") {
        // Fuzzy/prefix command suggestion. If the completion is already a
        // complete, runnable command (no-arg commands like wakelock /
        // freeze / lock, or a resize preset), RUN it directly on Enter —
        // that's the one-keystroke fuzzy invoke. Tab / → autocompletes
        // without running (handled by the global keydown effect below).
        // Anything still needing an argument (or pwgen, which shows a live
        // preview) yields `false` from dispatchCommand → fill the input.
        const parsed = parseCommand(target.data.completion);
        if (
          parsed &&
          (await dispatchCommand(parsed.spec.kind, parsed.arg, parsed.spec.keyword))
        ) {
          return;
        }
        setQuery(target.data.completion);
        requestAnimationFrame(() => {
          searchRef.current?.focus();
          const len = target.data.completion.length;
          searchRef.current?.setSelectionRange(len, len);
        });
        return;
      } else if (target.kind === "command") {
        // Runnable power command — dispatch by kind (shared with the
        // fuzzy command-suggestion path above). Pass the typed keyword so
        // the wakelock toast can brand itself Caffeine vs Wakelock.
        const kw = target.data.rawInput.trim().split(/\s+/)[0]?.toLowerCase();
        await dispatchCommand(target.data.commandKind, target.data.arg, kw);
        return;
      } else if (target.kind === "kill-target") {
        // Destructive: confirm before killing. The dialog shows the
        // exact PID + name so the user can't mistake which process
        // they're terminating.
        const { pid, name, force } = target.data;
        const sig = force ? "SIGKILL (force quit)" : "SIGTERM (graceful)";
        if (!(await confirmDialog(`${name}\nPID ${pid}\nSignal: ${sig}`, "Kill process?"))) {
          return;
        }
        await killProcess(pid, force);
        // Stay open — the user might want to kill another one. Just
        // refresh the snapshot by triggering a re-fetch.
        setProcessSnapshot((cur) => cur.filter((p) => p.pid !== pid));
        return;
      } else if (target.kind === "finder-file") {
        // Open the file in the system default app. The popup hides
        // so focus snaps to whichever app takes it (Preview, etc.).
        await openUrl(`file://${target.data.path}`);
        await hidePopup();
      } else if (target.kind === "meme") {
        // Copy the meme file to the clipboard (animation preserved), then
        // hide so the user can paste it wherever they were.
        try {
          await copyMeme(target.data.path);
        } catch (e) {
          setPasteError("other");
          console.error("copy_meme failed", e);
          return;
        }
        await hidePopup();
      } else if (target.kind === "figlet-font") {
        // Render the selected font's FULL banner (with the current chip opts).
        // Enter pastes it as text; SHIFT+Enter copies it as a tightly-cropped,
        // theme-coloured PNG (clipboard + history) — for targets that mangle
        // monospace; CMD/CTRL+SHIFT+Enter renders it with a TRANSPARENT
        // background (glyphs in the theme text colour) and does BOTH — saves
        // it to ~/Downloads (revealed) AND copies it to the clipboard.
        try {
          const banner = await figletRender(figletParsed.text, target.data.name, figletOpts);
          if (!banner.text.trim()) return; // nothing to render (empty/all-unsupported)
          if (shiftKey) {
            const png = bannerPngBase64(banner.text, themePngColors(metaKey));
            if (!png) return;
            if (metaKey) {
              // Copy first (clipboard + history), then save + reveal — the
              // reveal steals focus, so it must be the last visible action.
              await figletCopyPng(png, figletParsed.text);
              await figletSavePng(png, figletParsed.text);
            } else {
              await figletCopyPng(png, figletParsed.text);
            }
            await hidePopup();
          } else {
            await pasteGenerated(banner.text, figletDefaults?.save_history ?? true);
          }
        } catch (e) {
          setPasteError("other");
          console.error("figlet_render/paste failed", e);
        }
        return;
      } else if (target.kind === "social") {
        // Download the Tab-selected mode (video by default; YouTube also offers
        // audio — Tab flips it). Routed through the preview bar via a run signal
        // so its progress animation shows; the popup stays open and the backend
        // reveals the file in Finder on completion.
        setSocialRunSignal((n) => n + 1);
        return;
      } else if (target.kind === "help") {
        // `?`-index row: put the command into the search bar, ready to use —
        // with a trailing space when it takes an argument, so typing continues
        // seamlessly. The popup stays open.
        const spec = COMMANDS.find((c) => c.keyword === target.data.command);
        const needsArg = spec ? spec.syntax.trim() !== spec.keyword : false;
        const completion = target.data.command + (needsArg ? " " : "");
        setQuery(completion);
        requestAnimationFrame(() => {
          searchRef.current?.focus();
          searchRef.current?.setSelectionRange(completion.length, completion.length);
        });
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

  // Keep the "activate the selected row" ref current for the native
  // Enter-while-unfocused event (popup-activate-selected). Written in an effect
  // (never during render) so it captures the latest `activate` + `selected`.
  useEffect(() => {
    activateSelectedRef.current = () => void activate(selected);
  });

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

  const onTogglePin = async (i: number, pinned: boolean) => {
    const target = combined[i];
    if (!target || target.kind !== "clip") return;
    try {
      await setClipPinned(target.data.id, pinned);
      // The backend emits clipboard-changed, but refresh eagerly so the row
      // re-orders without waiting for the event round-trip.
      await refreshHistory();
    } catch (e) {
      console.error("toggle pin failed", e);
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
    onEnter: (shiftKey, altKey, metaKey) => void activate(selected, shiftKey, altKey, metaKey),
    onEscape: () => {
      void hidePopup();
    },
    // Shift+↑ / Shift+↓ adjust the system output volume by ±5 points,
    // grid-snapped backend-side so repeated presses land on clean multiples
    // of 5. Fire-and-forget — macOS plays its volume feedback.
    onShiftArrow: (direction) => {
      void adjustVolume(direction === "up" ? 5 : -5).catch((e) =>
        console.error("adjust_volume failed", e),
      );
    },
    // In game mode the game owns the keyboard — disable the popup nav
    // handler so Esc / arrows don't double-fire. BPM mode + TOTP mode
    // own it too (Esc inside each overlay calls its own onExit).
    enabled:
      activeTab === "history" &&
      !gameMode &&
      !bpmMode &&
      !equalizerMode &&
      !totpMode &&
      !pwgenEditing &&
      // The inline snippet editor owns the keyboard while it's open — otherwise
      // Enter would paste the snippet instead of saving the edit.
      snippetEditingId === null &&
      !brightnessFocus &&
      !soundFocus &&
      !hueFocus &&
      !statsFocus &&
      !boomFocus &&
      !uptimeFocus &&
      !weatherFocus &&
      !tokensFocus &&
      !calendarFocus &&
      !cleanFocus &&
      !snitchFocus &&
      !shazamFocus,
  });

  // Global Esc fallback (v0.87.6): Esc must close the popup on EVERY tab.
  // The list-nav handler above only runs on the History tab, so Settings /
  // Snippets / Notes / Timesheet / Features had NO Esc path at all. This
  // listener runs in the BUBBLE phase and only when no specialised handler
  // consumed the event (games, inline panels, overlays and the list nav all
  // preventDefault their Esc — their back/exit semantics stay untouched).
  // A focused form field (not the main search box) blurs first — the "back"
  // step — so the second Esc closes the popup.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== "Escape" || e.repeat || e.defaultPrevented || e.isComposing) return;
      const el = document.activeElement as HTMLElement | null;
      if (
        el &&
        el !== searchRef.current &&
        (el.tagName === "INPUT" ||
          el.tagName === "TEXTAREA" ||
          el.tagName === "SELECT" ||
          el.isContentEditable)
      ) {
        e.preventDefault();
        el.blur();
        return;
      }
      e.preventDefault();
      void hidePopup();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [hidePopup]);


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
          ) : gameMode === "space" ? (
            <SpaceInvadersGame onExit={exitGame} />
          ) : gameMode === "flappy" ? (
            <FlappyGame onExit={exitGame} />
          ) : (
            <SnakeGame onExit={exitGame} wrap={gameMode === "snake-wrap"} />
          )}
        </div>
      </div>
    );
  }

  // BPM mode — Enter on the `bpm` row takes over the app-shell with
  // the microphone-driven BPM detector. Esc inside the component
  // calls onExit (audio teardown handled there), which drops us back
  // to the normal popup with a cleared query.
  if (bpmMode) {
    const exitBpm = () => {
      setBpmMode(false);
      setQuery("");
      setSelected(0);
      // Drop any pin so click-outside dismiss works normally again.
      void setSuppressHide(false).catch(() => undefined);
      requestAnimationFrame(() => searchRef.current?.focus());
    };
    return (
      <div className="flex h-screen w-screen p-2">
        <div className="app-shell md3-pop-in flex h-full w-full flex-col">
          <BpmDetector onExit={exitBpm} />
        </div>
      </div>
    );
  }

  // Equalizer mode — Enter on the `equalizer`/`eq` row takes over the
  // app-shell with the microphone-driven spectrum visualizer. Same shape as
  // BPM mode: Esc inside the component calls onExit (audio teardown + pin
  // release handled there).
  if (equalizerMode) {
    const exitEqualizer = () => {
      setEqualizerMode(false);
      setQuery("");
      setSelected(0);
      void setSuppressHide(false).catch(() => undefined);
      requestAnimationFrame(() => searchRef.current?.focus());
    };
    return (
      <div className="flex h-screen w-screen p-2">
        <div className="app-shell md3-pop-in flex h-full w-full flex-col">
          <EqualizerVisualizer onExit={exitEqualizer} />
        </div>
      </div>
    );
  }

  // 2FA management — full overlay with list / add / import / export.
  if (totpMode) {
    const exitTotp = () => {
      setTotpMode(false);
      setQuery("");
      setSelected(0);
      requestAnimationFrame(() => searchRef.current?.focus());
    };
    return (
      <div className="flex h-screen w-screen p-2">
        <div className="app-shell md3-pop-in flex h-full w-full flex-col">
          <TotpOverlay onExit={exitTotp} onHidePopup={hidePopup} />
        </div>
      </div>
    );
  }

  return (
    <div className="flex h-screen w-screen p-2">
      <div ref={shellRef} className="app-shell fade-in flex h-full w-full flex-col">
        {/* Paste-failure banner — sticky at the top, click-to-dismiss. */}
        {pasteError && (
          <div
            className={
              "md3-banner-in flex items-start gap-2 border-b px-4 py-2 text-[12px] " +
              (pasteError === "ax"
                ? "border-amber-500/40 bg-amber-500/10"
                : "border-red-500/40 bg-red-500/10")
            }
          >
            <span className="flex-1">
              {pasteError === "ax" ? (
                <>
                  <b>Paste failed — macOS Accessibility access not granted.</b> Open the{" "}
                  <b>Settings</b> tab and click <b>Force re-grant</b> in the amber banner.
                  After granting in System Settings, click <b>Restart now</b>.
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
          <div className="md3-banner-in flex items-start gap-2 border-b border-amber-500/40 bg-amber-500/10 px-4 py-2 text-[12px]">
            <span className="flex-1">
              <b>OCR failed — macOS Screen Recording access not granted.</b> Without it,{" "}
              <code>screencapture</code> is denied and the region marquee never appears.
              Grant it in{" "}
              <b>System Settings → Privacy &amp; Security → Screen Recording</b> for
              Inspector Rust, then relaunch.
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
          <div className="md3-banner-in flex items-start gap-2 border-b border-amber-500/40 bg-amber-500/10 px-4 py-2 text-[12px]">
            <span className="flex-1">
              <b>Text expansion failed — macOS Accessibility access not granted.</b>{" "}
              Inspector Rust can&apos;t read the focused field or type the snippet without
              it. Use <b>Force re-grant</b> in the amber banner below, then click{" "}
              <b>Restart now</b>.
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

        {finderAutomationDenied && (
          <div className="md3-banner-in flex items-start gap-2 border-b border-amber-500/40 bg-amber-500/10 px-4 py-2 text-[12px]">
            <span className="flex-1">
              <b>Finder selection unavailable — macOS Automation access not granted.</b>{" "}
              Open{" "}
              <b>
                System Settings → Privacy &amp; Security → Automation → Inspector Rust
              </b>{" "}
              and toggle <b>Finder</b> on. Then press <b>Ctrl+Shift+F</b> again.
            </span>
            <button
              onClick={() => setFinderAutomationDenied(false)}
              className="rounded px-1.5 text-[var(--color-muted)] hover:bg-[var(--color-surface)]"
              title="Dismiss"
            >
              ×
            </button>
          </div>
        )}

        {expanderBlocked && (
          <div className="md3-banner-in flex items-start gap-2 border-b border-amber-500/40 bg-amber-500/10 px-4 py-2 text-[12px]">
            <span className="flex-1">
              {expanderBlocked === "password" ? (
                <>
                  <b>Text expansion blocked — focused field is a password input.</b>{" "}
                  Refusing on purpose: pasting a snippet into a credential field would
                  leak the body into your password manager / sudo prompt / OS password
                  dialog.
                </>
              ) : expanderBlocked === "secure_input" ? (
                <>
                  <b>Text expansion blocked — secure event input is active.</b> macOS is
                  suppressing synthetic input (typically because a password field is the
                  keyboard responder). Try again after leaving the secure field.
                </>
              ) : (
                <>
                  <b>Text expansion can't work in terminals.</b> Terminals don't expose
                  their input line to the accessibility API, and the keystroke-cycle
                  fallback (<code>⌥⇧←</code> select-word) becomes an ESC-sequence instead
                  of a selection. Workarounds:{" "}
                  <b>type the abbreviation here in the popup search bar</b> and press{" "}
                  <b>⏎</b> to paste, OR configure a <b>Direct hotkey → snippet</b> in
                  Settings (those bypass reading and work in any app, terminals included).
                </>
              )}
            </span>
            <button
              onClick={() => setExpanderBlocked(null)}
              className="rounded px-1.5 text-[var(--color-muted)] hover:bg-[var(--color-surface)]"
              title="Dismiss"
            >
              ×
            </button>
          </div>
        )}

        {timerFiredLabel && (
          <div className="md3-banner-in flex items-start gap-2 border-b border-[var(--color-accent)]/60 bg-[var(--color-accent)]/15 px-4 py-2 text-[12px]">
            <span className="flex-1">
              ⏰ <b>Timer done — {timerFiredLabel}</b>{" "}
              <span className="text-[var(--color-muted)]">
                · also fired a macOS notification + Glass sound
              </span>
            </span>
            <button
              onClick={() => setTimerFiredLabel(null)}
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
              calcMode={calcResult !== null || convertResult !== null}
            />
          ) : (
            <div className="flex h-14 items-center border-b border-[var(--color-border)] pl-4 pr-[260px]">
              <span className="text-[15px] font-semibold">
                {activeTab === "snippets"
                  ? "Snippets"
                  : activeTab === "notes"
                    ? "Notes"
                    : activeTab === "timesheet"
                      ? "Timesheet"
                      : activeTab === "features"
                        ? "Features"
                        : "Settings"}
              </span>
            </div>
          )}
          <TabBar
            active={activeTab}
            onSelect={(tab) => {
              setActiveTab(tab);
              if (tab === "snippets") void refreshSnippets();
              if (tab === "notes") void refreshNotes();
            }}
          />
        </div>

        {/* Content — keyed on the active tab so it replays the MD3
            emphasized-decelerate enter transition on every tab switch. */}
        <div key={activeTab} className="md3-tab-enter flex min-h-0 flex-1 flex-col">
          {activeTab === "history" ? (
            <div className="flex min-h-0 flex-1">
              <div
                className={
                  "w-2/5 border-r border-[var(--color-border)]" +
                  (listEntrance ? " md3-list-enter" : "")
                }
              >
                <HistoryList
                  entries={combined}
                  selectedIndex={selected}
                  onSelect={setSelected}
                  onActivate={activate}
                  onSaveAsNote={onSaveAsNote}
                  onDeleteClip={onDeleteClip}
                  onTogglePin={onTogglePin}
                  onClearAll={onClearAllHistory}
                  lineageHighlight={lineageHighlight}
                  pinnedOnly={pinnedOnly}
                  pinnedCount={pinnedCount}
                  onTogglePinnedOnly={togglePinnedOnly}
                />
              </div>
              <div className="w-3/5 min-w-0">
                {helpTarget ? (
                  <div className="md3-pop-in h-full">
                    <CommandHelp
                      target={(() => {
                        // Index mode: the preview follows the list selection —
                        // arrow through commands, read their full docs live.
                        if (helpTarget.kind === "index") {
                          const sel = combined[selected];
                          if (sel?.kind === "help") {
                            const doc = lookupDoc(sel.data.command);
                            if (doc) return { kind: "doc", doc } as HelpTarget;
                          }
                        }
                        return helpTarget;
                      })()}
                      onNavigate={(q) => {
                        setQuery(q);
                        requestAnimationFrame(() => searchRef.current?.focus());
                      }}
                    />
                  </div>
                ) : brightnessMode ? (
                  <div className="md3-pop-in h-full">
                    <BrightnessPanel
                      focused={brightnessFocus}
                      onUnfocus={() => {
                        setBrightnessFocus(false);
                        requestAnimationFrame(() => searchRef.current?.focus());
                      }}
                      onExit={() => {
                        setBrightnessMode(false);
                        setBrightnessFocus(false);
                        requestAnimationFrame(() => searchRef.current?.focus());
                      }}
                    />
                  </div>
                ) : soundMode ? (
                  <div className="md3-pop-in h-full">
                    <SoundPanel
                      focused={soundFocus}
                      onExit={() => {
                        setSoundMode(false);
                        setSoundFocus(false);
                        requestAnimationFrame(() => searchRef.current?.focus());
                      }}
                    />
                  </div>
                ) : hueMode ? (
                  <div className="md3-pop-in h-full">
                    <HuePanel
                      focused={hueFocus}
                      onExit={() => {
                        setHueMode(false);
                        setHueFocus(false);
                        requestAnimationFrame(() => searchRef.current?.focus());
                      }}
                    />
                  </div>
                ) : irisMode ? (
                  <div className="md3-pop-in h-full">
                    <IrisPanel />
                  </div>
                ) : statsMode ? (
                  <div className="md3-pop-in h-full">
                    <StatsPanel
                      focused={statsFocus}
                      onExit={() => {
                        setStatsMode(false);
                        setStatsFocus(false);
                        requestAnimationFrame(() => searchRef.current?.focus());
                      }}
                    />
                  </div>
                ) : boomMode ? (
                  <div className="md3-pop-in h-full">
                    <BoomPanel
                      focused={boomFocus}
                      onInteract={() =>
                        requestAnimationFrame(() => searchRef.current?.focus())
                      }
                      onExit={() => {
                        setBoomMode(false);
                        setBoomFocus(false);
                        requestAnimationFrame(() => searchRef.current?.focus());
                      }}
                    />
                  </div>
                ) : uptimeMode ? (
                  <div className="md3-pop-in h-full">
                    <UptimePanel
                      focused={uptimeFocus}
                      onExit={() => {
                        setUptimeMode(false);
                        setUptimeFocus(false);
                        requestAnimationFrame(() => searchRef.current?.focus());
                      }}
                    />
                  </div>
                ) : weatherMode ? (
                  <div className="md3-pop-in h-full">
                    <WeatherPanel
                      focused={weatherFocus}
                      arg={isWeatherCmd ? (parsedCommand?.arg ?? "") : ""}
                      onInteract={() =>
                        requestAnimationFrame(() => searchRef.current?.focus())
                      }
                      onExit={() => {
                        setWeatherMode(false);
                        setWeatherFocus(false);
                        requestAnimationFrame(() => searchRef.current?.focus());
                      }}
                    />
                  </div>
                ) : tokensMode ? (
                  <div className="md3-pop-in h-full">
                    <TokensPanel
                      focused={tokensFocus}
                      onExit={() => {
                        setTokensMode(false);
                        setTokensFocus(false);
                        requestAnimationFrame(() => searchRef.current?.focus());
                      }}
                    />
                  </div>
                ) : randomMode ? (
                  <div className="md3-pop-in h-full">
                    <RandomPanel
                      arg={isRandomCmd ? (parsedCommand?.arg ?? "") : ""}
                      onValue={(n) => {
                        randomValueRef.current = n;
                      }}
                      onInteract={() =>
                        requestAnimationFrame(() => searchRef.current?.focus())
                      }
                    />
                  </div>
                ) : calendarMode ? (
                  <div className="md3-pop-in h-full">
                    <CalendarPanel
                      focused={calendarFocus}
                      arg={isCalendarCmd ? (parsedCommand?.arg ?? "") : ""}
                      onInteract={() =>
                        requestAnimationFrame(() => searchRef.current?.focus())
                      }
                      onExit={() => {
                        setCalendarMode(false);
                        setCalendarFocus(false);
                        requestAnimationFrame(() => searchRef.current?.focus());
                      }}
                    />
                  </div>
                ) : cleanMode ? (
                  <div className="md3-pop-in h-full">
                    <CleanPanel
                      focused={cleanFocus}
                      onInteract={() =>
                        requestAnimationFrame(() => searchRef.current?.focus())
                      }
                      onExit={() => {
                        setCleanMode(false);
                        setCleanFocus(false);
                        requestAnimationFrame(() => searchRef.current?.focus());
                      }}
                    />
                  </div>
                ) : snitchMode === "apps" ? (
                  <div className="md3-pop-in h-full">
                    <SnitchPanel
                      focused={snitchFocus}
                      onInteract={() =>
                        requestAnimationFrame(() => searchRef.current?.focus())
                      }
                      onExit={() => {
                        setSnitchMode(null);
                        setSnitchFocus(false);
                        requestAnimationFrame(() => searchRef.current?.focus());
                      }}
                    />
                  </div>
                ) : snitchMode === "map" ? (
                  <div className="md3-pop-in h-full">
                    <SnitchMapPanel
                      focused={snitchFocus}
                      onExit={() => {
                        setSnitchMode(null);
                        setSnitchFocus(false);
                        requestAnimationFrame(() => searchRef.current?.focus());
                      }}
                    />
                  </div>
                ) : shazamMode ? (
                  <div className="md3-pop-in h-full">
                    <ShazamPanel
                      focused={shazamFocus}
                      initialView={shazamMode === "history" ? "history" : "recognize"}
                      viewSignal={shazamSignal}
                      onExit={() => {
                        setShazamMode(null);
                        setShazamFocus(false);
                        requestAnimationFrame(() => searchRef.current?.focus());
                      }}
                    />
                  </div>
                ) : (
                  <div ref={previewFadeRef} className="h-full min-h-0">
                  <PreviewPanel
                    entry={current}
                    pwgenMode={pwgenMode}
                    onPwgenModeChange={(m) => {
                      setPwgenMode(m);
                      setPwgenSeed((s) => s + 1);
                    }}
                    onPwgenReroll={() => setPwgenSeed((s) => s + 1)}
                    onPwgenEdit={(value) =>
                      setPwgenEdit({ seed: pwgenSeed, query, mode: pwgenMode, value })
                    }
                    onPwgenEditingChange={setPwgenEditing}
                    onPwgenCommit={async () => {
                      if (current?.kind !== "pwgen") return;
                      const { writeText } =
                        await import("@tauri-apps/plugin-clipboard-manager");
                      await writeText(current.data.password);
                      await hidePopup();
                    }}
                    socialMode={socialMode}
                    onSocialModeChange={setSocialMode}
                    socialRunSignal={socialRunSignal}
                    fakerCatalog={fakerCat}
                    fakerDefaults={fakerDef}
                    fakerReroll={fakerReroll}
                    secCatalog={secCat}
                    secDefaults={secDef}
                    figletText={figletParsed.text}
                    figletOpts={figletOpts}
                    onFigletOptsChange={(patch) =>
                      setFigletOptsOverride((prev) => ({ ...prev, ...patch }))
                    }
                    liveTranslation={liveTranslation}
                    snippetCategories={snippetCategories}
                    snippetEditing={
                      current?.kind === "snippet" && snippetEditingId === current.data.id
                    }
                    onSnippetEdit={() => {
                      if (current?.kind === "snippet") setSnippetEditingId(current.data.id);
                    }}
                    onSnippetSaved={async () => {
                      await refreshSnippets();
                      setSnippetRev((r) => r + 1); // re-run the snippet search
                      setSnippetEditingId(null);
                      // Hand the keyboard back to the list so Enter pastes the
                      // edited version straight away — the query is untouched.
                      searchRef.current?.focus();
                    }}
                    onSnippetEditCancel={() => {
                      setSnippetEditingId(null);
                      searchRef.current?.focus();
                    }}
                  />
                  </div>
                )}
              </div>
            </div>
          ) : activeTab === "snippets" ? (
            <SnippetsPanel snippets={snippets} categories={snippetCategories} onRefresh={refreshSnippets} />
          ) : activeTab === "notes" ? (
            <NotesPanel
              notes={notes}
              categories={noteCategories}
              onRefresh={refreshNotes}
            />
          ) : activeTab === "timesheet" ? (
            <TimesheetPanel />
          ) : activeTab === "features" ? (
            <FeaturesPanel />
          ) : (
            <SettingsPanel
              jumpTo={settingsJump}
              onBackupImported={async () => {
                // After a Backup → Import, refresh every list that might
                // have new rows. History reloads itself via the
                // `clipboard-changed` event the watcher emits, but Notes
                // and Snippets need an explicit nudge.
                await Promise.all([refreshHistory(), refreshSnippets(), refreshNotes()]);
              }}
            />
          )}
        </div>

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
          wakelockActive={wakelockActive}
          activeTimerCount={activeTimerCount}
          trackingActive={trackStatusState.active}
          trackingPaused={trackStatusState.paused}
        />
      </div>
    </div>
  );
}

const TAB_ITEMS = [
  ["history", "History"],
  ["snippets", "Snippets"],
  ["notes", "Notes"],
  ["timesheet", "Timesheet"],
  ["features", "Features"],
  ["settings", "Settings"],
] as const;

type TabId = (typeof TAB_ITEMS)[number][0];

/** Header tab bar with a SLIDING pill indicator (v0.88.0): the accent pill is
 *  one shared element that glides under the active tab (measured left/width,
 *  transform+width transition on the MD3 emphasized curve) instead of each
 *  button repainting its own background. Buttons keep the tactile press. */
function TabBar({
  active,
  onSelect,
}: {
  active: TabId;
  onSelect: (tab: TabId) => void;
}) {
  const btnRefs = useRef<Partial<Record<TabId, HTMLButtonElement | null>>>({});
  const [pill, setPill] = useState<{ x: number; w: number } | null>(null);

  useLayoutEffect(() => {
    const el = btnRefs.current[active];
    if (!el) return;
    setPill({ x: el.offsetLeft, w: el.offsetWidth });
  }, [active]);

  return (
    <div className="absolute right-3 top-1/2 flex -translate-y-1/2 gap-1">
      {/* The gliding indicator — behind the labels. */}
      {pill && (
        <span
          aria-hidden
          className="absolute top-0 left-0 h-full rounded bg-[var(--color-accent)] transition-[transform,width] duration-300 ease-[cubic-bezier(0.2,0,0,1)] motion-reduce:transition-none"
          style={{ transform: `translateX(${pill.x}px)`, width: pill.w }}
        />
      )}
      {TAB_ITEMS.map(([id, label]) => (
        <button
          key={id}
          ref={(el) => {
            btnRefs.current[id] = el;
          }}
          onClick={() => onSelect(id)}
          className={
            "relative z-10 rounded px-2 py-1 text-[11px] font-medium whitespace-nowrap transition-colors duration-200 active:scale-90 " +
            (active === id
              ? "text-[var(--color-accent-fg)]"
              : "text-[var(--color-muted)] hover:bg-[var(--color-surface)] hover:text-[var(--color-fg)]")
          }
        >
          {label}
        </button>
      ))}
    </div>
  );
}

export default App;
