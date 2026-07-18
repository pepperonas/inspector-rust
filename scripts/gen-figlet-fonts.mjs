#!/usr/bin/env node
// gen-figlet-fonts.mjs — vendor the FIGlet .flf font set into the repo and
// derive its metadata + license manifest. DEV-TIME script (run once when
// refreshing the bundle), NOT part of the build. Source = a directory of .flf
// files (e.g. pyfiglet's `pyfiglet/fonts/`, a mature MIT project that already
// assembled a redistributable collection).
//
//   node scripts/gen-figlet-fonts.mjs <source-fonts-dir>
//
// It (1) copies each .flf into core/rust-lib/assets/figlet-fonts/, EXCLUDING any
// whose header carries a redistribution-hostile clause; (2) writes THIRDPARTY-
// FONTS (per-font attribution harvested from the .flf comment header + the
// redistribution posture + the exclusion list); (3) writes the metadata overlay
// assets/figlet-fonts.categories.json (category + popular flag per font — the
// Single Source of Truth the frontend reads via IPC, never the font bytes).
//
// build.rs then compresses each committed .flf and the app inflates lazily.

import { readdirSync, readFileSync, writeFileSync, mkdirSync, rmSync } from "node:fs";
import { join, basename, resolve, dirname } from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const OUT_DIR = join(ROOT, "core/rust-lib/assets/figlet-fonts");
const CATS_JSON = join(ROOT, "core/rust-lib/assets/figlet-fonts.categories.json");
const THIRDPARTY = join(ROOT, "THIRDPARTY-FONTS");

const src = process.argv[2];
if (!src) {
  console.error("usage: node scripts/gen-figlet-fonts.mjs <source-fonts-dir>");
  process.exit(2);
}

// A header comment matching any of these is treated as NOT redistributable and
// excluded (documented in THIRDPARTY-FONTS). pyfiglet's set carries none, but
// the rule is a required guard — we redistribute these fonts.
const HOSTILE =
  /\b(may not (be )?(re)?distribut|not (be )?(re)?distribut|do not distribute|for personal use only|shareware|not for (commercial|redistribution))/i;

// Curated "popular" subset — surfaces first in the gallery (the long tail is
// reached by fuzzy search). Confirmed present + reliably rendering + distinct.
const POPULAR = new Set([
  "standard", "slant", "small", "big", "banner", "block", "ansi_shadow", "ansi_regular",
  "doom", "ogre", "larry3d", "epic", "isometric1", "colossal", "cyberlarge", "cybermedium",
  "digital", "mini", "lean", "3d-ascii", "alligator", "avatar", "basic", "bell",
  "big_money-ne", "bloody", "bulbhead", "chunky", "contessa", "cosmic", "cricket", "doh",
  "dos_rebel", "drpepper", "electronic", "fender", "fuzzy", "ghost", "gothic", "graffiti",
  "impossible", "ivrit", "jazmine", "lcd", "letters", "nancyj", "o8", "delta_corps_priest_1",
]);

// Explicit category for well-known fonts; the rest fall to the heuristic below.
const CATEGORY = new Map(
  Object.entries({
    standard: "standard", banner: "banner", banner3: "banner", "banner3-D": "banner", banner4: "banner",
    big: "block", block: "block", blocks: "block", blocky: "block", colossal: "block", doh: "block",
    dos_rebel: "block", ansi_regular: "block", ansi_shadow: "block", isometric1: "block",
    isometric2: "block", isometric3: "block", isometric4: "block",
    slant: "slanted", "3d-ascii": "slanted", "3d_diagonal": "slanted", larry3d: "slanted",
    henry_3d: "slanted", italic: "slanted", nancyj: "slanted",
    mini: "small", small: "small", cybersmall: "small", digital: "small", lcd: "small", o8: "small",
    morse: "small", "3x5": "small", "5x7": "small", "5x8": "small", tiny: "small",
    script: "script", nscript: "script", jazmine: "script", fraktur: "script", cursive: "script",
    caligraphy: "script", graceful: "script", cybermedium: "script",
    doom: "decorative", ogre: "decorative", epic: "decorative", fender: "decorative",
    bloody: "decorative", ghost: "decorative", graffiti: "decorative", bulbhead: "decorative",
    contessa: "decorative", cosmic: "decorative", cricket: "decorative", drpepper: "decorative",
    electronic: "decorative", fuzzy: "decorative", avatar: "decorative", basic: "decorative",
    bell: "decorative", chunky: "decorative", alligator: "decorative", impossible: "decorative",
    ivrit: "decorative", letters: "decorative", cyberlarge: "decorative", gothic: "decorative",
  }),
);

function heuristicCategory(name) {
  const n = name.toLowerCase();
  if (/banner/.test(n)) return "banner";
  if (/(^|_|-)(3d|iso)/.test(n) || /isometric/.test(n)) return "block";
  if (/block|blocky/.test(n)) return "block";
  if (/script|cursive|calig|italic|jazmine|nscript|fraktur/.test(n)) return "script";
  if (/mini|small|tiny|^\d+x\d+$|^cl[rb]?\d|^cli\d/.test(n)) return "small";
  if (/banner|colossal|doh/.test(n)) return "block";
  return "other";
}

/** Count lines the way Rust's `str::lines()` does (used by figlet-rs): split on
 *  `\n`, strip a `\r`, and absorb exactly one trailing newline. */
function rustLines(text) {
  const lines = text.replace(/\r/g, "").split("\n");
  if (lines.length > 0 && lines[lines.length - 1] === "") lines.pop();
  return lines;
}

/** figlet-rs's `.flf` parser rejects a font whose code-tag section (the
 *  extended À/é/Cyrillic/… glyphs after the required 102 chars) isn't a clean
 *  multiple of `height+1` lines — stricter than real figlet, and it trips ~11
 *  fonts including `standard`. We keep the font by dropping a MALFORMED code-tag
 *  tail (the required 95 ASCII + 7 German chars survive; rarer accents become a
 *  gracefully-reported "unsupported char"). A well-formed code-tag section is
 *  left intact, so fonts figlet-rs *can* parse keep their extended glyphs. */
function normalizeFlf(text) {
  const lines = rustLines(text);
  const fields = (lines[0] ?? "").trim().split(/\s+/);
  const height = parseInt(fields[1], 10);
  const commentLines = parseInt(fields[5], 10);
  if (!Number.isFinite(height) || height <= 0 || !Number.isFinite(commentLines)) return text;
  const requiredEnd = 1 + commentLines + 102 * height; // header + comments + 102 chars
  const codetag = lines.length - requiredEnd;
  if (codetag <= 0) return text; // no code-tag section
  if (codetag % (height + 1) === 0) return text; // well-formed → keep intact
  // Malformed tail → truncate to the required section (trailing newline kept).
  return lines.slice(0, requiredEnd).join("\n") + "\n";
}

/** Extract the comment/attribution lines from a .flf header. Line 1 is
 *  `flf2a<hardblank> h b maxlen oldlayout commentlines …`; the next
 *  `commentlines` lines are the comment. */
function harvestComment(text) {
  const lines = text.split(/\r?\n/);
  const header = lines[0] ?? "";
  const fields = header.trim().split(/\s+/);
  const commentCount = parseInt(fields[5], 10);
  if (!Number.isFinite(commentCount) || commentCount <= 0) return "";
  return lines
    .slice(1, 1 + commentCount)
    .map((l) => l.trim())
    .filter(Boolean)
    .join(" ");
}

/** Read a .flf as UTF-8 text, normalising Latin-1 fonts (many .flf are
 *  ISO-8859-1, e.g. "standard") to UTF-8 so figlet-rs's `from_content` (which
 *  takes a UTF-8 `&str`) can parse them. Detect: if the bytes don't round-trip
 *  through strict UTF-8, decode as Latin-1 (1 byte → 1 code point, lossless).
 *  Returns the UTF-8 text to WRITE (bytes preserved for real UTF-8 fonts,
 *  transcoded for Latin-1 ones). */
function readFlfAsUtf8(buf) {
  const asUtf8 = buf.toString("utf8");
  if (Buffer.from(asUtf8, "utf8").equals(buf)) return asUtf8; // already valid UTF-8
  return buf.toString("latin1"); // Latin-1 → JS string → written back as UTF-8
}

const files = readdirSync(src).filter((f) => f.endsWith(".flf"));
if (files.length === 0) {
  console.error(`gen-figlet-fonts: no .flf files in ${src}`);
  process.exit(1);
}

rmSync(OUT_DIR, { recursive: true, force: true });
mkdirSync(OUT_DIR, { recursive: true });

const kept = [];
const excluded = [];
for (const file of files.sort()) {
  const name = basename(file, ".flf");
  const raw = readFlfAsUtf8(readFileSync(join(src, file)));
  if (!raw.startsWith("flf2a")) {
    excluded.push({ name, reason: "not a flf2a font" });
    continue;
  }
  const comment = harvestComment(raw);
  if (HOSTILE.test(comment)) {
    excluded.push({ name, reason: "header forbids redistribution" });
    continue;
  }
  const text = normalizeFlf(raw); // drop a malformed code-tag tail if present
  writeFileSync(join(OUT_DIR, file), text); // UTF-8 (Latin-1 sources transcoded)
  const category = CATEGORY.get(name) ?? heuristicCategory(name);
  kept.push({ name, category, popular: POPULAR.has(name), comment });
}

// Metadata overlay (no bytes) — the frontend's Single Source of Truth.
kept.sort((a, b) => a.name.localeCompare(b.name));
writeFileSync(
  CATS_JSON,
  JSON.stringify(
    kept.map(({ name, category, popular }) => ({ name, category, popular })),
    null,
    0,
  ) + "\n",
);

// THIRDPARTY-FONTS — the license manifest we ship with the fonts.
const catCounts = {};
for (const k of kept) catCounts[k.category] = (catCounts[k.category] ?? 0) + 1;
const manifest = [
  "# Third-party FIGlet fonts",
  "",
  `Inspector Rust bundles ${kept.length} FIGlet \`.flf\` fonts to power the`,
  "`figlet` command. They are vendored from the pyfiglet project",
  "(https://github.com/pwaller/pyfiglet, MIT), which assembled and redistributes",
  "this collection.",
  "",
  "## Redistribution posture",
  "",
  "Per the pyfiglet maintainers, any legal constraint on these fonts is",
  "considered long expired (public domain); the collection is redistributed",
  "accordingly. Each font's own attribution/comment header is reproduced below.",
  "During vendoring, any font whose header carries a redistribution-hostile",
  "clause (may-not-distribute / personal-use-only / shareware) is EXCLUDED.",
  "If you own a font here and want it removed, open an issue.",
  "",
  `Generated by scripts/gen-figlet-fonts.mjs. Fonts kept: ${kept.length}. Excluded: ${excluded.length}.`,
  "",
  "## Categories",
  "",
  ...Object.entries(catCounts)
    .sort()
    .map(([c, n]) => `- ${c}: ${n}`),
  "",
  "## Excluded fonts",
  "",
  excluded.length === 0
    ? "_None — no bundled font's header forbids redistribution._"
    : excluded.map((e) => `- \`${e.name}\` — ${e.reason}`).join("\n"),
  "",
  "## Per-font attribution",
  "",
  ...kept.map((k) => `- **${k.name}** — ${k.comment || "(no attribution in header)"}`),
  "",
].join("\n");
writeFileSync(THIRDPARTY, manifest);

console.log(
  `gen-figlet-fonts: kept ${kept.length}, excluded ${excluded.length} → ${OUT_DIR}`,
);
console.log(`  categories: ${Object.entries(catCounts).map(([c, n]) => `${c}=${n}`).join(", ")}`);
console.log(`  wrote ${CATS_JSON} + ${THIRDPARTY}`);
