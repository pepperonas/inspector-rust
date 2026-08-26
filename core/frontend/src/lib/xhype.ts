/**
 * `x!` — timeline + maths for the full-screen spectacle (v0.133.0).
 *
 * Six acts, ~15 s, one canvas, one rAF. Everything deterministic and
 * allocation-free in the hot path; this module holds the PURE parts so the
 * choreography is testable without a GPU:
 *
 *   I  IGNITION  a single ember wakes in the black
 *   II GRID      a technocratic horizon rushes the viewer
 *   III SLOP     the feed floods — glyph rain, corrupted text, chroma tear
 *   IV BURN      it all catches fire
 *   V  NOVA      white collapse → supernova → warp into space
 *   VI VOID      the stars go out
 *
 * ⚠️ **Photosensitivity is a real hazard here**, not a nicety: this piece is
 * meant to be violent, so `FLASH_MIN_GAP_MS` keeps FULL-SCREEN flashes below
 * the WCAG 2.3.1 three-per-second threshold. Localised sparks, embers and
 * scanline jitter are unconstrained — it's the whole-field luminance jumps
 * that trigger seizures. Never lower that gap to "make it punchier".
 */

export interface XAct {
  key: "ignition" | "grid" | "slop" | "burn" | "nova" | "void";
  /** Act start, ms from the beginning. */
  at: number;
  /** Duration, ms. */
  dur: number;
  /** Shown in the corner readout — the piece narrates itself. */
  caption: string;
}

/** Act captions per mode — the HUD narrates whatever the piece is about. */
const CAPTIONS: Record<XMode, readonly string[]> = {
  features: ["I · BOOT", "II · ARSENAL", "III · DURCHSATZ", "IV · LAST", "V · VOLLGAS", "VI · IDLE"],
  news: ["I · TICKER", "II · LAGE", "III · RAUSCHEN", "IV · BRAND", "V · SCHLAGZEILE", "VI · ARCHIV"],
};

/** Six acts over 30 s — the v0.134 proportions, doubled. */
export const ACTS: readonly XAct[] = [
  { key: "ignition", at: 0, dur: 3600, caption: "I" },
  { key: "grid", at: 3600, dur: 6400, caption: "II" },
  { key: "slop", at: 10000, dur: 6400, caption: "III" },
  { key: "burn", at: 16400, dur: 6400, caption: "IV" },
  { key: "nova", at: 22800, dur: 4000, caption: "V" },
  { key: "void", at: 26800, dur: 3200, caption: "VI" },
];

/** The HUD caption for an act in a given mode. */
export function captionFor(mode: XMode, index: number): string {
  return CAPTIONS[mode][index] ?? ACTS[index]?.caption ?? "";
}

/** Total runtime in ms. */
export const X_DURATION = ACTS[ACTS.length - 1].at + ACTS[ACTS.length - 1].dur;

/** Minimum gap between full-screen flashes — WCAG 2.3.1 allows at most three
 *  per second; 340 ms is just under that, deliberately. */
export const FLASH_MIN_GAP_MS = 340;

/** Which act is playing at `t` ms, and how far through it we are (0..1).
 *  Before the start → the first act at 0; past the end → `null`. */
export function actAt(t: number): { act: XAct; local: number; index: number } | null {
  if (t >= X_DURATION) return null;
  const clamped = Math.max(0, t);
  for (let i = ACTS.length - 1; i >= 0; i--) {
    const a = ACTS[i];
    if (clamped >= a.at) return { act: a, local: (clamped - a.at) / a.dur, index: i };
  }
  return { act: ACTS[0], local: 0, index: 0 };
}

/** May a full-screen flash fire now? Guards the photosensitivity threshold. */
export function flashAllowed(lastFlashAt: number | null, now: number): boolean {
  return lastFlashAt === null || now - lastFlashAt >= FLASH_MIN_GAP_MS;
}

export const clamp01 = (x: number): number => (x < 0 ? 0 : x > 1 ? 1 : x);
export const easeOut = (x: number): number => 1 - Math.pow(1 - clamp01(x), 3);
export const easeIn = (x: number): number => Math.pow(clamp01(x), 3);
/** 0 → 1 → 0 across the unit interval; the shape of every stab and flare. */
export const arc = (x: number): number => Math.sin(Math.PI * clamp01(x));

/** Deterministic hash noise in [0,1) — no `Math.random` in the render loop, so
 *  a frame can be reproduced (and tested). */
export function noise(i: number, seed = 1): number {
  let h = Math.imul(i ^ seed, 0x27d4eb2d) >>> 0;
  h ^= h >>> 15;
  h = Math.imul(h, 0x85ebca6b) >>> 0;
  h ^= h >>> 13;
  return (h >>> 0) / 4294967296;
}

/** Perspective projection for the rushing grid: a line at depth `z` (0 = at
 *  the viewer, 1 = at the horizon) maps to a screen y. */
export function horizonY(z: number, h: number, horizon: number): number {
  const d = Math.max(0.0001, 1 - clamp01(z));
  return horizon + (h - horizon) * d * d;
}

/** One warp star's radial position after `p` (0..1) of its flight. */
export function warpRadius(p: number, maxR: number): number {
  return easeIn(p) * maxR;
}

/** The palette. Black ground, ember heat, technocrat cold, white as
 *  punctuation only. */
export const PALETTE = {
  ember: "#ff3b0f",
  flame: "#ff8c1a",
  gold: "#ffc857",
  cold: "#7c3aed",
  cyan: "#22d3ee",
  bone: "#e8e4dc",
} as const;

export type XMode = "features" | "news";
export type XWords = Readonly<Record<XAct["key"], readonly string[]>>;

/**
 * What the app can DO, in single words — the `x!` showcase names capabilities,
 * not command keywords: "REPO" or "SNITCH" tell a stranger nothing, "RIPPER"
 * and "FIREWALL" do. Curated on purpose (this is editorial copy, not a
 * restatement of the command registry — see the registry invariant in
 * CLAUDE.md for why that distinction matters), and long enough that two runs
 * rarely draw the same set.
 *
 * When a genuinely new capability ships, add its word here.
 */
export const FEATURES: readonly string[] = [
  "TRANSLATOR", "CALCULATOR", "CONVERTER", "RIPPER", "SECTOOLS",
  "SCREENSHOTS", "RECORDER", "TEXTSCANNER", "COLORPICKER", "EYEDROPPER",
  "QR-CODES", "PASSWORDS", "AUTHENTICATOR", "SNIPPETS", "EXPANDER",
  "CLIPBOARD", "TIMESHEET", "EQUALIZER", "BPM-DETEKTOR", "SHAZAM",
  "WELTZEIT", "WETTER", "DISKUSAGE", "CODEZÄHLER", "REPO-STATS",
  "ANDROID", "HUE-LICHT", "HELLIGKEIT", "TRIMMER", "CLEANER",
  "FIREWALL", "SYSTEMSTATS", "UPTIME", "ALIAS-BUILDER", "KALENDER",
  "TIMER", "ALARM", "WACHHALTER", "PROZESS-KILLER", "TESTDATEN",
  "ASCII-ART", "MARKDOWN→PDF", "BACKUP", "NOTIZEN", "LAUNCHER",
  "FENSTER-SNAP", "GESTEN", "IRIS", "DISCO", "FREISTELLER",
  "OPTIMIERER", "RESIZER", "NETTOLOHN", "WELTKARTE", "DATEIMANAGER",
  "OCR", "AUDIO-SWAP", "EMOJI-TEXT", "SUCHMASCHINEN", "TASTENKÜRZEL",
];

/** Fisher-Yates with the deterministic `noise` — a shuffle you can seed. */
export function shuffle<T>(items: readonly T[], seed: number): T[] {
  const out = [...items];
  for (let i = out.length - 1; i > 0; i--) {
    const j = Math.floor(noise(i, seed) * (i + 1));
    [out[i], out[j]] = [out[j], out[i]];
  }
  return out;
}

/**
 * `x!` — a randomised tour through what the app can do. `commandCount` is the
 * live registry size (passed in, so this stays pure) and only feeds the nova's
 * tally; the words themselves come from [`FEATURES`]. A fresh seed per run
 * means a different eleven capabilities each time.
 */
export function featureWords(commandCount: number, seed: number): XWords {
  const f = shuffle(FEATURES, seed);
  const at = (i: number) => f[i % f.length] ?? "INSPECTOR";
  return {
    ignition: ["INSPECTOR"],
    grid: [at(0), at(1), at(2), at(3)],
    slop: [at(4), at(5), at(6), at(7)],
    burn: [at(8), at(9)],
    nova: [commandCount > 0 ? `${commandCount} BEFEHLE` : "ALLES AN BORD"],
    void: ["⌃ SPACE"],
  };
}

/**
 * `x!!` — today's headlines. Only HEADLINES (short factual statements) with
 * the source named in the HUD; never article text. Long ones are shortened
 * on a word boundary so 200px type stays legible.
 */
export function newsWords(headlines: readonly string[], seed: number): XWords {
  const hs = shuffle(
    headlines.filter((h) => h.trim().length > 0).map((h) => shorten(h)),
    seed,
  );
  if (hs.length === 0) return featureWords(0, seed);
  const at = (i: number) => hs[i % hs.length];
  return {
    ignition: ["HEUTE.."],
    grid: [at(0), at(1), at(2)],
    slop: [at(3), at(4), at(5), at(6)],
    burn: [at(7), at(8)],
    nova: [at(0)],
    void: ["…tagesschau.de"],
  };
}

/** Trim a headline to something a display face can carry: cut on a word
 *  boundary, never mid-word, and only when it's genuinely long. Pure. */
export function shorten(text: string, max = 42): string {
  const t = text.replace(/\s+/g, " ").trim();
  if (t.length <= max) return t;
  const cut = t.slice(0, max);
  const sp = cut.lastIndexOf(" ");
  return (sp > max * 0.5 ? cut.slice(0, sp) : cut).trim() + "…";
}

/** Corrupt a word with combining marks — the "AI slop" texture. `amount` 0..1
 *  scales how many marks pile on. Pure + deterministic. */
export function corrupt(word: string, amount: number, seed = 1): string {
  const MARKS = ["́", "̰", "҉", "͓", "ͫ", "͈", "͜"];
  const n = Math.round(clamp01(amount) * 3);
  if (n === 0) return word;
  let out = "";
  let i = 0;
  for (const ch of word) {
    out += ch;
    if (ch !== " ") {
      for (let k = 0; k < n; k++) {
        out += MARKS[Math.floor(noise(i * 7 + k, seed) * MARKS.length)];
      }
    }
    i++;
  }
  return out;
}
