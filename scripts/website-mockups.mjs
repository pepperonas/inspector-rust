#!/usr/bin/env node
// Renders the product page's mockups from the REAL app UI against a dummy backend.
//
//   node scripts/website-mockups.mjs            all images
//   node scripts/website-mockups.mjs history qr only these ids
//   node scripts/website-mockups.mjs hero       the hero art (three windows, right side left free
//                                               for the headline) → $TMPDIR/inspector-rust-hero.png,
//                                               then: build.py … --hero <that file>
//
// How: core/frontend/mockup.html boots the real app with Tauri's mockIPC and the invented data in
// src/mockup/backend.ts. This script starts Vite, types each scene's query like a user would,
// captures the popup at 2x, and frames it (backdrop, shadow) into website/assets/{gallery,eggs}/.
// Nothing here reads the machine's clipboard, files or accounts — every visible value is made up.
//
// Needs: Playwright (the copy bundled with @playwright/mcp is found automatically, or set
// PLAYWRIGHT_MODULE), cwebp and magick for the final encodes.
import { createRequire } from "node:module";
import { spawn, execFileSync } from "node:child_process";
import { mkdirSync, mkdtempSync, readFileSync, existsSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = join(dirname(fileURLToPath(import.meta.url)), "..");
const FRONTEND = join(ROOT, "core/frontend");
const OUT = join(ROOT, "website/assets");
const PORT = 1433;
const BASE = `http://localhost:${PORT}/mockup.html`;

function loadPlaywright() {
  const candidates = [process.env.PLAYWRIGHT_MODULE, "playwright"];
  try {
    const g = execFileSync("npm", ["root", "-g"], { encoding: "utf8" }).trim();
    candidates.push(join(g, "@playwright/mcp/node_modules/playwright"), join(g, "playwright"));
  } catch { /* no global npm */ }
  for (const c of candidates.filter(Boolean)) {
    try { return createRequire(import.meta.url)(c); } catch { /* next */ }
  }
  throw new Error("Playwright not found — npm i -g playwright, or set PLAYWRIGHT_MODULE");
}

// ── scenes ────────────────────────────────────────────────────────────────────
// `keys` run after typing; `wait` ms before the capture. Gallery ids are what site.json lists.
const GALLERY = [
  { id: "history", query: "", keys: ["ArrowDown", "ArrowDown", "ArrowDown"] },
  { id: "snippets", query: "sig", keys: ["ArrowDown"] },
  { id: "calculator", query: "5 km in mi" },
  { id: "translate", query: "tren The meeting moves to Thursday at three.", wait: 2200 },
  { id: "commands", query: "?" },
  { id: "weather", query: "weather lisbon", wait: 2400 },
  { id: "stats", query: "stats", wait: 2200 },
  { id: "totp", query: "2fa", keys: ["Enter"], wait: 1800 },
  { id: "disk", query: "disk ~/Projects", keys: ["Enter"], wait: 2600 },
  { id: "clock", query: "clock" },
  { id: "hue", query: "hue", wait: 1800 },
  { id: "qr", query: "qr https://inspector-rust.celox.io" },
];

// Games resume a suspended run from localStorage — seeding one gives an attractive, repeatable state.
const snakeBody = [[16, 7], [15, 7], [14, 7], [13, 7], [12, 7], [12, 8], [12, 9], [11, 9], [10, 9], [9, 9], [8, 9],
  [8, 8], [8, 7], [8, 6], [7, 6], [6, 6]].map(([x, y]) => ({ x, y }));
const aliens = [];
for (let row = 0; row < 5; row++) {
  for (let col = 0; col < 11; col++) {
    const gone = (row === 4 && [0, 1, 4, 8, 9, 10].includes(col)) || (row === 3 && [0, 10, 5].includes(col)) || (row === 2 && col === 9);
    aliens.push({ alive: !gone, row, x: 250 + col * 36, y: 96 + row * 28 });
  }
}
const SEEDS = {
  "inspector-rust.game.pong.state": { ballX: 470, ballY: 170, ballVx: 6.2, ballVy: 2.4, ballSpeed: 6.8, playerY: 210, botY: 130, playerScore: 3, botScore: 2 },
  "inspector-rust.game.pong.best": 7,
  "inspector-rust.game.snake-classic.state": { snake: snakeBody, dir: "right", pendingDir: "right", food: { x: 19, y: 7 }, score: 13 },
  "inspector-rust.game.snake-classic.best": 21,
  "inspector-rust.game.spacer.state": { aliens, bullets: [{ active: true, x: 432, y: 330, vy: -11, fromPlayer: true }, { active: true, x: 330, y: 280, vy: 7, fromPlayer: false }], playerX: 410, playerY: 505, formationDir: 1, score: 340, lives: 2 },
  "inspector-rust.game.spacer.best": 1280,
  "inspector-rust.game.flappy.state": { birdY: 205, vy: -3, pipes: [{ x: 120, gapTop: 160, scored: true }, { x: 370, gapTop: 130, scored: false }, { x: 620, gapTop: 205, scored: false }], score: 7, dead: false, sinceSpawnX: 30, started: true },
  "inspector-rust.game.flappy.best": 23,
};

const EGGS = [
  { id: "pong", query: "getshaky", keys: ["Shift"], wait: 2400, after: 90 },
  { id: "snake", query: "rockthebox", keys: ["Shift"], wait: 2600, after: 60 },
  { id: "spacer", query: "spacer", keys: ["Shift"], wait: 2200, after: 90 },
  { id: "flappy", query: "learningtofly", pre: 1500, keys: [], wait: 70, after: 0 },
  { id: "equalizer", query: "equalizer", keys: ["Enter"], wait: 6500 },
  { id: "x", window: "x-overlay", wait: 24500, viewport: { width: 1440, height: 900 } },
];

// The hero: three windows packed into x≈40–980 of a 2400×1200 canvas — the page's scrim fades
// everything right of ~24 % of the width into the headline's background — the page sets the
// headline over the right side. Back to front: weather, history, 2FA.
const HERO = [
  { id: "hero-weather", from: "weather", css: "left:40px;top:100px;width:600px;transform:perspective(2400px) rotateY(16deg) rotateZ(-2deg);filter:brightness(.75)" },
  { id: "hero-history", from: "history", css: "left:150px;top:250px;width:760px;transform:perspective(2400px) rotateY(10deg)" },
  { id: "hero-totp", from: "totp", css: "left:560px;top:640px;width:420px;transform:perspective(2400px) rotateY(12deg) rotateZ(1.5deg)" },
];
const HERO_OUT = join(tmpdir(), "inspector-rust-hero.png");

function heroHtml(shots) {
  const wins = shots.map(({ data, css }) =>
    `<img src="${data}" style="position:absolute;${css};border-radius:22px;
      box-shadow:0 60px 120px -30px rgba(0,0,0,.85),0 0 0 1px rgba(255,255,255,.08)">`).join("");
  return `<!doctype html><html><head><style>html,body{margin:0}
    .stage{position:relative;width:2400px;height:1200px;overflow:hidden;
      background:radial-gradient(45% 70% at 22% 40%, rgba(99,102,241,.38), transparent 72%),
                 radial-gradient(35% 50% at 8% 95%, rgba(244,114,182,.18), transparent 70%),
                 #0c0d11;}</style></head><body><div class="stage">${wins}</div></body></html>`;
}

// ── plumbing ──────────────────────────────────────────────────────────────────
async function waitForServer() {
  for (let i = 0; i < 60; i++) {
    try { if ((await fetch(BASE)).ok) return; } catch { /* not yet */ }
    await new Promise((r) => setTimeout(r, 250));
  }
  throw new Error("vite did not come up");
}

async function capture(browser, scene, dir) {
  const ctx = await browser.newContext({
    viewport: scene.viewport ?? { width: 840, height: 600 }, deviceScaleFactor: 2,
    locale: "en-US", timezoneId: "Europe/Lisbon", colorScheme: "dark",
  });
  await ctx.addInitScript((seeds) => {
    for (const [k, v] of Object.entries(seeds)) localStorage.setItem(k, JSON.stringify(v));
  }, SEEDS);
  const page = await ctx.newPage();
  // A fixed late-morning clock: world clock, clip ages and the calendar render the same every time.
  await page.clock.setFixedTime(new Date("2026-09-24T10:41:00+01:00"));
  await page.goto(scene.window ? `${BASE}?window=${scene.window}` : BASE);
  await page.waitForTimeout(1400);
  if (!scene.window) {
    const input = page.locator('input[placeholder^="Search"]').first();
    await input.fill("");
    if (scene.query) await input.type(scene.query, { delay: 8 });
  }
  const eggWait = scene.after !== undefined;
  if (eggWait) {
    // Games: Enter starts them, the intro plays, then a key lifts the "resumed" gate. Flappy starts
    // as soon as its word is typed, so there Enter itself lifts the gate once the game has mounted.
    if (scene.pre) await page.waitForTimeout(scene.pre);
    await page.keyboard.press("Enter");
    await page.waitForTimeout(scene.wait);
    for (const k of scene.keys ?? []) await page.keyboard.press(k);
    await page.waitForTimeout(scene.after);
  } else {
    for (const k of scene.keys ?? []) { await page.keyboard.press(k); await page.waitForTimeout(250); }
    await page.waitForTimeout(scene.wait ?? 1600);
  }
  const misses = await page.evaluate(() => [...(window.__mockMisses ?? [])]);
  const file = join(dir, `${scene.id}.png`);
  await page.screenshot({ path: file, omitBackground: true });
  await ctx.close();
  return { file, misses };
}

// Frame a raw capture: dark backdrop with the site's indigo glow, the window with a soft shadow.
function frameHtml(dataUrl, { width, height, bare }) {
  return `<!doctype html><html><head><style>
    html,body{margin:0;background:transparent}
    .stage{width:${width}px;height:${height}px;display:grid;place-items:center;overflow:hidden;
      background:radial-gradient(60% 70% at 18% 12%, rgba(99,102,241,.42), transparent 70%),
                 radial-gradient(55% 60% at 88% 92%, rgba(244,114,182,.20), transparent 70%),
                 linear-gradient(160deg,#141626,#0c0d11 60%);}
    img{width:${bare ? width : Math.round(width * 0.86)}px;display:block;
      ${bare ? "" : "border-radius:18px;box-shadow:0 40px 90px -20px rgba(0,0,0,.75),0 0 0 1px rgba(255,255,255,.07);"}}
  </style></head><body><div class="stage"><img src="${dataUrl}"></div></body></html>`;
}

async function frame(browser, raw, out, opts) {
  const ctx = await browser.newContext({ viewport: { width: opts.width, height: opts.height }, deviceScaleFactor: 2 });
  const page = await ctx.newPage();
  await page.setContent(frameHtml(`data:image/png;base64,${readFileSync(raw).toString("base64")}`, opts));
  await page.waitForTimeout(150);
  await page.locator(".stage").screenshot({ path: out });
  await ctx.close();
}

function encode(src, base, width) {
  execFileSync("cwebp", ["-quiet", "-q", "84", "-resize", String(width), "0", src, "-o", `${base}.webp`]);
  execFileSync("magick", [src, "-resize", `${width}x`, "-quality", "84", `${base}.jpg`]);
}

// ── main ──────────────────────────────────────────────────────────────────────
const only = new Set(process.argv.slice(2));
const want = (id) => only.size === 0 || only.has(id);
const { chromium } = loadPlaywright();
const vite = spawn("npx", ["vite", "--port", String(PORT), "--strictPort"], { cwd: FRONTEND, stdio: "ignore" });
const tmp = mkdtempSync(join(tmpdir(), "ir-mockups-"));
let failed = false;
try {
  await waitForServer();
  // Whatever Chrome is at hand: installed Google Chrome first, then Playwright's own build.
  let browser;
  for (const opts of [{ channel: "chrome" }, {}]) {
    try { browser = await chromium.launch(opts); break; } catch (e) { if (!Object.keys(opts).length) throw e; }
  }
  if (want("hero") && only.size) {
    const shots = [];
    for (const h of HERO) {
      const scene = { ...GALLERY.find((g) => g.id === h.from), id: h.id };
      const { file } = await capture(browser, scene, tmp);
      shots.push({ css: h.css, data: `data:image/png;base64,${readFileSync(file).toString("base64")}` });
    }
    const ctx = await browser.newContext({ viewport: { width: 2400, height: 1200 }, deviceScaleFactor: 1 });
    const page = await ctx.newPage();
    await page.setContent(heroHtml(shots));
    await page.waitForTimeout(200);
    await page.locator(".stage").screenshot({ path: HERO_OUT });
    await ctx.close();
    console.log(`hero → ${HERO_OUT}`);
  }
  for (const [set, scenes] of [["gallery", GALLERY], ["eggs", EGGS]]) {
    mkdirSync(join(OUT, set), { recursive: true });
    for (const scene of scenes.filter((s) => want(s.id))) {
      const { file, misses } = await capture(browser, scene, tmp);
      const framed = join(tmp, `${scene.id}-framed.png`);
      const bare = scene.id === "x";
      await frame(browser, file, framed, { width: 1120, height: 800, bare });
      encode(framed, join(OUT, set, scene.id), 1120);
      console.log(`${set}/${scene.id}${misses.length ? `  (unhandled: ${misses.join(", ")})` : ""}`);
    }
  }
  await browser.close();
} catch (e) {
  failed = true;
  console.error(e);
} finally {
  vite.kill();
  if (existsSync(tmp)) rmSync(tmp, { recursive: true, force: true });
}
process.exit(failed ? 1 : 0);
