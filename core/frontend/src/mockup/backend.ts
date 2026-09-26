// Dummy backend for the mockup harness (mockup.html). Every answer is invented; nothing here reads
// the machine. Commands without a handler answer `null` and are recorded in `window.__mockMisses`,
// so a new panel that needs data shows up in the harness log instead of rendering silently empty.
import { emit } from "@tauri-apps/api/event";
import type { ClipEntry, Note, Snippet } from "../lib/types";

const misses = new Set<string>();
const NOW = Date.now();
const min = 60_000;

function clip(id: number, text: string, ago: number, extra: Partial<ClipEntry> = {}): ClipEntry {
  return {
    id, content_type: "text", content_text: text, content_data: text, hash: `h${id}`,
    byte_size: new TextEncoder().encode(text).length, created_at: NOW - ago * min,
    last_used_at: NOW - ago * min, pinned: false, note: null, derived_from: null, derived_kind: null,
    ...extra,
  };
}

let imageData: string | null = null;
function demoImage(): string {
  if (imageData) return imageData;
  const c = document.createElement("canvas");
  c.width = 960; c.height = 600;
  const g = c.getContext("2d")!;
  const grad = g.createLinearGradient(0, 0, 960, 600);
  grad.addColorStop(0, "#312e81"); grad.addColorStop(1, "#0f172a");
  g.fillStyle = grad; g.fillRect(0, 0, 960, 600);
  g.fillStyle = "rgba(165,180,252,.9)";
  [[120, 360, 90], [260, 300, 150], [400, 240, 210], [540, 200, 250], [680, 150, 300]].forEach(([x, y, h]) =>
    g.fillRect(x, y, 90, h));
  g.fillStyle = "#e0e7ff"; g.font = "600 38px -apple-system, sans-serif";
  g.fillText("Weekly active users", 120, 110);
  g.fillStyle = "#a5b4fc"; g.font = "24px -apple-system, sans-serif";
  g.fillText("+18 % week over week", 120, 150);
  imageData = c.toDataURL("image/png").split(",")[1];
  return imageData;
}

const RAW_CLIPS: ClipEntry[] = [
  clip(1, "https://github.com/tauri-apps/tauri/releases/tag/tauri-v2.10.3", 1),
  clip(2, 'fn main() {\n    let greeting = "Hello, world!";\n    println!("{greeting}");\n}', 4),
  clip(3, "[chart · 960×600]", 9, { content_type: "image", content_data: "", byte_size: 48_213 }),
  clip(4, "ana.lima@northwind.example", 16),
  clip(5, "Standup notes — ship the release on Friday, review the changelog on Thursday at 3 pm.", 23,
    { pinned: true }),
  clip(6, "#6366F1", 31),
  clip(7, "Rua Augusta 124, 1100-053 Lisboa", 44, { note: "Hotel for the conference" }),
  clip(8, '{ "name": "inspector-rust", "private": true, "version": "0.185.0" }', 58),
  clip(9, "+49 30 1234567", 75),
  clip(10, "SELECT id, email FROM users WHERE created_at > now() - interval '7 days';", 96),
  clip(11, "The quick brown fox jumps over the lazy dog", 131),
  clip(12, "38.7223, -9.1393", 180),
];
// The backend sorts pinned first, then by recency.
export const CLIPS = [...RAW_CLIPS].sort((a, b) => Number(b.pinned) - Number(a.pinned) || b.last_used_at - a.last_used_at);

export const SNIPPETS: Snippet[] = [
  { id: 1, abbreviation: "sig", title: "Email signature",
    body: "Best regards,\nAna Lima\nProduct Engineering · Northwind\n{date:%d %B %Y}", created_at: NOW, updated_at: NOW,
    category: "Writing", version: 3 },
  { id: 2, abbreviation: "addr", title: "Office address", body: "Northwind Ltd.\nRua Augusta 124\n1100-053 Lisboa",
    created_at: NOW, updated_at: NOW, category: "Writing", version: 1 },
  { id: 3, abbreviation: "aireview", title: "Code review prompt",
    body: "Review the following change for correctness, edge cases and naming. Quote the lines you mean.\n\n{clipboard}",
    created_at: NOW, updated_at: NOW, category: "AI Prompts", version: 2 },
  { id: 4, abbreviation: "mtg", title: "Meeting invite",
    body: "Hi {cursor},\n\ndoes Thursday 3 pm work for a 30-minute call about the release?", created_at: NOW,
    updated_at: NOW, category: "Writing", version: 1 },
];

const NOTES: Note[] = [];

// ── scene data ───────────────────────────────────────────────────────────
const H = 3_600_000;
const day = (d: number) => new Date(NOW + d * 86_400_000).toISOString().slice(0, 10);
const GB = 1024 ** 3;
const MORNING = (() => { const d = new Date(NOW); d.setUTCHours(10, 0, 0, 0); return d.getTime(); })();

const WEATHER = {
  location: "Lisbon, PT", lat: 38.72, lon: -9.14, tz_offset: 3600, units: "metric",
  current: { temp: 24.3, feels_like: 24.9, temp_min: 19.1, temp_max: 26.8, humidity: 58, pressure: 1017,
    wind_speed: 4.2, wind_deg: 300, description: "few clouds", icon: "02d", kind: "clouds",
    sunrise: Math.floor(MORNING / 1000) - 3 * 3600, sunset: Math.floor(MORNING / 1000) + 9 * 3600 },
  // A fixed late-morning timeline (location time = UTC+1), so hours and day/night icons agree
  // whenever the mockups are rendered.
  hourly: [0, 3, 6, 9, 12].map((h, i) => ({ dt: Math.floor((MORNING + h * H) / 1000), temp: [24, 26, 25, 21, 18][i],
    icon: ["02d", "01d", "02d", "01n", "01n"][i], kind: ["clouds", "clear-day", "clouds", "clear-night", "clear-night"][i],
    description: "", pop: [0, 0, 0.1, 0.05, 0][i] })),
  daily: [0, 1, 2, 3, 4].map((d, i) => ({ dt: Math.floor((NOW + d * 86_400_000) / 1000), date: day(d),
    temp_min: [19, 18, 17, 16, 18][i], temp_max: [27, 26, 23, 21, 24][i], icon: ["02d", "01d", "10d", "04d", "01d"][i],
    kind: ["clouds", "clear-day", "rain", "clouds", "clear-day"][i], description: "" })),
};

const STATS = {
  host_name: "northwind-mbp", os_name: "macOS 26.1", kernel: "25.1.0", cpu_arch: "arm64", uptime_secs: 3 * 86400 + 5 * 3600 + 17 * 60,
  cpu_brand: "Apple M3 Pro", cpu_usage: 23.6, cpu_freq_mhz: 4056, physical_cores: 12, logical_cores: 12,
  per_core: [41, 12, 33, 8, 57, 19, 26, 5, 14, 71, 9, 22], load_avg: [2.41, 2.08, 1.96],
  mem_total: 36 * GB, mem_used: 21.4 * GB, mem_available: 14.6 * GB, swap_total: 2 * GB, swap_used: 0.3 * GB,
  disks: [{ name: "Macintosh HD", mount: "/", fs: "apfs", total: 994 * GB, available: 412 * GB, removable: false, kind: "SSD" }],
  net_rx_per_sec: 2.4 * 1024 * 1024, net_tx_per_sec: 310 * 1024,
  temps: [{ label: "CPU", celsius: 51.5 }, { label: "GPU", celsius: 44.0 }, { label: "SSD", celsius: 36.5 }],
  fans: [{ label: "Fan", rpm: 1850 }],
  battery: { percent: 86, state: "discharging", power_watts: 9.8, time_to_empty_secs: 9 * 3600 + 40 * 60,
    time_to_full_secs: null, health_percent: 97, cycle_count: 142, temperature_c: 31.2, vendor: null, model: null },
};

const TOTP = [
  ["GitHub", "ana.lima"], ["Google", "ana.lima@northwind.example"], ["Stripe", "finance@northwind.example"],
  ["Cloudflare", "ops@northwind.example"], ["Figma", "ana.lima"], ["Notion", "ana.lima@northwind.example"],
].map(([issuer, account], i) => ({ id: i + 1, issuer, account, digits: 6, period: 30, algorithm: "SHA1", created_at: NOW }));
const CODES = ["482 913", "730 256", "104 887", "396 541", "815 062", "257 349"].map((c) => c.replace(" ", ""));

function dir(name: string, kids: [string, number][]) {
  const children = kids.map(([n, gb]) => ({ name: n, size: gb * GB, is_dir: !n.includes("."), child_count: n.includes(".") ? 0 : 3 }));
  return { name, size: children.reduce((a, c) => a + c.size, 0), is_dir: true, child_count: children.length, children };
}
function diskScan() {
  const tree = {
    name: "Projects", size: 0, is_dir: true, child_count: 6,
    children: [
      dir("video-edits", [["raw", 38], ["exports", 14], ["proxies", 6]]),
      dir("design-system", [["figma-exports", 7.5], ["icons", 1.2], ["fonts", 0.8]]),
      dir("mobile-app", [["build", 9.4], ["node_modules", 3.1], ["src", 0.4]]),
      dir("website", [["public", 2.2], ["node_modules", 1.4], ["src", 0.2]]),
      dir("datasets", [["2025", 11], ["2026", 6.5]]),
      dir("notes", [["archive", 0.6], ["drafts", 0.2]]),
    ],
  };
  tree.size = tree.children.reduce((a, c) => a + c.size, 0);
  return {
    root_path: "/Users/ana/Projects", root_name: "Projects", total: tree.size, volume_mount: "/",
    volume_total: 994 * GB, volume_free: 412 * GB, is_volume_root: false, tree,
    top_files: [["video-edits/raw/keynote-a-cam.mov", 12.4], ["video-edits/raw/keynote-b-cam.mov", 9.8],
      ["datasets/2025/events.parquet", 4.1], ["video-edits/exports/teaser-4k.mp4", 3.6]]
      .map(([path, gb]) => ({ path: `/Users/ana/Projects/${path}`, size: (gb as number) * GB })),
    items: 48_213,
  };
}

const TRANSLATIONS: Record<string, string> = {
  "wie spät ist es in lissabon?": "What time is it in Lisbon?",
  "guten morgen, wie geht es dir?": "Good morning, how are you?",
  "the meeting moves to thursday at three.": "Das Meeting wird auf Donnerstag um drei verschoben.",
};

const LIGHTS = [
  ["Living room", true, 82, true], ["Kitchen", true, 64, true], ["Desk lamp", true, 45, true],
  ["Hallway", false, 30, false], ["Bedroom", false, 20, true], ["Balcony", true, 100, true],
].map(([name, on, b, color], i) => ({ id: String(i + 1), name, on, brightness: b, reachable: true, supports_color: color, dimmable: true }));


function sameFuzzy(q: string, s: string) {
  return s.toLowerCase().includes(q.toLowerCase());
}

// ── synthetic "microphone": a 124 BPM groove for the equalizer / bpm scenes ──
// Streamed as the same `mic-audio` events the native capture sends ({rate, b64} of 16-bit mono PCM),
// so the real analysis graph runs on it: spectrum, level and beat detection are genuine, only the
// sound is made up.
const RATE = 48_000;
const CHUNK = 1024;
let micTimer: number | null = null;
let micPos = 0;
function synthChunk(): string {
  const out = new Int16Array(CHUNK);
  const beat = (60 / 124) * RATE;
  const chord = [110, 138.59, 164.81, 220, 277.18, 329.63, 440, 659.25, 880];
  for (let i = 0; i < CHUNK; i++) {
    const n = micPos + i;
    const t = n / RATE;
    const inBeat = n % beat;
    const kick = inBeat < 0.16 * RATE
      ? Math.sin(2 * Math.PI * (55 + 90 * Math.exp(-inBeat / (0.02 * RATE))) * (inBeat / RATE)) * Math.exp(-inBeat / (0.07 * RATE)) : 0;
    const off = (n + beat / 2) % beat;
    const hat = off < 0.03 * RATE ? (Math.random() * 2 - 1) * Math.exp(-off / (0.008 * RATE)) * 0.35 : 0;
    let pad = 0;
    chord.forEach((f, k) => { pad += Math.sin(2 * Math.PI * f * t + k) * (0.05 / (1 + k * 0.25)); });
    pad *= 0.7 + 0.3 * Math.sin(2 * Math.PI * 0.25 * t);
    const noise = (Math.random() * 2 - 1) * 0.04;
    out[i] = Math.max(-1, Math.min(1, kick * 0.9 + hat + pad + noise)) * 32000;
  }
  micPos += CHUNK;
  let bin = "";
  const bytes = new Uint8Array(out.buffer);
  for (let i = 0; i < bytes.length; i++) bin += String.fromCharCode(bytes[i]);
  return btoa(bin);
}
function startMic() {
  if (micTimer !== null) return;
  micTimer = window.setInterval(() => { void emit("mic-audio", { rate: RATE, b64: synthChunk() }); }, (CHUNK / RATE) * 1000);
}
function stopMic() {
  if (micTimer !== null) window.clearInterval(micTimer);
  micTimer = null;
}

export function handle(cmd: string, args: Record<string, unknown>): unknown {
  switch (cmd) {
    // ── boot ──────────────────────────────────────────────────────────────
    case "get_animation_stage": return "reduced";
    case "get_crt_animation": return { ms: 0, min: 80, max: 900, default_ms: 250 };
    case "get_theme_preference": return "dark";
    case "get_lineage_highlight": return true;
    case "get_history": return CLIPS;
    case "search_history": return CLIPS.filter((c) => sameFuzzy(String(args.query ?? ""), c.content_text));
    case "get_clip": {
      const c = CLIPS.find((x) => x.id === args.id);
      return c && c.content_type === "image" ? { ...c, content_data: demoImage() } : c ?? null;
    }
    case "list_snippets": return SNIPPETS;
    case "find_snippets": {
      const q = String(args.query ?? "").toLowerCase();
      return q ? SNIPPETS.filter((s) => s.abbreviation.startsWith(q) || sameFuzzy(q, s.title)) : [];
    }
    case "list_snippet_categories":
      return [{ id: 1, name: "AI Prompts", sort_order: 0, count: 1 }, { id: 2, name: "Writing", sort_order: 1, count: 3 }];
    case "list_notes": return NOTES;
    case "list_note_categories": return [];
    case "list_apps": return [];
    case "get_sleep_status":
      return { supported: true, sleep_disabled: false, prevented: false, indefinite: false, max_timeout_secs: null, holders: [] };
    case "plugin:app|version": return "0.185.0";
    case "app_version": return "0.185.0";
    case "plugin:window|outer_size": return { width: 1680, height: 1200 };
    case "plugin:window|scale_factor": return 2;
    case "faker_catalog": return [];
    case "faker_get_defaults": return { locale: "en", count: 1, format: "plain", pinned: [], save_history: false };
    case "sec_catalog": return { tools: [], common_wordlists: [], john_formats: [] };
    case "sec_get_defaults":
      return { wordlist: "", output_dir: "", timing: "T3", threads: 4, rate: 0, john_line: "jumbo", terminal: "terminal",
        auto_enter: false, scope_note: "", save_history: false };
    case "bruno_get_defaults":
      return { tax_class: 1, state: "BE", children: 0, is_church_member: false, health_add: 2.45, kv_type: "gkv",
        pkv_monthly: 0, kv_sick_pay: false, business_type: "freiberufler", hebesatz: 410, self_married: false };
    case "wakelock_get": return { on: false, mode: "normal" };
    case "list_timers": return [];
    case "track_status":
      return { active: false, session_id: null, paused: false, manual_paused: false, since: null, active_app: null };
    // ── scenes ────────────────────────────────────────────────────────────
    case "mic_capture_start": startMic(); return null;
    case "mic_capture_stop": stopMic(); return null;
    case "x_overlay_payload": return { mode: "features", headlines: [] };
    case "get_boom_config": return null;
    case "weather_fetch": return WEATHER;
    case "get_weather_config": return { has_key: true, units: "metric" };
    case "get_system_stats": return STATS;
    case "totp_list": return TOTP;
    case "totp_current_codes_all": {
      const left = 30 - (Math.floor(Date.now() / 1000) % 30);
      return TOTP.map((t, i) => ({ id: t.id, code: CODES[i], seconds_remaining: left }));
    }
    case "disk_scan": return diskScan();
    case "translate_text": {
      const t = TRANSLATIONS[String(args.text ?? "").trim().toLowerCase()];
      return { text: t ?? String(args.text ?? ""), detected_source: String(args.source ?? "de"), provider: "google" };
    }
    case "get_clock_zones": return JSON.stringify(["Europe/Lisbon", "America/New_York", "Asia/Tokyo", "Australia/Sydney"]);
    case "hue_status": return { connected: true, bridge_ip: "192.168.1.20", paired: true };
    case "hue_list_lights": return LIGHTS;
    default:
      if (!misses.has(cmd)) {
        misses.add(cmd);
        console.info("[mock] unhandled", cmd, JSON.stringify(args).slice(0, 160));
      }
      return null;
  }
}

(window as unknown as { __mockMisses: Set<string> }).__mockMisses = misses;
