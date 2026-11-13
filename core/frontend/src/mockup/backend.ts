// Dummy backend for the mockup harness (mockup.html). Every answer is invented; nothing here reads
// the machine. Commands without a handler answer `null` and are recorded in `window.__mockMisses`,
// so a new panel that needs data shows up in the harness log instead of rendering silently empty.
import { emit } from "@tauri-apps/api/event";
import type { ClipEntry, Note, Snippet } from "../lib/types";

const misses = new Set<string>();
/** The editor key map's stored deviations (record.shortcuts). */
let mockShortcuts: object = {};
/** Invented style presets of the mockup recordings folder (stage 4), one of them damaged. */
let mockPresets: { name: string; file: string; ok: boolean; error: string | null }[] = [
  { name: "Northwind launch", file: "Northwind launch.irpreset", ok: true, error: null },
  { name: "Phone portrait", file: "Phone portrait.irpreset", ok: true, error: null },
  { name: "Plain dark", file: "Plain dark.irpreset", ok: true, error: null },
  { name: "Old export", file: "Old export.irpreset", ok: false, error: "record.editor.preset_version" },
];
let mockDefaultPreset: string | null = "Northwind launch.irpreset";
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
  disks: [
    { name: "Macintosh HD", mount: "/", fs: "apfs", total: 994 * GB, available: 412 * GB, removable: false, kind: "SSD" },
    { name: "Macintosh HD", mount: "/System/Volumes/Data", fs: "apfs", total: 994 * GB, available: 412 * GB, removable: false, kind: "SSD" },
    { name: "Archive", mount: "/Volumes/Archive", fs: "exfat", total: 1862 * GB, available: 640 * GB, removable: true, kind: "HDD" },
  ],
  net_rx_per_sec: 2.4 * 1024 * 1024, net_tx_per_sec: 310 * 1024,
  temps: [
    { label: "PMU tdie1", celsius: 51.5 }, { label: "PMU tdie2", celsius: 53.1 }, { label: "PMU tdie3", celsius: 49.8 },
    { label: "PMU TP1g", celsius: 44.0 }, { label: "PMU TP2g", celsius: 45.2 },
    { label: "gas gauge battery", celsius: 31.2 }, { label: "NAND CH0 temp", celsius: 36.5 }, { label: "PMU tcal", celsius: 41.0 },
  ],
  fans: [{ label: "Fan 1", rpm: 1850 }, { label: "Fan 2", rpm: 1920 }],
  battery: { percent: 86, state: "Discharging", power_watts: 9.8, time_to_empty_secs: 9 * 3600 + 40 * 60,
    time_to_full_secs: null, health_percent: 97, cycle_count: 142, temperature_c: 31.2, vendor: null, model: "bq40z651" },
  cpu_user: 16.9, cpu_system: 6.7, perf_cores: 6, eff_cores: 6,
  memory: { pressure: 0, wired: 3.1 * GB, active: 12.6 * GB, inactive: 9.8 * GB, compressed: 4.2 * GB, free: 6.3 * GB, swapout_bps: 0, swapin_bps: 0 },
  disk_io: { read_bps: 1.8 * 1024 * 1024, write_bps: 640 * 1024 },
  net_ifaces: [
    { name: "en0", rx_per_sec: 2.4 * 1024 * 1024, tx_per_sec: 300 * 1024, addr: "192.168.1.42" },
    { name: "utun4", rx_per_sec: 0, tx_per_sec: 10 * 1024, addr: "100.64.0.7" },
  ],
};
const TOP_PROCS = {
  cpu_ready: true,
  by_cpu: [
    { name: "Xcode", cpu_pct: 142.3, mem: 3.4 * GB, procs: 6 }, { name: "Safari", cpu_pct: 38.6, mem: 2.9 * GB, procs: 14 },
    { name: "Figma", cpu_pct: 21.4, mem: 1.8 * GB, procs: 5 }, { name: "Docker", cpu_pct: 9.2, mem: 4.1 * GB, procs: 4 },
    { name: "Spotify", cpu_pct: 3.1, mem: 0.4 * GB, procs: 6 },
  ],
  by_mem: [
    { name: "Docker", cpu_pct: 9.2, mem: 4.1 * GB, procs: 4 }, { name: "Xcode", cpu_pct: 142.3, mem: 3.4 * GB, procs: 6 },
    { name: "Safari", cpu_pct: 38.6, mem: 2.9 * GB, procs: 14 }, { name: "Figma", cpu_pct: 21.4, mem: 1.8 * GB, procs: 5 },
    { name: "Slack", cpu_pct: 0.4, mem: 0.9 * GB, procs: 7 },
  ],
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


// ── currency rates (EUR value of one unit), loc report, shazam history ──
const FX = {
  eur_per: { EUR: 1, USD: 1 / 1.1712, GBP: 1 / 0.8431, CHF: 1 / 0.9368, JPY: 1 / 173.2, SEK: 1 / 11.02,
    PLN: 1 / 4.263, CZK: 1 / 24.31, BTC: 73_319, ETH: 3_842 },
  ecb_date: "2026-09-23", ecb_fetched_ms: NOW, crypto_fetched_ms: NOW, stale: false, error: null,
};

const LOC_LANGS: [string, number, number, number, number][] = [
  ["TypeScript", 212, 48_310, 6_120, 7_004], ["Rust", 64, 21_870, 4_310, 3_210],
  ["CSS", 18, 3_940, 220, 610], ["JSON", 22, 1_880, 0, 12], ["Markdown", 15, 0, 2_910, 830],
  ["Shell", 9, 640, 180, 150],
];
const LOC = (() => {
  const code = LOC_LANGS.reduce((a, l) => a + l[2], 0);
  const languages = LOC_LANGS.map(([name, files, c, comments, blanks]) =>
    ({ name, files, code: c, comments, blanks, code_pct: (c / code) * 100 }));
  const sum = (k: "files" | "comments" | "blanks") => languages.reduce((a, l) => a + l[k], 0);
  return {
    root_label: "northwind-app", paths: ["/Users/ana/Projects/northwind-app"], respected_ignores: true,
    languages, total_files: sum("files"), total_code: code, total_comments: sum("comments"),
    total_blanks: sum("blanks"), total_lines: code + sum("comments") + sum("blanks"), inaccurate: false,
    subdirs: ["src", "server", "docs", "scripts"],
  };
})();

// Made-up tracks; covers are generated gradients so nothing loads from the network.
function cover(a: string, b: string) {
  const svg = `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 64 64"><defs><linearGradient id="g" x1="0" y1="0" x2="1" y2="1"><stop offset="0" stop-color="${a}"/><stop offset="1" stop-color="${b}"/></linearGradient></defs><rect width="64" height="64" fill="url(#g)"/><circle cx="32" cy="32" r="13" fill="none" stroke="#fff" stroke-opacity=".55" stroke-width="3"/></svg>`;
  return `data:image/svg+xml;base64,${btoa(svg)}`;
}
const SONGS = [
  ["Harbour Lights", "The Paper Kites Club", "Night Ferry", "Indie", "2025", "#f59e0b", "#ef4444", 12],
  ["Glass Parade", "Mira Solen", "Afterglow", "Electronic", "2026", "#6366f1", "#ec4899", 95],
  ["Slow Tide", "Northern Lanes", "Coastlines", "Alternative", "2024", "#0ea5e9", "#22c55e", 260],
  ["Neon Botanica", "Kasimir & The Vines", "Greenhouse", "Pop", "2026", "#a855f7", "#14b8a6", 1440],
  ["Carousel", "Lua Almeida", "Fado Nova", "Fado", "2025", "#f43f5e", "#f97316", 3100],
].map(([title, artist, album, genre, released, a, b, mins], i) => ({
  id: i + 1, recognized_at: NOW - (mins as number) * 60_000,
  title, artist, album, genre, released, cover_url: cover(a as string, b as string),
  shazam_url: "https://example.com", spotify_url: "https://example.com", youtube_url: "https://example.com",
}));

// ── second gallery batch ──
const MONITORS = [
  { id: 1, name: "Built-in Retina Display", brightness: 72, supports_ddc: true, edr_max: 160 },
  { id: 2, name: "Studio Display", brightness: 55, supports_ddc: true, edr_max: 100 },
];
const AUDIO = [
  { id: "71", name: "MacBook Pro Speakers", is_default: false },
  { id: "84", name: "AirPods Pro", is_default: true },
  { id: "92", name: "Studio Display Speakers", is_default: false },
  { id: "105", name: "USB Audio Interface", is_default: false },
];
const ALIASES = [
  ["gs", "git status -sb"], ["gl", "git log --oneline --graph -20"], ["ll", "ls -lah"],
  ["serve", "python3 -m http.server 8000"], ["dps", "docker ps --format 'table {{.Names}}\\t{{.Status}}'"],
  ["k", "kubectl"], ["weather", "curl wttr.in/Lisbon"],
].map(([name, command], i) => ({ name, command, file: i < 5 ? "~/.zshrc" : "~/.zsh/aliases.zsh", primary: i < 5 }));
const IP = { ip: "203.0.113.42", city: "Lisbon", region: "Lisbon", country: "Portugal", country_code: "PT",
  postal: "1100-148", latitude: 38.7223, longitude: -9.1393, timezone: "Europe/Lisbon",
  organization: "Northwind Fibre", asn: "AS64500" };
const SNITCH_APPS = [
  ["Safari", 14, ["198.51.100.10", "203.0.113.5"]], ["Slack", 6, ["198.51.100.44"]], ["Spotify", 4, ["203.0.113.77"]],
  ["Figma", 3, ["198.51.100.91"]], ["Mail", 2, ["203.0.113.18"]], ["Dropbox", 2, ["198.51.100.120"]],
  ["Tracker Helper", 1, ["203.0.113.200"]],
].map(([command, n, remotes], i) => ({ key: String(command), command, pids: [4100 + i], connection_count: n, remotes, blocked: i === 6 }));
const SNITCH_GEO = [
  ["198.51.100.10", 37.39, -122.08, "United States", "Mountain View", "Example Cloud"],
  ["203.0.113.5", 53.35, -6.26, "Ireland", "Dublin", "Example CDN"],
  ["198.51.100.44", 39.04, -77.49, "United States", "Ashburn", "Example Cloud"],
  ["203.0.113.77", 59.33, 18.07, "Sweden", "Stockholm", "Example Music"],
  ["198.51.100.91", 50.11, 8.68, "Germany", "Frankfurt", "Example Cloud"],
  ["203.0.113.18", 35.68, 139.69, "Japan", "Tokyo", "Example Mail"],
  ["198.51.100.120", -33.87, 151.21, "Australia", "Sydney", "Example Storage"],
  ["203.0.113.200", 1.35, 103.82, "Singapore", "Singapore", "Example Ads"],
].map(([ip, lat, lon, country, city, isp]) => ({ ip, lat, lon, country, city, isp }));
const SNITCH_CONN = SNITCH_APPS.flatMap((a) => (a.remotes as string[]).map((ip) =>
  ({ pid: a.pids[0], command: a.command, proto: "TCP", remote_ip: ip, remote_port: 443, v6: false })));
const TOKENS = {
  overview: { total_tokens: 48_210_000, input_tokens: 1_240_000, output_tokens: 610_000, cache_read_tokens: 42_900_000,
    cache_create_tokens: 3_460_000, estimated_cost: 38.72, input_cost: 3.72, output_cost: 9.15, cache_read_cost: 12.87,
    cache_create_cost: 12.98, sessions: 14, total_active_min: 312, avg_active_min_per_day: 312, active_days: 1,
    messages: 1_284, rate_limit_hits: 0, lines_added: 4_812, lines_removed: 1_977, lines_written: 6_789,
    period_from: "2026-09-24", period_to: "2026-09-24" },
  models: [["claude-opus-5-5", "Opus 5.5", 31_400_000, 29.1], ["claude-sonnet-5-5", "Sonnet 5.5", 14_100_000, 8.4],
    ["claude-haiku-4-5", "Haiku 4.5", 2_710_000, 1.22]].map(([model, label, t, cost]) => ({ model, label,
    total_tokens: t, input_tokens: (t as number) * 0.03, output_tokens: (t as number) * 0.012, cache_read_tokens: (t as number) * 0.88,
    cache_create_tokens: (t as number) * 0.07, cost, messages: Math.round((t as number) / 38_000) })),
  projects: [["northwind-app", 21_300_000, 17.4, 6], ["design-system", 12_800_000, 10.1, 3], ["website", 8_100_000, 6.3, 3],
    ["data-pipeline", 6_010_000, 4.92, 2]].map(([name, t, cost, sess]) => ({ name, total_tokens: t, input_tokens: 0,
    output_tokens: 0, cache_read_tokens: 0, cache_create_tokens: 0, cost, messages: Math.round((t as number) / 38_000),
    sessions: sess, lines_added: 1200, lines_removed: 480, lines_written: 1680, first_ts: null, last_ts: null })),
  sessions: [], from: "2026-09-24", to: "2026-09-24", prior_day: null, sessions_loaded: false,
};
const CLEAN = {
  dirs: [
    ["~/Library/Caches/com.google.Chrome", 2.84 * GB, 18_214, "browser"],
    ["~/Library/Caches/com.apple.Safari", 0.61 * GB, 4_110, "browser"],
    ["~/Library/Caches/Homebrew", 3.72 * GB, 312, "other_caches"],
    ["~/Library/Caches/pip", 1.18 * GB, 9_840, "other_caches"],
    ["~/Library/Logs/DiagnosticReports", 0.21 * GB, 188, "logs"],
    ["~/Library/Developer/Xcode/DerivedData", 7.9 * GB, 61_022, "xcode_caches"],
    ["~/Projects/old-prototype/node_modules", 0.94 * GB, 40_311, "stale_node_modules"],
    ["~/Downloads (old installers)", 2.3 * GB, 6, "installers"],
  ].map(([path, size, count, category]) => ({ path, size, count, category })),
  total_bytes: 0,
  categories: [["browser", "Browser caches", 0], ["other_caches", "Other app caches", 0], ["logs", "Logs", 0],
    ["xcode_caches", "Xcode caches", 0], ["stale_node_modules", "Stale node_modules", 0], ["installers", "Old installers", 0]],
};
CLEAN.total_bytes = CLEAN.dirs.reduce((a, d) => a + (d.size as number), 0);
for (const c of CLEAN.categories) c[2] = CLEAN.dirs.filter((d) => d.category === c[0]).reduce((a, d) => a + (d.size as number), 0);
/** An invented Logitech mouse for the `logi` panel. */
const LOGI: { status: import("../lib/logi").LogiStatus; config: import("../lib/logi").LogiConfig } = {
  status: {
    supported: true,
    connected: true,
    permission_denied: false,
    name: "MX Master",
    battery: { percent: 80, charging: false },
    dpi: { current: 1000, default: 1000, options: [400, 600, 800, 1000, 1200, 1400, 1600] },
    wheel: { mode: "smart", threshold: 16 },
    invert: false,
    controls: [0x50, 0x51].map((cid) => ({ cid, divertable: false })).concat([0x52, 0x53, 0x56, 0xc3, 0xc4].map((cid) => ({ cid, divertable: true }))),
    options_running: false,
  },
  config: {
    dpi: 1000,
    wheel: null,
    invert: null,
    buttons: { "195": { up: { type: "volume_up" }, down: { type: "volume_down" }, press: { type: "mute_toggle" } } },
  },
};

const BT = [
  ["AirPods Pro", "a4-c3-f0-11-22-33", true, "Headphones"], ["Magic Keyboard", "3c-a6-f6-44-55-66", true, "Keyboard"],
  ["MX Master 3S", "d4-9c-dd-77-88-99", true, "Device"], ["Bose SoundLink", "04-52-c7-aa-bb-cc", false, "Speaker"],
  ["Pixel 9", "f8-0f-f9-dd-ee-ff", false, "Phone"],
].map(([name, address, connected, kind]) => ({ name, address, connected, kind }));
const PROCS = [
  ["Google Chrome", 2_840], ["Figma", 1_620], ["Slack", 910], ["Docker", 1_380], ["Code", 1_150], ["Spotify", 420],
  ["Mail", 310], ["Finder", 140], ["Notion", 680], ["Terminal", 95],
].map(([name, mb], i) => ({ pid: 3100 + i * 37, name, memory_mb: mb, exe: `/Applications/${name}.app` }));
const SOCIAL = { url: "", title: "Building a clipboard manager in Rust — full walkthrough",
  uploader: "Northwind Dev", duration_s: 1_847, thumbnail: cover("#6366f1", "#0ea5e9"),
  description: "From the global hotkey to encrypted history: a tour through a Tauri app, with the parts that took longest to get right." };
const MACHINE = { os_name: "macOS", os_version: "26.1", kernel: "25.1.0", arch: "arm64", device_model: "MacBook Pro",
  host_name: "northwind-mbp", cpu_brand: "Apple M3 Pro", physical_cores: 12, logical_cores: 12, mem_total_bytes: 36 * GB };
const WL = [["sort", "Integer-Sortierung", "Melem/s"], ["sha256", "SHA-256", "MB/s"], ["matmul", "Matrixmultiplikation", "MFLOP/s"],
  ["nbody", "N-Körper-Simulation", "Mpaare/s"], ["sieve", "Primzahlsieb", "Mkand/s"], ["deflate", "Deflate-Kompression", "MB/s"],
  ["text", "Textauswertung", "MB/s"]];
function benchRunData(id: string, k: number, ago: number) {
  const sec = (mult: number, base: number[]) => {
    const workloads = WL.map(([wid, name, unit], i) => ({ id: wid, name, unit, rate: base[i] * mult * k,
      score: Math.round(1000 * mult * k * (0.9 + i * 0.04)), iterations: 100, seconds: 0.6 }));
    const geo = Math.exp(workloads.reduce((a, w) => a + Math.log(w.score), 0) / workloads.length);
    return { score: Math.round(geo), workloads };
  };
  const base = [82, 241, 5200, 38, 1250, 96, 410];
  return { schema: 1, id, finished_at_ms: NOW - ago, duration_s: 9.4, app_version: "0.188.0", baseline_machine: "Apple M3 Pro",
    machine: MACHINE, threads: 12, single: sec(1, base), multi: sec(8.6, base) };
}
const BENCH_HISTORY = [benchRunData("r2", 1, 86_400_000 * 3), benchRunData("r1", 0.93, 86_400_000 * 20)];
const psRun = (strategy: string, perf: number) => ({ strategy, final_url: "https://northwind.example/",
  fetch_time: "2026-09-24T09:41:00Z", lighthouse_version: "12.8.0",
  categories: [["performance", "Performance", perf], ["accessibility", "Accessibility", 0.97],
    ["best-practices", "Best Practices", 1], ["seo", "SEO", 0.92]]
    .map(([id, label, score]) => ({ id, label, score: Math.round((score as number) * 100) })),
  metrics: [["first-contentful-paint", "First Contentful Paint", strategy === "mobile" ? "1.8 s" : "0.5 s", 0.9],
    ["largest-contentful-paint", "Largest Contentful Paint", strategy === "mobile" ? "2.9 s" : "0.8 s", perf],
    ["total-blocking-time", "Total Blocking Time", strategy === "mobile" ? "120 ms" : "20 ms", 0.95],
    ["cumulative-layout-shift", "Cumulative Layout Shift", "0.02", 1],
    ["speed-index", "Speed Index", strategy === "mobile" ? "2.4 s" : "0.9 s", 0.88]]
    .map(([id, label, display, score]) => ({ id, label, display, score: Math.round((score as number) * 100) })) });
const PAGESPEED = { url: "https://northwind.example", desktop: psRun("desktop", 0.99), mobile: psRun("mobile", 0.84), errors: [] };

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
let lastDbfs = -60;
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
  let sq = 0;
  for (let i = 0; i < CHUNK; i++) sq += (out[i] / 32768) ** 2;
  lastDbfs = 20 * Math.log10(Math.max(1e-9, Math.sqrt(sq / CHUNK)));
  let bin = "";
  const bytes = new Uint8Array(out.buffer);
  for (let i = 0; i < bytes.length; i++) bin += String.fromCharCode(bytes[i]);
  return btoa(bin);
}
function startMic() {
  if (micTimer !== null) return;
  micTimer = window.setInterval(() => {
    void emit("mic-audio", { rate: RATE, b64: synthChunk() });
    // The dezibel meter listens to the Rust-computed level instead of the PCM.
    void emit("mic-level", { dbfs: lastDbfs });
  }, (CHUNK / RATE) * 1000);
}
function stopMic() {
  if (micTimer !== null) window.clearInterval(micTimer);
  micTimer = null;
}

const SPEED_DAY = 86_400_000;
const SPEED_BASE = Date.UTC(2026, 8, 30, 8, 15);
/** Invented speed-test history, newest first. */

// ── pulse (invented: a 16-GB Mac under memory pressure) ─────────────────────
const PULSE_NOW = Math.floor(Date.UTC(2026, 9, 6, 14, 0) / 1000);
function pulseSample(i: number) {
  const wave = Math.sin(i / 9) * 0.5 + 0.5;
  const pressure = wave > 0.72 ? 4 : wave > 0.45 ? 2 : 1;
  const ssd = (4 + wave * 26) * 1e6;
  return {
    ts: PULSE_NOW - (179 - i) * 5,
    dur: 5,
    pressure,
    swap_used: (17.5 + wave * 3.2) * 1e9,
    swap_total: 22.5e9,
    swapout_bps: wave * 9e6,
    swapin_bps: wave * 3e6,
    pageout_bps: 0,
    compress_ps: 400 + wave * 2600,
    decompress_ps: 300 + wave * 1800,
    ssd_write_bps: ssd,
    ssd_read_bps: ssd * 0.6,
    swap_write_bps: Math.min(ssd, wave * 9e6),
    cpu_pct: 25 + wave * 40,
    thermal: 0,
    wired: 3.1e9,
    active: 6.8e9,
    inactive: 3.9e9,
    free: 0.15e9,
    compressor: 5.9e9,
  };
}
const PULSE_RECENT = Array.from({ length: 180 }, (_, i) => pulseSample(i));
const PULSE_GROUPS = [
  { name: "Gradle-Daemon", footprint: 5.8e9, write_bps: 2.4e6, procs: 2 },
  { name: "Google Chrome", footprint: 4.1e9, write_bps: 0.6e6, procs: 38 },
  { name: "Docker", footprint: 3.2e9, write_bps: 1.1e6, procs: 6 },
  { name: "Kotlin-Daemon", footprint: 1.9e9, write_bps: 0.3e6, procs: 1 },
  { name: "Android Studio", footprint: 1.6e9, write_bps: 0.2e6, procs: 4 },
];
function pulseAgg(ts: number, secs: number, i: number) {
  const w = Math.sin(i / 3) * 0.5 + 0.5;
  const crit = secs * Math.max(0, w - 0.45);
  const warn = secs * 0.2 * w;
  return {
    ts, secs,
    p_normal: secs - crit - warn, p_warn: warn, p_crit: crit,
    swap_min: 15e9, swap_avg: (16 + w * 4) * 1e9, swap_max: 21e9, swap_total: 22.5e9,
    written: secs * (6 + w * 20) * 1e6, read: secs * 8e6,
    swap_written: secs * w * 7e6, swapped_in: secs * w * 2e6,
    compressions: secs * 1500, cpu_avg: 30 + w * 30,
    causers: [
      { name: "Gradle-Daemon", written: secs * 2e6, peak_footprint: 6.1e9 },
      { name: "Google Chrome", written: secs * 0.5e6, peak_footprint: 4.4e9 },
      { name: "Docker", written: secs * 1e6, peak_footprint: 3.3e9 },
    ],
  };
}
function pulseHistoryMock(range: string) {
  const plan: Record<string, [number, number]> = {
    "1h": [60, 60], "24h": [900, 96], "7d": [3600, 168], "30d": [86400, 30], "90d": [86400, 90],
  };
  const [bucket, n] = plan[range] ?? plan["24h"];
  const buckets = Array.from({ length: n }, (_, i) => pulseAgg(PULSE_NOW - (n - i) * bucket, bucket, i));
  const sum = (k: "written" | "swap_written" | "p_crit" | "p_warn" | "p_normal" | "secs") =>
    buckets.reduce((a, b) => a + b[k], 0);
  const total = { ...pulseAgg(PULSE_NOW - n * bucket, 0, 0), written: sum("written"), swap_written: sum("swap_written"),
    p_crit: sum("p_crit"), p_warn: sum("p_warn"), p_normal: sum("p_normal"), secs: sum("secs"),
    causers: pulseAgg(0, sum("secs"), 2).causers };
  const days = Array.from({ length: 8 }, (_, i) => pulseAgg(PULSE_NOW - (i + 1) * 86400, 86400, i * 2));
  return {
    range, bucket_secs: bucket, buckets, total, days,
    forecast: { tbw_bytes: 300e12, per_day: 62e9, days: 30, lifetime_written: 41e12, years: 11.4 },
  };
}

// GitHub analytics — an invented account (nothing real), 60 days of history.
const GH_NOW = Date.now();
const GH_DAY = 86_400_000;
const ghSeries = (start: number, perDay: number, days = 60): [number, number][] =>
  Array.from({ length: days }, (_, i) => [GH_NOW - (days - 1 - i) * GH_DAY, Math.round(start + perDay * i + Math.sin(i / 3) * 2)]);
const GH_FOLLOWERS = ghSeries(118, 0.42);
const GH_STARS = ghSeries(301, 1.4);
const GH_REPO = (id: number, name: string, stars: number, forks: number, lang: string, extra: Record<string, unknown> = {}) => ({
  id, name, full_name: `northwind-ana/${name}`, private: false, archived: false, fork: false, language: lang,
  html_url: "", description: null, license: "MIT", stars, forks, watchers: null, open_issues: id % 4,
  updated_at_ms: GH_NOW - id * 3 * GH_DAY, pushed_at_ms: GH_NOW - [2, 19, 47, 96, 158, 290][(id - 1) % 6] * GH_DAY,
  created_at_ms: GH_NOW - [1420, 980, 760, 610, 2100, 2400][(id - 1) % 6] * GH_DAY,
  size_kb: [18_400, 6_200, 3_900, 1_200, 90, 640][(id - 1) % 6],
  topics: [["rust", "cli", "devtools"], ["typescript", "data"], ["ui", "design-system"], ["go"], [], ["blog"]][(id - 1) % 6],
  homepage: null, default_branch: "main", ...extra,
});
const GH_ROWS = [
  { ...GH_REPO(1, "harbor-cli", 214, 31, "Rust"), growth_7d: 9, growth_30d: 38 },
  { ...GH_REPO(2, "tidepool", 96, 12, "TypeScript"), growth_7d: 4, growth_30d: 17 },
  { ...GH_REPO(3, "lantern-ui", 41, 6, "TypeScript"), growth_7d: 1, growth_30d: 5 },
  { ...GH_REPO(4, "saltmarsh", 22, 3, "Go"), growth_7d: 0, growth_30d: 2 },
  { ...GH_REPO(5, "dotfiles", 8, 1, "Shell", { private: true }), growth_7d: null, growth_30d: null },
  { ...GH_REPO(6, "old-blog", 3, 0, "HTML", { archived: true }), growth_7d: 0, growth_30d: 0 },
];
const ghDeltas = (s: [number, number][]) => ({
  d24h: s[59][1] - s[58][1], d7d: s[59][1] - s[52][1], d30d: s[59][1] - s[29][1], d90d: null, d1y: null,
});
/** Reconstructed stars: a slow start, then a launch bump ~14 months ago (invented). */
const GH_STAR_HISTORY: [number, number][] = (() => {
  const out: [number, number][] = [];
  let v = 0;
  for (let d = 1420; d >= 60; d -= 10) {
    v += d > 440 ? 0.6 : d > 400 ? 9 : 2.1;
    out.push([GH_NOW - d * GH_DAY, Math.round(v)]);
  }
  return [...out, ...GH_STARS];
})();
const GH_MONTHS: [number, number | null][] = Array.from({ length: 12 }, (_, i) => {
  const d = new Date(GH_NOW);
  return [Date.UTC(d.getUTCFullYear(), d.getUTCMonth() - 11 + i, 1), [9, 7, 12, 31, 18, 14, 22, 16, 11, 19, 38, 21][i]];
});
function githubDashboardMock(range: string) {
  const days = { "7d": 7, "30d": 30, "90d": 90, "1y": 365, "5y": 1825 }[range] ?? 9999;
  const cut = (s: [number, number][]) => s.filter(([t]) => t >= GH_NOW - days * GH_DAY);
  return {
    user_id: 1,
    profile: {
      login: "northwind-ana", name: "Ana Lima", avatar_url: null, html_url: "https://github.com/northwind-ana",
      created_at_ms: GH_NOW - 4615 * GH_DAY, bio: "Tools for small harbours and big data.", company: "Northwind",
      location: "Lisbon", blog: "northwind.example", hireable: null, public_gists: 6, public_repos: 5,
      private_repos: 1, followers: GH_FOLLOWERS[59][1], following: 64,
    },
    forks: cut(GH_STAR_HISTORY.map(([t, v]) => [t, Math.round(v / 7)] as [number, number])),
    repos: cut([[GH_NOW - 2400 * GH_DAY, 1], [GH_NOW - 2100 * GH_DAY, 2], [GH_NOW - 1420 * GH_DAY, 3], [GH_NOW - 980 * GH_DAY, 4], [GH_NOW - 760 * GH_DAY, 5], [GH_NOW - 610 * GH_DAY, 6], [GH_NOW, 6]]),
    reconstructed_from_ms: GH_STAR_HISTORY[0][0],
    star_gains: GH_MONTHS,
    backfill: { done: 9, total: 9, truncated: 0, stars_complete: true },
    kpis: [
      { key: "followers", value: GH_FOLLOWERS[59][1], deltas: ghDeltas(GH_FOLLOWERS) },
      { key: "stars", value: GH_STARS[59][1], deltas: ghDeltas(GH_STARS) },
      { key: "repos", value: GH_ROWS.length, deltas: { d24h: 0, d7d: 0, d30d: 1, d90d: null } },
      { key: "forks", value: 53, deltas: { d24h: 0, d7d: 2, d30d: 6, d90d: null } },
    ],
    followers: cut(GH_FOLLOWERS),
    stars: cut(GH_STAR_HISTORY),
    rows: GH_ROWS,
    growth: GH_ROWS.slice(0, 5).map((r, i) => ({ id: r.id, name: r.name, points: cut(ghSeries(r.stars - 30 / (i + 1), 0.5 / (i + 1))) })),
    history_since_ms: GH_FOLLOWERS[0][0],
    snapshot_count: 60,
    private_count: 1,
  };
}

const SP_NOW = Date.UTC(2026, 9, 11, 18, 40);
const SPOTIFY_ROWS = (
  [
    [0, "Wicked Games", "Parra for Cuva", "Paradigm", 262_000, 140_000, "playing"],
    [5, "Ocean Drive", "Khruangbin", "Mordechai", 238_000, 238_000, "listened"],
    [9, "Bad Kingdom", "Moderat", "II", 291_000, 61_000, "partial"],
    [10, "Computerliebe", "Kraftwerk", "Computerwelt", 409_000, 9_000, "skipped"],
    [17, "Rising Tide", "Parra for Cuva", "Paradigm", 214_000, 214_000, "listened"],
    [21, "Maria también", "Khruangbin", "Con Todo El Mundo", 196_000, 112_000, "partial"],
    [25, "A New Error", "Moderat", "Moderat", 365_000, 365_000, "listened"],
  ] as const
).map(([min, track, artist, album, dur, played, status], i) => ({
  id: 100 - i,
  started_at: SP_NOW - (min as number) * 60_000 * 4,
  ended_at: SP_NOW - (min as number) * 60_000 * 4 + (played as number),
  track_uri: `spotify:track:mock${i}`,
  track,
  artist,
  album,
  duration_ms: dur,
  played_ms: played,
  status,
  artwork_url: null,
  open: false,
}));
const SPOTIFY_HISTORY = {
  rows: SPOTIFY_ROWS,
  total: SPOTIFY_ROWS.length,
  played_ms: SPOTIFY_ROWS.reduce((a, r) => a + r.played_ms, 0),
};

const SPEED_RUNS = [
  [0, 388, 20.4, 14.0, 3.6, 31, 11],
  [0.3, 362, 19.1, 15.2, 4.1, 44, 18],
  [1.1, 401, 21.0, 13.1, 2.9, 29, 12],
  [2.2, 214, 17.8, 22.6, 9.8, 96, 35],
  [3.4, 379, 20.2, 14.4, 3.2, 33, 14],
  [5.0, 356, null, 16.0, 4.4, 40, null],
  [6.3, 392, 20.8, 13.6, 3.0, 30, 12],
].map(([d, down, up, ping, jit, ld, lu], i) => ({
  id: 7 - i,
  at: SPEED_BASE - (d as number) * SPEED_DAY,
  download_bps: (down as number) * 1e6,
  upload_bps: up == null ? null : (up as number) * 1e6,
  latency_ms: ping,
  jitter_ms: jit,
  loaded_down_ms: ld,
  loaded_up_ms: lu,
  colo: "LIS",
  country: "PT",
  ip: null,
}));

// `?lang=de` renders the mockup in German; Settings → Language switches live.
let mockLang: "system" | "en" | "de" =
  typeof location !== "undefined" && new URLSearchParams(location.search).get("lang") === "de" ? "de" : "system";
const mockLanguage = () => ({
  setting: mockLang,
  effective: mockLang === "de" ? "de" : "en",
  system: "en",
});


// ── record editor (stage 2) — invented 24-s take, spans computed here only for the mockup ──
type EdDoc = { clips: { startMs: number; endMs: number }[]; speed: { clip: number; startMs: number; endMs: number; speed: number }[] } & Record<string, unknown>;
/** The running mockup voiceover take (`editor_voiceover_start`). */
let edVoiceover: { programmeMs: number; file: string } | null = null;
/** Inserted takes of the mockup project (filled by `editor_import_take`). */
const edTakes: Record<string, unknown> = {};
let edDoc: EdDoc = {
  version: 5,
  clips: [{ startMs: 0, endMs: 9_000 }, { startMs: 11_000, endMs: 24_000 }],
  speed: [{ clip: 1, startMs: 14_000, endMs: 20_000, speed: 3 }],
  crop: [null, null],
  output: { aspect: "auto" },
  micGainDb: -3,
  appearance: {
    background: { kind: "wallpaper", id: "01" }, backgroundMotion: "none", padding: 50, radiusPx: 40,
    shadow: 0.6, backgroundBlur: 0, motionBlur: 0.2, frame: "none", frameTheme: "light", depthOfField: true,
  },
  cursor: {
    show: true, autoHide: false, theme: "default", custom: null, size: 3.6, smoothing: 0.3, motionBlur: 0.35,
    clickBounce: 1, clickImpact: false, clickSound: false, clickSoundGainDb: 0, model3d: false,
  },
  // Stage 7: one invented zoom with the screen turned left (3D camera).
  zooms: [
    {
      id: "zoom-tilt", clip: 0, startMs: 2_000, endMs: 6_500, depth: 3, customScale: null, focusX: 0.5, focusY: 0.5,
      focusMode: "auto", origin: "manual", hideCursor: false, rotation: "left",
    },
  ],
  autoZoomDone: true,
  // Stage 7: one invented vignette over part of the second clip.
  effects: [{ id: "fx-vignette", clip: 1, startMs: 11_500, endMs: 13_800, effect: "vignette", values: { strength: 0.5 } }],
  // Invented annotations, one of each kind (stage 4): a title, an arrow, a blur over an
  // "e-mail address", and an image waiting for its file.
  annotations: [
    {
      id: "ann-title", clip: 0, startMs: 1_000, endMs: 5_500, x: 8, y: 8, w: 44, h: 12, zIndex: 1,
      body: {
        kind: "text", content: "Welcome to Northwind", color: "#ffffff", background: "rgba(0, 0, 0, 0.7)", fontSize: 48,
        fontFamily: "Inter", bold: true, italic: false, underline: false, align: "center", animation: "rise",
      },
    },
    { id: "ann-arrow", clip: 0, startMs: 3_000, endMs: 7_000, x: 60, y: 40, w: 14, h: 14, zIndex: 2, body: { kind: "arrow", direction: "down-left", color: "#34B27B", strokeWidth: 6 } },
    {
      id: "ann-blur", clip: 1, startMs: 12_000, endMs: 18_000, x: 55, y: 18, w: 30, h: 10, zIndex: 3,
      body: {
        kind: "blur", style: "mosaic", shape: "freehand", color: "white", intensity: 12, blockSize: 12,
        points: [[10, 30], [25, 10], [55, 8], [82, 20], [90, 45], [78, 72], [52, 90], [22, 84], [8, 58]],
      },
    },
    { id: "ann-image", clip: 1, startMs: 19_000, endMs: 22_000, x: 70, y: 70, w: 20, h: 20, zIndex: 4, body: { kind: "image", file: "" } },
  ],
  // An invented music bed (a looping piano clip) under the whole take.
  audioTracks: [
    {
      id: "trk-piano", kind: "music", file: "3a9e1c40-7f2b-4d18-9c55-0e6b2d4f8a17.m4a", clip: 0, startMs: 500, durationMs: 7_000,
      offsetMs: 0, spanMs: null, gainDb: -18, looped: true, fadeInMs: 1_000, fadeOutMs: 1_000, muted: false, label: "Calm Piano Loop",
    },
  ],
  systemGainDb: 0,
  // Stage 5: the invented take has a camera, picture-in-picture, and one layout change.
  camera: {
    preset: "picture-in-picture",
    sizePct: 40,
    mask: "square",
    roundness: null,
    mirror: false,
    reactiveZoom: true,
    anchor: "bottom-right",
    crop: null,
    background: { mode: "none", blur: 0.5, custom: { kind: "wallpaper", id: "01" } },
  },
  layoutRegions: [{ id: "lay-dual", clip: 1, startMs: 16_000, preset: "dual-frame" }],
  // Stage 6: an invented transcript (the words between 9 s and 11 s were cut) and captions on.
  transcripts: [{ source: null, lang: "en", words: edWords() }],
  captions: {
    enabled: true, fontSize: 48, fontFamily: "Inter", bold: true, color: "#ffffff", plate: true, plateColor: "#000000",
    plateOpacity: 0.55, anchorV: "bottom", anchorH: "center", insetY: null, insetX: null, minWords: 2, maxWords: 7, lang: "en",
  },
};
/** Invented narration for the mockup take, timed word by word. */
function edWords() {
  const text =
    "Welcome to Northwind. In the next minute we set up a new workspace, invite the team and share the first board. " +
    "Um, skip this. " +
    "Open settings, choose members, and paste the addresses of your colleagues. That is all it takes.";
  let t = 400;
  return text.split(" ").map((w, i) => {
    if (w === "Um,") t = 9_100;
    if (w === "Open") t = 11_300;
    const len = 180 + (w.length * 35);
    const word = { id: `w${i + 1}`, startMs: t, endMs: t + len, text: w };
    t += len + (w.endsWith(".") || w.endsWith(",") ? 260 : 60);
    return word;
  });
}
/** Invented wallpaper thumbnails and pointer previews (SVG data URLs, nothing real). */
const ED_WALL_HUES: [number, number][] = [[220, 280], [200, 160], [20, 340], [40, 10], [180, 230], [260, 300], [100, 150], [0, 30]];
const edSvg = (svg: string) => `data:image/svg+xml;utf8,${encodeURIComponent(svg)}`;
const edWallpaper = ([a, b]: [number, number]) =>
  edSvg(`<svg xmlns="http://www.w3.org/2000/svg" width="160" height="90"><defs><linearGradient id="g" x1="0" y1="0" x2="1" y2="1"><stop offset="0" stop-color="hsl(${a} 70% 55%)"/><stop offset="1" stop-color="hsl(${b} 65% 35%)"/></linearGradient></defs><rect width="160" height="90" fill="url(#g)"/></svg>`);
const ED_THEMES: [string, string][] = [["default", "#111"], ["studio-ink", "#1e293b"], ["prism-glow", "#8b5cf6"], ["pop-coral", "#f97316"], ["pixel-candy", "#ec4899"], ["star-sprout", "#22c55e"], ["glass-lens", "#3b82f6"]];
const edArrow = (fill: string) =>
  edSvg(`<svg xmlns="http://www.w3.org/2000/svg" width="32" height="32"><path d="M6 3 L6 25 L12 19 L16 28 L20 26 L16 17 L24 17 Z" fill="${fill}" stroke="#fff" stroke-width="1.5"/></svg>`);
function edOutputSize(doc: EdDoc): [number, number] {
  const a = (doc.output as { aspect: string }).aspect;
  const fixed: Record<string, number> = { "16:9": 16 / 9, "9:16": 9 / 16, "1:1": 1, "4:3": 4 / 3, "3:4": 3 / 4, "21:9": 21 / 9 };
  const r = fixed[a] ?? 3024 / 1964;
  return r >= 1 ? [1920, Math.round(1920 / r / 2) * 2] : [Math.round(1920 * r / 2) * 2, 1920];
}
let edPlaying = false;
let edPos = 3_400;
const ED_FORMAT_ORDER = ["auto", "source", "16:9", "9:16", "1:1", "4:3", "3:4", "21:9"];
function edFormats(doc: EdDoc): string[] {
  const d = doc as unknown as { output: { aspect: string }; variants?: { aspect: string }[] };
  const all = [d.output.aspect, ...(d.variants ?? []).map((v) => v.aspect)];
  return ED_FORMAT_ORDER.filter((a) => all.includes(a));
}

function edView(doc: EdDoc) {
  let t = 0;
  const speed: { index: number; progStartMs: number; progEndMs: number }[] = [];
  const clips = doc.clips.map((c, ci) => {
    const start = t;
    let src = c.startMs;
    doc.speed
      .map((r, index) => ({ r, index }))
      .filter(({ r }) => r.clip === ci)
      .sort((a, b) => a.r.startMs - b.r.startMs)
      .forEach(({ r, index }) => {
        t += r.startMs - src;
        const ps = t;
        t += (r.endMs - r.startMs) / r.speed;
        speed.push({ index, progStartMs: Math.round(ps), progEndMs: Math.round(t) });
        src = r.endMs;
      });
    t += c.endMs - src;
    return { progStartMs: Math.round(start), progEndMs: Math.round(t) };
  });
  // Zooms: source ms → programme ms via the clip's offset (the mockup's docs have no speed there).
  const zooms = ((doc.zooms ?? []) as { clip: number; startMs: number; endMs: number }[])
    .map((z, index) => ({ z, index }))
    .filter(({ z }) => doc.clips[z.clip])
    .map(({ z, index }) => {
      const off = clips[z.clip].progStartMs - doc.clips[z.clip].startMs;
      return { index, progStartMs: z.startMs + off, progEndMs: z.endMs + off };
    });
  // Effect regions the same way (mockup only; Rust maps these in the app).
  const effects = ((doc.effects ?? []) as { id: string; clip: number; startMs: number; endMs: number; effect: string }[])
    .map((r, index) => ({ r, index }))
    .filter(({ r }) => doc.clips[r.clip])
    .map(({ r, index }) => {
      const off = clips[r.clip].progStartMs - doc.clips[r.clip].startMs;
      return { index, id: r.id, effect: r.effect, programmeStartMs: r.startMs + off, programmeEndMs: r.endMs + off, row: 0 };
    });
  // Annotations the same way (mockup only; the app gets these spans from Rust).
  const annotationSpans = ((doc.annotations ?? []) as { clip: number; startMs: number; endMs: number }[])
    .map((a, index) => ({ a, index }))
    .filter(({ a }) => doc.clips[a.clip])
    .map(({ a, index }) => {
      const off = clips[a.clip].progStartMs - doc.clips[a.clip].startMs;
      return { index, progStartMs: a.startMs + off, progEndMs: a.endMs + off };
    });
  // Tracks and waveforms (mockup only; the app gets both from Rust).
  const programme = Math.round(t);
  const tracks: Record<string, unknown>[] = [];
  ((doc.audioTracks ?? []) as { clip: number; startMs: number; durationMs: number; offsetMs: number; spanMs: number | null; looped: boolean; fadeInMs: number; fadeOutMs: number; gainDb: number; kind: string; muted: boolean }[])
    .forEach((a, index) => {
      if (!doc.clips[a.clip]) return;
      const head = a.startMs + clips[a.clip].progStartMs - doc.clips[a.clip].startMs;
      const window = Math.max(1, a.durationMs - a.offsetMs);
      const span = a.spanMs ?? (a.looped ? programme - head : window);
      for (let played = 0, repeat = 0; played < span && repeat < 1000; played += window, repeat += 1) {
        const len = Math.min(window, span - played);
        tracks.push({
          index, repeat, progStartMs: head + played, progEndMs: head + played + len, fileStartMs: a.offsetMs, fileEndMs: a.offsetMs + len,
          fadeInMs: played === 0 ? a.fadeInMs : 0, fadeOutMs: played + len >= span ? a.fadeOutMs : 0, gainDb: a.gainDb, kind: a.kind, muted: a.muted,
        });
        if (!a.looped) break;
      }
    });
  const wave = (seed: number, n: number, loud: number) =>
    Array.from({ length: n }, (_, i) => {
      const v = loud * (0.35 + 0.65 * Math.abs(Math.sin(i * 0.13 + seed) * Math.sin(i * 0.031 + seed * 2)));
      return [-v * (0.8 + 0.2 * Math.sin(i * 0.7)), v];
    }).flat();
  const waveforms = [
    ...clips.flatMap((c, ci) => [
      { kind: "system", clip: ci, track: null, repeat: 0, progStartMs: c.progStartMs, progEndMs: c.progEndMs, peaks: wave(ci + 1, 160, 0.35) },
      { kind: "mic", clip: ci, track: null, repeat: 0, progStartMs: c.progStartMs, progEndMs: c.progEndMs, peaks: wave(ci + 4, 160, 0.75) },
    ]),
    ...tracks.map((e) => ({
      kind: "track", clip: null, track: e.index, repeat: e.repeat, progStartMs: e.progStartMs, progEndMs: e.progEndMs, peaks: wave(9, 120, 0.6),
    })),
  ];
  const clickHits = [4_200, 9_800, 15_300].flatMap((ms) => [
    { progMs: ms, gain: 1, release: false },
    { progMs: ms + 90, gain: 0.7, release: true },
  ]);
  const screenRects = doc.clips.map(() => [0.06, 0.06, 0.88, 0.88] as [number, number, number, number]);
  // Layout regions until the clip's next one or its end (mockup only; Rust maps these in the app,
  // ignoring speed here like the zooms).
  const regions = (doc.layoutRegions ?? []) as { clip: number; startMs: number }[];
  const layoutSpans = regions
    .map((r, index) => ({ r, index }))
    .filter(({ r }) => doc.clips[r.clip])
    .map(({ r, index }) => {
      const c = doc.clips[r.clip];
      const next = Math.min(c.endMs, ...regions.filter((o) => o.clip === r.clip && o.startMs > r.startMs).map((o) => o.startMs));
      const off = clips[r.clip].progStartMs - c.startMs;
      return { index, progStartMs: r.startMs + off, progEndMs: Math.min(clips[r.clip].progEndMs, next + off) };
    });
  const clipCameras = doc.clips.map(() => true);
  // A rough picture-in-picture box (mockup only; Rust lays it out in the app): a 16:9 camera at
  // `sizePct`, by the free position or the bottom-right corner.
  const [ow, oh] = edOutputSize(doc);
  const cam = (doc.camera ?? {}) as { sizePct?: number; mask?: string; position?: { x: number; y: number } | null };
  const long = (Math.min(ow, oh) * (cam.sizePct ?? 40)) / 100;
  const [cw, ch] = cam.mask === "rectangle" ? [long, (long * 9) / 16] : [(long * 9) / 16, (long * 9) / 16];
  const m = Math.min(ow, oh) * 0.02;
  const cx = cam.position ? Math.min(ow - m - cw, Math.max(m, cam.position.x * ow - cw / 2)) : ow - m - cw;
  const cy = cam.position ? Math.min(oh - m - ch, Math.max(m, cam.position.y * oh - ch / 2)) : oh - m - ch;
  const cameraRects = doc.clips.map(() => [cx / ow, cy / oh, cw / ow, ch / oh] as [number, number, number, number]);
  return { doc, programmeMs: programme, clips, speed, zooms, effects, annotationSpans, layoutSpans, clipCameras, cameraRects, tracks, waveforms, clickHits, screenRects, outputSize: edOutputSize(doc), takes: edTakes, micSources: [""], denoised: [""], formats: edFormats(doc) };
}
function edFrame(): ArrayBuffer {
  const w = 320, h = 180;
  const buf = new ArrayBuffer(16 + w * h * 4);
  const v = new DataView(buf);
  v.setBigUint64(0, BigInt(Date.now()), true);
  v.setUint32(8, w, true);
  v.setUint32(12, h, true);
  const px = new Uint8ClampedArray(buf, 16);
  for (let y = 0; y < h; y++)
    for (let x = 0; x < w; x++) {
      const i = (y * w + x) * 4;
      const win = x > 40 && x < 280 && y > 30 && y < 150;
      px[i] = win ? 236 : 30 + y / 3;
      px[i + 1] = win ? (y < 44 ? 210 : 240) : 60 + x / 8;
      px[i + 2] = win ? 245 : 120 + x / 4;
      px[i + 3] = 255;
    }
  return buf;
}
const edThumb = (i: number) =>
  "data:image/svg+xml;utf8," +
  encodeURIComponent(`<svg xmlns='http://www.w3.org/2000/svg' width='160' height='90'><rect width='160' height='90' fill='hsl(${210 + i * 7},45%,${30 + (i % 4) * 4}%)'/><rect x='20' y='15' width='120' height='60' rx='4' fill='#eef'/></svg>`);

/** Invented screenshot for the screenshot editor (a plain settings window). */
/** Invented text on the mock screenshot, plus fake OCR word boxes for it
 *  (Menlo 14 px: 8.43 px per character, glyphs ~11 px tall). */
const SHOT_LINES = [
  { x: 64, y: 252, text: "Kontakt: ana.lima@northwind.example" },
  { x: 64, y: 282, text: "Telefon: +49 30 5550 1234" },
  { x: 64, y: 312, text: "IBAN DE89 3704 0044 0532 0130 00" },
  { x: 64, y: 342, text: "API key: sk-proj-Demo0123456789abcdefXYZ" },
  { x: 64, y: 372, text: "Server 203.0.113.42 läuft" },
  { x: 64, y: 402, text: "Notiz: alles andere ist harmlos" },
];
function shotOcrJson(): string {
  const W = 960, H = 600, cw = 8.43, top = 11;
  return JSON.stringify(
    SHOT_LINES.map((l) => {
      const words = [...l.text.matchAll(/\S+/g)].map((m) => ({
        s: m.index!,
        e: m.index! + m[0].length,
        b: [(l.x + m.index! * cw) / W, (l.y - top) / H, (m[0].length * cw) / W, (top + 3) / H],
      }));
      return { t: l.text, b: [l.x / W, (l.y - top) / H, (l.text.length * cw) / W, (top + 3) / H], w: words };
    }),
  );
}

const SHOT_SVG =
  "data:image/svg+xml;utf8," +
  encodeURIComponent(`<svg xmlns='http://www.w3.org/2000/svg' width='960' height='600'>
<rect width='960' height='600' fill='#f3f4f6'/><rect width='960' height='44' fill='#e5e7eb'/>
<circle cx='24' cy='22' r='7' fill='#f87171'/><circle cx='48' cy='22' r='7' fill='#fbbf24'/><circle cx='72' cy='22' r='7' fill='#34d399'/>
<text x='480' y='28' font-family='Helvetica' font-size='15' text-anchor='middle' fill='#374151'>Northwind — Settings</text>
<rect x='40' y='80' width='300' height='16' rx='4' fill='#d1d5db'/><rect x='40' y='112' width='520' height='12' rx='4' fill='#e5e7eb'/>
<rect x='40' y='134' width='460' height='12' rx='4' fill='#e5e7eb'/><rect x='40' y='156' width='380' height='12' rx='4' fill='#e5e7eb'/>
<rect x='40' y='210' width='420' height='220' rx='10' fill='#ffffff' stroke='#e5e7eb'/>
${SHOT_LINES.map((l) => `<text x='${l.x}' y='${l.y}' font-family='Menlo' font-size='14' fill='#374151'>${l.text}</text>`).join("")}
<rect x='640' y='470' width='180' height='52' rx='12' fill='#2563eb'/>
<text x='730' y='503' font-family='Helvetica' font-size='18' text-anchor='middle' fill='#fff'>Save changes</text></svg>`);
let mockEditorPrefs: string | null = null;
/** An invented raster picture for "insert picture" (a logo-ish card). */
function mockPicture(): string {
  const c = document.createElement("canvas");
  c.width = 320;
  c.height = 200;
  const x = c.getContext("2d");
  if (!x) return "";
  const g = x.createLinearGradient(0, 0, 320, 200);
  g.addColorStop(0, "#f97316");
  g.addColorStop(1, "#db2777");
  x.fillStyle = g;
  x.fillRect(0, 0, 320, 200);
  x.fillStyle = "#fff";
  x.font = "bold 34px Helvetica";
  x.fillText("Northwind", 40, 112);
  return c.toDataURL("image/png");
}

export function handle(cmd: string, args: Record<string, unknown>): unknown {
  switch (cmd) {
    // ── screenshot editor ─────────────────────────────────────────────
    case "get_pending_screenshot_data_url": return SHOT_SVG;
    case "get_editor_prefs": return mockEditorPrefs;
    case "set_editor_prefs": mockEditorPrefs = String(args.json ?? ""); return null;
    case "set_editor_size": return null;
    case "editor_ocr_words": return shotOcrJson();
    case "editor_clipboard_image": return mockPicture();
    case "editor_insert_image_file": return mockPicture();
    case "editor_take_project":
      return new URLSearchParams(window.location.search).has("project")
        ? JSON.stringify({
            version: 1,
            doc: {
              annotations: [
                { id: "a3", type: "arrow", x1: 120, y1: 120, x2: 380, y2: 260, color: "#ef4444", width: 6, style: "classic" },
                { id: "a4", type: "image", x: 560, y: 60, w: 240, h: 150, src: mockPicture() },
              ],
              crop: null,
              background: null,
            },
            background: { kind: "gradient", color: "#6366f1", color2: "#ec4899", padding: 48, radius: 14, shadow: true },
          })
        : null;
    // ── boot ──────────────────────────────────────────────────────────────
    case "get_animation_stage": return "reduced";
    case "get_crt_animation": return { ms: 0, min: 80, max: 900, default_ms: 250 };
    case "get_theme_preference": return "dark";
    case "get_language": return mockLanguage();
    case "set_language": mockLang = (args.setting as typeof mockLang) ?? "system"; return mockLanguage();
    case "get_lineage_highlight": return true;
    case "get_history": return CLIPS;
    case "search_history": return CLIPS.filter((c) => sameFuzzy(String(args.query ?? ""), c.content_text));
    case "get_clip": {
      const c = CLIPS.find((x) => x.id === args.id);
      return c && c.content_type === "image" ? { ...c, content_data: demoImage() } : c ?? null;
    }
    case "list_snippets": return SNIPPETS;
    case "get_direct_slots": return [];
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
    case "get_limits_forecast_open": return ["weekly_all-1"];
    case "set_limits_forecast_open": return null;
    case "claude_limits_status": {
      // Week Sat 23:00Z → Sat 23:00Z, "now" = Wed 11:00Z; a tracker-style
      // forecast with series for the week, linear for the session.
      const start = Date.parse("2026-10-03T23:00:00Z");
      const H = 3_600_000;
      const iso = (ms: number) => new Date(ms).toISOString();
      const now = start + 84 * H;
      const actual: [string, number][] = [];
      for (let h = 0; h <= 84; h += 2) {
        const hr = (h + 1) % 24; // Berlin-ish hour
        const day = hr >= 9 && hr <= 23;
        actual.push([iso(start + h * H), Math.min(58, (actual[actual.length - 1]?.[1] ?? 0) + (day ? 1.5 : 0.1))]);
      }
      const cur = actual[actual.length - 1][1];
      const forecast: [string, number, number, number][] = [];
      for (let h = 84; h <= 168; h += 3) {
        const g = ((h - 84) / 84) * cur * 1.05;
        forecast.push([iso(start + h * H), cur + g, cur + g * 0.75, cur + g * 1.3]);
      }
      const ghost = (k: number) => ({
        start: iso(start - k * 168 * H),
        points: Array.from({ length: 29 }, (_, i) => [i * 360, Math.min(100, i * (2.6 + k * 0.5))] as [number, number]),
      });
      const week = {
        version: 1, basis: "calibrated", confidence: "good", status: "ahead",
        window: { start: iso(start), end: iso(start + 168 * H) }, now: iso(now),
        pace: { planPercent: 50, deltaPoints: cur - 50 },
        atReset: { median: forecast[forecast.length - 1][1], low: forecast[forecast.length - 1][2], high: forecast[forecast.length - 1][3] },
        exhaustsAt: { median: null, early: iso(start + 150 * H), late: null },
        k: 0.012, notes: [], source: "tracker",
        series: { actual, measured: actual.filter((_, i) => i % 6 === 3), forecast, ghosts: [ghost(1), ghost(2), ghost(3)] },
      };
      const session = {
        version: 1, basis: "linear", confidence: "rough", status: "exhausts",
        window: { start: iso(now - 3 * H), end: iso(now + 2 * H) }, now: iso(now),
        pace: { planPercent: 60, deltaPoints: 18 }, atReset: { median: 130, low: 109, high: 151 },
        exhaustsAt: { median: iso(now + 50 * 60_000), early: iso(now + 30 * 60_000), late: iso(now + 85 * 60_000) },
        k: null, notes: [], series: null, source: "local",
      };
      const mk = (id: string, name: string, kind: string, percent: number, resets: number, f: unknown) => ({
        id, name, kind, group: null, percent, resets_at: iso(resets), severity: null, active: true, known: true,
        money: null, window_minutes: kind === "session" ? 300 : 10080, forecast: f,
      });
      return {
        report: {
          limits: [
            mk("session-0", "Aktuelle Sitzung", "session", 78, now + 2 * H, session),
            mk("weekly_all-1", "Woche · alle Modelle", "weekly_all", cur, start + 168 * H, week),
            mk("weekly_scoped-2", "Woche · Fable", "weekly_scoped", 0, start + 168 * H, { ...session, status: "idle", source: "local", pace: { planPercent: 50, deltaPoints: -50 }, window: week.window, atReset: { median: 0, low: 0, high: 0 }, exhaustsAt: null }),
          ],
          extra: null, breakdown: [], legacy: false,
        },
        fetched_at_ms: now, error: null, error_detail: null, retry_at_ms: null, poll_minutes: 5, codex: null, antigravity: null,
      };
    }
    case "get_free_space":
      return { name: "Macintosh HD", mount: "/", available: 186_420_000_000, total: 494_384_795_648 };
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
    case "get_system_stats": {
      // A little movement, so the sparklines have something to draw.
      const w = Math.sin(Date.now() / 2400);
      return { ...STATS, cpu_usage: 23.6 + w * 9, cpu_user: 16.9 + w * 7, cpu_system: 6.7 + w * 2,
        net_rx_per_sec: (2.4 + w * 1.4) * 1024 * 1024, battery: { ...STATS.battery, power_watts: 9.8 + w * 2.2 } };
    }
    case "stats_top_processes": return TOP_PROCS;
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
    case "get_uptime_secs": return 3 * 86400 + 5 * 3600 + 17 * 60 + 42;
    case "list_brightness_monitors": return MONITORS;
    case "get_monitor_brightness": return MONITORS.find((m) => m.id === args.id)?.brightness ?? 70;
    case "list_audio_outputs": return AUDIO;
    case "audio_route":
      return {
        devices: AUDIO.map((d) => ({ ...d, transport: /AirPods/.test(d.name) ? "bluetooth" : /MacBook/.test(d.name) ? "builtin" : "" })),
        boom_enabled: false, boom_installed: false, boom_device: null, boom_target: null,
      };
    case "get_system_volume": return 62;
    case "alias_list": return ALIASES;
    case "ip_fetch": return IP;
    case "snitch_list_apps": return SNITCH_APPS;
    case "snitch_is_armed": return true;
    case "snitch_connections": return SNITCH_CONN;
    case "snitch_geolocate": return SNITCH_GEO;
    case "snitch_home": return { ip: "203.0.113.42", lat: 38.72, lon: -9.14, country: "Portugal", city: "Lisbon", isp: "Northwind Fibre" };
    case "snitch_activity": return [{ pid: 4100, bytes_per_sec: 480_000 }, { pid: 4102, bytes_per_sec: 190_000 }];
    case "token_usage_fetch": return TOKENS;
    case "cleaner_status": return { phase: "idle", view: null };
    case "cleaner_scan": return CLEAN;
    case "bluetooth_list": return BT;
    case "logi_view":
    case "logi_refresh":
      return LOGI;
    case "logi_set_config": {
      LOGI.config = args.config as typeof LOGI.config;
      if (LOGI.config.dpi && LOGI.status.dpi) LOGI.status.dpi.current = LOGI.config.dpi;
      return LOGI;
    }
    case "logi_request_access": return true;
    case "list_processes": return PROCS;
    case "social_metadata": return { ...SOCIAL, url: String(args.url ?? "") };
    case "social_ytdlp_available": return true;
    case "set_suppress_hide": return null;
    case "bench_plan": return { workloads: WL.map((w) => w[1]), estimated_seconds: 9, threads: 12, baseline_machine: "Apple M3 Pro", machine: MACHINE };
    case "bench_history": return BENCH_HISTORY;
    case "bench_run": return benchRunData("r3", 1.02, 0);
    case "pagespeed_analyze": return PAGESPEED;
    case "plugin:clipboard-manager|read_text": return "hello world";
    case "fx_rates": return FX;
    case "loc_count": return LOC;
    case "shazam_history_list": return SONGS;
    case "spotify_history": return SPOTIFY_HISTORY;
    case "spotify_top_artists":
      return [
        { artist: "Parra for Cuva", plays: 42 },
        { artist: "Kraftwerk", plays: 31 },
        { artist: "Khruangbin", plays: 18 },
        { artist: "Moderat", plays: 9 },
      ];
    case "shazam_is_listening": return false;
    // ── settings tab (plausible, invented values) ─────────────────────────
    case "adb_status": return { found: true, devices: [], recording: false };
    case "ai_get_config": return { provider: "claude_cli", models: {} };
    case "ai_provider_status": return [
      { id: "claude_cli", label: "Claude Code (lokal)", configured: true, hint: null, model: "", default_model: "claude-opus-5-5" },
      { id: "anthropic", label: "Anthropic API", configured: false, hint: null, model: "", default_model: "claude-opus-5-5" },
    ];
    case "cleaner_categories": return [];
    case "db_space": return { bytes: 18_400_000, freelist_pages: 12, page_count: 4500 };
    case "faker_locales": return [{ code: "en", label: "English" }, { code: "de", label: "Deutsch" }];
    case "figlet_fonts": return [{ name: "standard", category: "classic", popular: true, pinned: false }];
    case "figlet_get_defaults": return { font: "standard", width: 80, align: "left", trim: true, comment: "none", boxed: false, pinned: [], save_history: false };
    case "get_alarm_style": return "overlay";
    case "get_auto_backup_config": return { enabled: true, folder: "~/Google Drive/Backups", interval_min: 60, keep: 24, encrypt: true, include_history: true, include_timesheet: false };
    case "get_auto_backup_status": return { last_ms: Date.now() - 1_200_000, last_check_ms: Date.now() - 600_000, next_check_ms: Date.now() + 3_000_000, last_error: "", encrypt: true, has_password: true, folder_ok: true, snapshot_count: 12 };
    case "get_auto_expand_config": return { enabled: true, trigger: "delimiter", match_case: false, expand_inside_words: false, undo_enabled: true };
    case "get_autostart_enabled": return true;
    case "get_cleaner_config": return { level: "safe", min_age_days: 7, categories: {}, dev_roots: ["~/dev"], stale_days: 90 };
    case "get_clipboard_privacy": return { exclude_apps: "1Password", auto_clear_seconds: 0 };
    case "get_cursor_wrap_config": return { enabled: true, left: true, right: true, top: true, bottom: true, corner_deadzone_px: 8, wrap_during_drag: false };
    case "get_device_sync_config": return { enabled: false, folder: "", include_totp: false };
    case "get_device_sync_status": return { last_ms: 0, last_error: "", device_id: "mock", peers: 0, has_passphrase: false, folder_ok: false };
    case "get_expander_config": return { enabled: true, hotkey: "Alt+Digit1", accessibility_granted: true };
    case "get_finder_automation_status": return true;
    case "get_gesture_config": return { enabled: true, fingers: 3, volume_step: 5, tiptap: false, typing_guard: true, volume: true, mute: true, guard: {} };
    case "get_history_hotkey": return "Ctrl+Shift+KeyV";
    case "get_history_hotkey_default": return "Ctrl+Shift+KeyV";
    case "get_popup_hotkey": return "Ctrl+Space";
    case "get_popup_hotkey_default": return "Ctrl+Space";
    case "get_history_max": return { max: 1000, min: 50, ceiling: 100000 };
    case "get_input_lock_chord": return ["KeyI", "KeyR"];
    case "get_keepalive_enabled": return false;
    case "get_meme_dir": return "~/My Drive/media/memes";
    case "get_md2pdf_footer":
    case "set_md2pdf_footer":
      return { template: "© {year} Ana Lima | northwind.example", default_template: "© {year} {user}", preview: "© 2026 Ana Lima | northwind.example" };
    case "get_ocr_save_source_image": return true;
    case "get_pagespeed_key": return false;
    case "get_paste_plain_text_only": return true;
    case "get_popup_close_on_blur": return true;
    case "get_repo_config": return { clone_dir: "~/dev", has_token: false, gh_available: true };
    case "get_screen_recording_status": return true;
    case "get_screenshot_sound": return "camera";
    case "get_snippet_storage": return { count: 312, bytes: 184_000 };
    case "get_sound_enabled": return true;
    case "get_sync_config": return { enabled: false, url: "", token: "" };
    case "get_sync_status": return { last_ms: 0, last_error: "" };
    case "get_timesheet_config": return { idle_seconds: 300, retention_days: 90, claude_watcher: true, denylist: "", daily_goal_minutes: 480 };
    case "get_window_palette_config": return { enabled: true, cols: 16, rows: 10, trigger: "zoom_hover", hide_system_menu: true };
    case "get_window_size_preference": return "medium";
    case "get_window_snap_config": return { enabled: true, dwell_ms: 350 };
    case "list_action_hotkeys": return [];
    case "gesture_bindings_get": return {
      bindings: [
        { id: "volume-up", trigger: { kind: "swipe_up", fingers: 3 }, action: { type: "volume_up" }, app: null, app_name: null, enabled: true, typing_guard: true },
        { id: "volume-down", trigger: { kind: "swipe_down", fingers: 3 }, action: { type: "volume_down" }, app: null, app_name: null, enabled: true, typing_guard: true },
        { id: "mute", trigger: { kind: "tap", fingers: 3 }, action: { type: "mute_toggle" }, app: null, app_name: null, enabled: true, typing_guard: true },
      ],
      customised: false, min_fingers: 3, max_fingers: 5,
    };
    case "ai_tasks_state": return { tasks: [], running: [], paused: false, languages: [], next_due: {} };
    case "track_category_rules": return [];
    // ── speedtest (invented runs) ─────────────────────────────────────────
    case "finder_context":
      return { dir: "/Users/ana/Projekte/website/docs", selected: ["/Users/ana/Projekte/website/docs/notiz.md"], selected_count: 1, from_selection: true };
    case "pulse_live":
      return {
        supported: true, enabled: true,
        latest: PULSE_RECENT[PULSE_RECENT.length - 1], recent: PULSE_RECENT,
        groups: PULSE_GROUPS, groups_at: PULSE_NOW - 12, unreadable: 187,
        disk_found: true, capacity: 500e9,
        overhead: { samples: 720, sample_avg_ms: 0.4, sample_max_ms: 1.1, scans: 120, scan_avg_ms: 3.1, scan_max_ms: 5.2, rows_written: 180, cpu_secs: 1.4, cpu_pct: 0.04, running_secs: 3600 },
        db_bytes: 2_400_000,
        smart: { percentage_used: 6, data_written_bytes: 41e12, model: "APPLE SSD AP0512Z", at: PULSE_NOW },
        smartctl_present: true, last_error: null,
      };
    case "pulse_history": return pulseHistoryMock(String((args as { range?: string })?.range ?? "24h"));
    case "get_pulse_config": return { enabled: true, notify: false, notify_minutes: 10, tbw_tb: 0 };
    case "set_pulse_config": return (args as { config: unknown }).config;
    case "github_status":
      return {
        connected: true, source: "oauth", client_id_configured: true, pat_stored: false, gh_available: false,
        user_id: 1, login: "northwind-ana", name: "Ana Lima", avatar_url: null, html_url: "https://github.com/northwind-ana",
        last_ok_ms: GH_NOW - 12 * 60_000, last_checked_ms: GH_NOW - 12 * 60_000, last_error: null,
        rate: { limit: 5000, remaining: 4987, reset_at_ms: GH_NOW + 40 * 60_000 }, scopes: "",
        next_refresh_ms: GH_NOW + 48 * 60_000, rate_limited_until: null, refreshing: false,
        device: { state: "idle" }, config: { auto_refresh: true, interval_h: 1, private_repos: false },
      };
    case "github_refresh": return { gate: { kind: "too_soon", next_at_ms: GH_NOW + 48 * 60_000 }, fetched: false, account_snapshot: false, repo_snapshots: 0 };
    case "github_dashboard": return githubDashboardMock(String((args as { range?: string })?.range ?? "30d"));
    case "github_repo": {
      const row = GH_ROWS.find((r) => r.name === String((args as { query?: string }).query ?? "").toLowerCase()) ?? GH_ROWS[0];
      const share = row.stars / GH_STARS[59][1];
      const stars = GH_STAR_HISTORY.filter(([t]) => t >= (row.created_at_ms ?? 0)).map(([t, v]) => [t, Math.round(v * share)] as [number, number]);
      return {
        repo: { ...row, watchers: 14, description: "A fast command-line companion for harbour logistics." },
        stars: [[row.created_at_ms, 0], ...stars],
        forks: stars.map(([t, v]) => [t, Math.round(v / 7)]),
        watchers: [[GH_NOW - 40 * GH_DAY, 11], [GH_NOW - 12 * GH_DAY, 13], [GH_NOW, 14]],
        open_issues: GH_FOLLOWERS.map(([t], i) => [t, 3 + (i % 5)]),
        star_deltas: { d24h: 2, d7d: 9, d30d: 38, d90d: 71, d1y: 160 },
        fork_deltas: { d24h: 0, d7d: 1, d30d: 4, d90d: 9, d1y: 21 },
        first_seen_ms: GH_FOLLOWERS[0][0],
        stars_synced: { synced_ms: GH_NOW, synced_value: row.stars, count: row.stars, truncated: false },
        forks_synced: { synced_ms: GH_NOW, synced_value: row.forks, count: row.forks, truncated: false },
        star_gains: GH_MONTHS,
      };
    }
    case "github_repo_extras":
      return {
        commits: Array.from({ length: 52 }, (_, i) => [GH_NOW - (51 - i) * 7 * GH_DAY, Math.max(0, Math.round(6 + 5 * Math.sin(i / 4) + (i % 7)))]),
        commits_pending: false,
        releases: { count: 23, latest_tag: "v2.4.0", latest_ms: GH_NOW - 9 * GH_DAY, downloads: 4_812, dates: Array.from({ length: 23 }, (_, i) => GH_NOW - (9 + i * 38) * GH_DAY) },
        languages: [["Rust", 812_000], ["TypeScript", 96_000], ["Shell", 12_000]],
        views: { count: 1_204, uniques: 388, days: Array.from({ length: 14 }, (_, i) => [GH_NOW - (13 - i) * GH_DAY, 60 + Math.round(40 * Math.sin(i / 2) + i * 3)]) },
        clones: { count: 143, uniques: 61, days: Array.from({ length: 14 }, (_, i) => [GH_NOW - (13 - i) * GH_DAY, 6 + (i % 5) * 3]) },
        fetched_ms: GH_NOW,
      };
    case "get_github_config": return { auto_refresh: true, interval_h: 1, private_repos: false };
    case "speedtest_running": return false;
    case "speedtest_run": return SPEED_RUNS[0];
    case "speedtest_history": return SPEED_RUNS;
    case "speedtest_clear_history": return null;
    // Record HUD (stage 5): `?hud=denied` = camera on but refused (hint), otherwise authorized
    // with two cameras (device select). Source picked, every toggle on — the HUD's worst case.
    case "record_status":
      return { supported: true, reason: null, phase: "idle", elapsedMs: 0, projectDir: null, source: "Studio Display (3024 × 1964)", lowDiskMinutes: null, cameraWarning: null };
    case "get_record_config":
      return { projectDir: "/Users/demo/Movies/Inspector Rust", countdown: true, cursorMode: "editable-overlay", systemAudio: true, mic: true, openEditor: true, autoZoom: true, camera: true, cameraDevice: "", cameraQuality: "1080p" };
    case "set_record_config": return args.config ?? null;
    case "record_mic_devices":
      return [
        { id: "MacBook Pro Microphone", name: "MacBook Pro Microphone", isDefault: true },
        { id: "Studio Mic USB", name: "Studio Mic USB", isDefault: false },
      ];
    case "record_mic_meter_start":
    case "record_mic_meter_stop":
    case "record_camera_preview_start":
    case "record_camera_preview_stop":
      return null;
    case "record_mic_level": {
      // An invented speaking level that moves.
      const t = Date.now() / 1000;
      const level = Math.max(0.05, Math.min(0.85, 0.45 + 0.3 * Math.sin(t * 5.1) + 0.12 * Math.sin(t * 13.7)));
      return { level, dbfs: -60 + level * 60, fellBack: false, live: true };
    }
    case "record_camera_preview_frame": return mockCameraFrame(Number(args.since));
    case "record_camera_auth":
      return new URLSearchParams(location.search).get("hud") === "denied" ? "denied" : "authorized";
    case "record_camera_devices":
      return [
        { id: "cam-builtin", name: "FaceTime HD Camera", builtin: true },
        { id: "cam-usb", name: "Studio Webcam 4K", builtin: false },
      ];
    case "get_record_shortcuts": return mockShortcuts;
    case "set_record_shortcuts": mockShortcuts = (args.shortcuts as object) ?? {}; return mockShortcuts;
    case "editor_current": return {
        sessionId: 1,
        project: { dir: "/Users/ana/Movies/Inspector Rust/2026-11-02 Demo", title: "Onboarding walkthrough", createdAt: 0, durationMs: 24_000, status: "ready", failure: null },
        meta: { source: "display", sourceLabel: "Studio Display", width: 3024, height: 1964, fps: 60, durationMs: 24_000, systemAudio: true, mic: { offsetMs: 0, sampleRate: 48_000 }, cursorMode: "system", pauses: [], camera: { width: 1920, height: 1080, fps: 30, offsetMs: 0, deviceName: "FaceTime HD Camera" } },
        view: edView(edDoc),
        helperError: null,
      };
    case "editor_segmentation_available": return true;
    case "editor_update": edDoc = args.doc as EdDoc; return edView(edDoc);
    case "editor_seek": edPos = Number(args.programmeMs); return { clip: 0, sourceMs: edPos };
    case "editor_play": edPlaying = Boolean(args.playing); return { playing: edPlaying, positionMs: edPos, programmeMs: edView(edDoc).programmeMs, exporting: false, helperError: null, source: { clip: 0, sourceMs: edPos }, speed: 1 };
    case "editor_position": if (edPlaying) edPos += 66; return { playing: edPlaying, positionMs: edPos, programmeMs: edView(edDoc).programmeMs, exporting: false, helperError: null, source: { clip: 0, sourceMs: edPos }, speed: 1 };
    case "editor_to_source": return { clip: 0, sourceMs: Number(args.programmeMs) };
    case "editor_to_programme": return Number(args.sourceMs);
    case "editor_frame": return Number(args.sinceGen) > 0 ? new ArrayBuffer(0) : edFrame();
    case "editor_resize": return null;
    case "editor_platform_presets": return [
      { id: "youtube", aspect: "16:9", target: "p1080", fps: 30, quality: "standard", loudness: "-14", maxDurationS: null, softDurationS: null, suffix: "youtube" },
      { id: "shorts", aspect: "9:16", target: "p1080", fps: 30, quality: "high", loudness: "-14", maxDurationS: 180, softDurationS: null, suffix: "shorts" },
      { id: "reels", aspect: "9:16", target: "p1080", fps: 30, quality: "high", loudness: "-14", maxDurationS: 900, softDurationS: 180, suffix: "reels" },
      { id: "tiktok", aspect: "9:16", target: "p1080", fps: 30, quality: "high", loudness: "-14", maxDurationS: 3600, softDurationS: 600, suffix: "tiktok" },
    ];
    case "editor_export_info": {
      // Invented numbers in the shape Rust returns (the real ones come from `platform.rs`).
      const o = args.opts as { target?: string; quality?: string; bitrateMbps?: number | null; fps?: number; codec?: string };
      const upright = (edDoc.output as { aspect?: string } | undefined)?.aspect === "9:16";
      const short = ({ p720: 720, p1080: 1080, p1440: 1440, p2160: 2160 } as Record<string, number>)[o.target ?? "p1080"] ?? 1080;
      const long = Math.round((short * 16) / 9 / 2) * 2;
      const [width, height] = upright ? [short, long] : [long, short];
      const bpp = ({ small: 0.07, standard: 0.13, high: 0.2, max: 0.3 } as Record<string, number>)[o.quality ?? "auto"];
      const bitrateBps =
        o.quality === "custom" ? (o.bitrateMbps ?? 12) * 1e6
        : bpp ? Math.round(bpp * width * height * 30 * Math.pow((o.fps ?? 30) / 30, 0.585) * (o.codec === "hevc" ? 0.65 : 1))
        : null;
      const programmeMs = edView(edDoc).programmeMs;
      return {
        width, height, codec: o.codec ?? "h264", bitrateBps, forcedHevc: false,
        upscale: short > 1080 ? Math.round((short / 1080) * 10) / 10 : null,
        programmeMs, durationWarning: null,
        estimatedBytes: bitrateBps ? Math.round(((bitrateBps + 256_000) * programmeMs) / 8000) : null,
        hasCaptions: ((edDoc.transcripts as unknown[] | undefined) ?? []).length > 0,
      };
    }
    case "editor_export_cover": return "/Users/ana/Downloads/Product tour 2026-11-13 09.05 – cover.jpg";
    case "editor_thumbs": return Array.from({ length: Number(args.count) || 8 }, (_, i) => edThumb(i));
    case "editor_assets": return {
        wallpapers: ED_WALL_HUES.map((h, i) => ({ id: `0${i + 1}`, thumb: edWallpaper(h) })),
        themes: ED_THEMES.map(([id, fill]) => ({ id, preview: edArrow(fill) })),
        hasCursorData: true,
        clickDown: null,
        clickUp: null,
      };
    case "editor_variant": {
      const aspect = String(args.aspect);
      const d = edDoc as unknown as { output: { aspect: string }; variants?: { aspect: string }[] };
      const own = { aspect: d.output.aspect };
      const others = (d.variants ?? []).filter((v) => v.aspect !== aspect);
      if (args.op === "remove") {
        if (aspect === d.output.aspect && others.length) edDoc = { ...edDoc, output: { aspect: others[0].aspect }, variants: others.slice(1) } as EdDoc;
        else edDoc = { ...edDoc, variants: others } as EdDoc;
      } else if (aspect !== d.output.aspect) {
        edDoc = { ...edDoc, output: { aspect }, variants: [...others, own] } as EdDoc;
      }
      return edView(edDoc);
    }
    case "editor_denoise": return { ...edView(edDoc), micSources: [""], denoised: [""] };
    case "editor_autocut_suggest": {
      const o = (args.opts ?? {}) as { silences?: boolean; fillers?: boolean };
      const all = [
        { id: "cut-0", kind: "silence", clip: 0, startMs: 2_400, endMs: 4_100, programmeMs: 2_400, text: "" },
        { id: "cut-1", kind: "filler", clip: 0, startMs: 6_050, endMs: 6_420, programmeMs: 6_050, text: "ähm" },
        { id: "cut-2", kind: "silence", clip: 0, startMs: 11_200, endMs: 12_350, programmeMs: 11_200, text: "" },
        { id: "cut-3", kind: "filler", clip: 0, startMs: 15_900, endMs: 16_180, programmeMs: 15_900, text: "äh" },
      ];
      return {
        suggestions: all.filter((s) => (s.kind === "silence" ? o.silences !== false : o.fillers !== false)),
        hasMic: true,
        micPending: false,
        hasTranscript: true,
      };
    }
    case "editor_auto_zoom": {
      const zs = (edDoc.zooms ?? []) as { origin: string }[];
      if (!zs.some((z) => z.origin === "auto")) {
        edDoc = {
          ...edDoc,
          autoZoomDone: true,
          zooms: [
            ...zs,
            ...[3_700, 9_300, 14_800].map((s, i) => ({
              id: `auto-0-${s}`, clip: 0, startMs: s, endMs: s + 2_000, depth: [3, 2, 3][i], customScale: null,
              focusX: [0.3, 0.62, 0.48][i], focusY: [0.4, 0.55, 0.3][i], focusMode: "auto", origin: "auto", hideCursor: false,
            })),
          ],
        };
      }
      return edView(edDoc);
    }
    // Clip operations: the mockup only re-orders/copies the per-clip lists (no ids, no tracks).
    case "editor_move_clip": {
      const { from, to } = args as { from: number; to: number };
      const n = edDoc.clips.length;
      if (from >= n || to >= n || from === to) return edView(edDoc);
      const order = [...Array(n).keys()];
      order.splice(to, 0, order.splice(from, 1)[0]);
      const newOf = (old: number) => order.indexOf(old);
      const crop = (edDoc.crop ?? []) as unknown[];
      edDoc = {
        ...edDoc,
        clips: order.map((o) => edDoc.clips[o]),
        crop: order.map((o) => crop[o] ?? null),
        speed: edDoc.speed.map((r) => ({ ...r, clip: newOf(r.clip) })),
        zooms: ((edDoc.zooms ?? []) as { clip: number }[]).map((z) => ({ ...z, clip: newOf(z.clip) })),
      };
      return edView(edDoc);
    }
    case "editor_duplicate_clip": {
      const { index } = args as { index: number };
      const c = edDoc.clips[index];
      if (!c) return edView(edDoc);
      const shift = (k: number) => (k > index ? k + 1 : k);
      const crop = (edDoc.crop ?? []) as unknown[];
      edDoc = {
        ...edDoc,
        clips: [...edDoc.clips.slice(0, index + 1), { ...c }, ...edDoc.clips.slice(index + 1)],
        crop: [...crop.slice(0, index + 1), crop[index] ?? null, ...crop.slice(index + 1)],
        speed: edDoc.speed.map((r) => ({ ...r, clip: shift(r.clip) })),
        zooms: ((edDoc.zooms ?? []) as { clip: number }[]).map((z) => ({ ...z, clip: shift(z.clip) })),
      };
      return edView(edDoc);
    }
    case "editor_import_take":
      edDoc = {
        ...edDoc,
        clips: [...edDoc.clips, { startMs: 0, endMs: 8_000, source: "a1b2c3d4e5f60718" } as EdDoc["clips"][number]],
        crop: [...((edDoc.crop ?? []) as unknown[]), null],
      };
      edTakes.a1b2c3d4e5f60718 = { durationMs: 8_000, width: 1920, height: 1080, systemAudio: true, hasMic: false, micOffsetMs: 0, hasCursor: false };
      return edView(edDoc);
    case "editor_voiceover_start": {
      const { programmeMs } = args as { programmeMs: number };
      edVoiceover = { programmeMs, file: "voiceover-1793000000000.wav" };
      return { sessionId: 1, file: edVoiceover.file, programmeMs, maxMs: 8_000, clip: 0, sourceMs: programmeMs };
    }
    case "editor_voiceover_stop": {
      const take = edVoiceover;
      edVoiceover = null;
      if (!take) return edView(edDoc);
      const track = {
        id: "c0ffee00-0000-4000-8000-000000000001", kind: "voiceover", file: take.file, clip: 0, startMs: take.programmeMs,
        durationMs: 3_200, offsetMs: 0, spanMs: 3_200, gainDb: 0, looped: false, fadeInMs: 0, fadeOutMs: 0, muted: false, label: "",
      };
      edDoc = { ...edDoc, audioTracks: [...((edDoc.audioTracks ?? []) as unknown[]), track] } as EdDoc;
      return edView(edDoc);
    }
    case "editor_voiceover_cancel":
      edVoiceover = null;
      return null;
    case "record_presets_list": return { presets: mockPresets, defaultFile: mockDefaultPreset };
    case "record_preset_save": {
      const name = String(args.name).trim();
      const entry = { name, file: `${name}.irpreset`, ok: true, error: null };
      if (mockPresets.some((p) => p.name.toLowerCase() === name.toLowerCase())) throw "record.editor.preset_name_taken";
      mockPresets = [...mockPresets, entry].sort((a, b) => a.name.localeCompare(b.name));
      return entry;
    }
    case "record_preset_apply": {
      const portrait = String(args.file).startsWith("Phone");
      edDoc = {
        ...edDoc,
        output: { aspect: portrait ? "9:16" : "16:9" },
        appearance: { ...(edDoc.appearance as object), padding: portrait ? 12 : 30, frame: portrait ? "phone" : "laptop", frameTheme: "dark" },
      };
      return edView(edDoc);
    }
    case "record_preset_rename": {
      const name = String(args.name).trim();
      const entry = { name, file: `${name}.irpreset`, ok: true, error: null };
      mockPresets = mockPresets.map((p) => (p.file === args.file ? entry : p));
      if (mockDefaultPreset === args.file) mockDefaultPreset = entry.file;
      return entry;
    }
    case "record_preset_delete":
      mockPresets = mockPresets.filter((p) => p.file !== args.file);
      if (mockDefaultPreset === args.file) mockDefaultPreset = null;
      return null;
    case "record_preset_set_default": mockDefaultPreset = (args.file as string | null) ?? null; return null;
    case "record_presets_reveal": return null;
    case "record_cli_path": return "/Applications/InspectorRust.app/Contents/MacOS/ir-record";
    case "record_list_projects":
      return [
        { dir: "/Users/ana/Movies/Inspector Rust/2026-10-02 Onboarding", title: "Onboarding", createdAt: Date.parse("2026-10-02T09:12:00Z"), updatedAt: Date.parse("2026-10-04T11:30:00Z"), durationMs: 8_000, status: "ready", failure: null, active: false, thumb: null },
        { dir: "/Users/ana/Movies/Inspector Rust/2026-09-28 Release notes", title: "Release notes", createdAt: Date.parse("2026-09-28T15:40:00Z"), updatedAt: Date.parse("2026-09-28T16:02:00Z"), durationMs: 41_500, status: "ready", failure: null, active: false, thumb: null },
      ];
    case "record_rename_project": return String(args.title).trim();
    case "record_open_dialog": return null;
    case "get_meeting_config": return { mic: true, sys: true, mic_device: null, model: "large-v3-turbo-q5", lang: "auto", root: null, keep_audio: false, audio_days: 7, dedupe: "auto", dedupe_threshold: 0.8, ticker: {} };
    case "meeting_model_status": return [{ id: "large-v3-turbo-q5", file: "ggml-large-v3-turbo-q5_0.bin", bytes: 574_041_195, installed: true }];
    case "editor_transcribe": return { transcripts: [{ source: null, lang: String(args.lang ?? "en"), words: edWords() }], skipped: [], error: null, cancelled: false };
    case "editor_transcribe_cancel": return true;
    case "editor_import_asset":
      return args.kind === "audio"
        ? { file: "5b2d7e90-1c3a-4f6b-8d42-a9e0c7f31b65.mp3", durationMs: 42_000 }
        : { file: "6f1c2a8e-4b1d-4c5e-9a3f-2d7b8e0c1a44.png" };
    default:
      if (!misses.has(cmd)) {
        misses.add(cmd);
        console.info("[mock] unhandled", cmd, JSON.stringify(args).slice(0, 160));
      }
      return null;
  }
}

(window as unknown as { __mockMisses: Set<string> }).__mockMisses = misses;

let mockCamGen = 0;
/** An invented camera picture (a drawn figure, no real person): 8 bytes generation + JPEG. */
function mockCameraFrame(since: number): ArrayBuffer {
  if (mockCamGen > since && mockCamGen > 0) return new ArrayBuffer(0);
  const c = document.createElement("canvas");
  c.width = 640;
  c.height = 360;
  const g = c.getContext("2d");
  if (!g) return new ArrayBuffer(0);
  const bg = g.createLinearGradient(0, 0, 640, 360);
  bg.addColorStop(0, "#27304a");
  bg.addColorStop(1, "#4b3a5e");
  g.fillStyle = bg;
  g.fillRect(0, 0, 640, 360);
  g.fillStyle = "#e7c9a9";
  g.beginPath();
  g.arc(320, 150, 62, 0, Math.PI * 2);
  g.fill();
  g.fillStyle = "#3d6fb6";
  g.beginPath();
  g.ellipse(320, 370, 150, 130, 0, 0, Math.PI * 2);
  g.fill();
  const b64 = c.toDataURL("image/jpeg", 0.8).split(",")[1] ?? "";
  const bin = atob(b64);
  mockCamGen += 1;
  const out = new Uint8Array(8 + bin.length);
  new DataView(out.buffer).setBigUint64(0, BigInt(mockCamGen), true);
  for (let i = 0; i < bin.length; i++) out[8 + i] = bin.charCodeAt(i);
  return out.buffer;
}
