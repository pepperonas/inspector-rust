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
    case "get_uptime_secs": return 3 * 86400 + 5 * 3600 + 17 * 60 + 42;
    case "list_brightness_monitors": return MONITORS;
    case "get_monitor_brightness": return MONITORS.find((m) => m.id === args.id)?.brightness ?? 70;
    case "list_audio_outputs": return AUDIO;
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
    case "shazam_is_listening": return false;
    default:
      if (!misses.has(cmd)) {
        misses.add(cmd);
        console.info("[mock] unhandled", cmd, JSON.stringify(args).slice(0, 160));
      }
      return null;
  }
}

(window as unknown as { __mockMisses: Set<string> }).__mockMisses = misses;
