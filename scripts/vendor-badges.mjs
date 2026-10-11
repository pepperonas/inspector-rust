#!/usr/bin/env node
// vendor-badges.mjs — store the README's STATIC shields.io badges as SVG files
// in the repo and point the READMEs at them.
//
// Why: the READMEs carry ~300 badges. GitHub loads every external image through
// its image proxy (camo), which gives up after ~4 s; with that many images at
// once a large share timed out (measured: 131 of 312 answered 504) and showed
// as broken images — a different set on every page load. A relative image is
// served by GitHub itself, without camo and without shields.io.
//
// Which badges stay on shields.io:
//   * every badge `scripts/update-badges.mjs` rewrites (its numbers change with
//     the code — the rules are read from that script's source, so a new
//     computed badge is kept remote automatically);
//   * every non-`/badge/` endpoint (`/github/...` — CI, release, stars …),
//     because those are live by definition.
//
// Usage:
//   node scripts/vendor-badges.mjs           # download new badges, rewrite READMEs
//   node scripts/vendor-badges.mjs --check   # exit 1 if a static badge is still remote
//
// Idempotent: a badge already vendored is not downloaded again. The file name
// is derived from the URL, so the same badge in both READMEs is one file.

import { readFileSync, writeFileSync, existsSync, mkdirSync } from "node:fs";
import { createHash } from "node:crypto";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = join(dirname(fileURLToPath(import.meta.url)), "..");
const READMES = ["README.md", "README.de.md"];
export const BADGE_DIR = ".github/badges";

// A static-badge URL. Balanced parentheses are allowed inside (one badge text
// contains "(2149 Rust + 3383 TS)"), anything else that ends a URL is not.
const BADGE_URL = /https:\/\/img\.shields\.io\/badge\/(?:[^\s"'()<>]|\([^\s"'()<>]*\))+/g;

/** The `badge/...` regexes update-badges.mjs rewrites, read from its source. */
export function computedPatterns(updateSrc) {
  const out = [];
  for (const m of updateSrc.matchAll(/\.replace\(\s*\/((?:\\\/|[^/\n])+)\/g/g)) {
    try {
      out.push(new RegExp(m[1]));
    } catch {
      // A regex we cannot rebuild here is skipped — the check below then
      // simply treats more badges as static, never fewer as computed.
    }
  }
  return out;
}

export const isComputed = (url, patterns) => patterns.some((re) => re.test(url));

/** Stable, readable file name for a badge URL. */
export function badgeFile(url) {
  const path = decodeURIComponent(url.replace(/^https:\/\/img\.shields\.io\/badge\//, "").split("?")[0]);
  const slug = path
    .normalize("NFKD")
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-+|-+$/g, "")
    .slice(0, 48)
    .replace(/-+$/g, "");
  const hash = createHash("sha1").update(url).digest("hex").slice(0, 8);
  return `${slug || "badge"}-${hash}.svg`;
}

async function download(url) {
  let lastErr;
  for (let attempt = 0; attempt < 4; attempt++) {
    try {
      const res = await fetch(url, { signal: AbortSignal.timeout(20000) });
      const body = await res.text();
      if (res.ok && body.trimStart().startsWith("<svg")) return body;
      lastErr = new Error(`HTTP ${res.status}`);
    } catch (e) {
      lastErr = e;
    }
    await new Promise((r) => setTimeout(r, 800 * (attempt + 1)));
  }
  throw new Error(`could not download ${url}: ${lastErr?.message}`);
}

async function main() {
  const check = process.argv.includes("--check");
  const patterns = computedPatterns(readFileSync(join(ROOT, "scripts/update-badges.mjs"), "utf8"));
  if (patterns.length < 10) throw new Error(`read only ${patterns.length} rules from update-badges.mjs — refusing`);

  const texts = Object.fromEntries(READMES.map((f) => [f, readFileSync(join(ROOT, f), "utf8")]));
  const statics = new Set();
  for (const t of Object.values(texts)) {
    for (const [url] of t.matchAll(BADGE_URL)) if (!isComputed(url, patterns)) statics.add(url);
  }

  if (check) {
    if (statics.size) {
      console.error(`vendor-badges: ${statics.size} static badge(s) still load from shields.io — run node scripts/vendor-badges.mjs`);
      for (const u of statics) console.error("  " + u);
      process.exit(1);
    }
    console.log("vendor-badges: all static badges are vendored.");
    return;
  }

  mkdirSync(join(ROOT, BADGE_DIR), { recursive: true });
  const todo = [...statics].filter((u) => !existsSync(join(ROOT, BADGE_DIR, badgeFile(u))));
  let next = 0;
  await Promise.all(
    Array.from({ length: 6 }, async () => {
      while (next < todo.length) {
        const url = todo[next++];
        writeFileSync(join(ROOT, BADGE_DIR, badgeFile(url)), await download(url));
      }
    }),
  );

  for (const [f, t] of Object.entries(texts)) {
    const out = t.replace(BADGE_URL, (url) => (isComputed(url, patterns) ? url : `${BADGE_DIR}/${badgeFile(url)}`));
    if (out !== t) writeFileSync(join(ROOT, f), out);
  }
  console.log(`vendor-badges: ${statics.size} static badges vendored (${todo.length} downloaded) into ${BADGE_DIR}/.`);
}

if (process.argv[1] && fileURLToPath(import.meta.url) === process.argv[1]) {
  main().catch((e) => {
    console.error(e.message);
    process.exit(1);
  });
}
