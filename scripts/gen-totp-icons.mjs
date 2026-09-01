#!/usr/bin/env node
/**
 * Generate core/frontend/src/lib/totp-icons.json from the pinned simple-icons
 * package (devDependency of core/frontend) — the brand-icon data the 2FA
 * overlay shows next to each issuer (v0.161.0).
 *
 * Same source + version as the "Hundert Portale" icon catalogue. The FULL set
 * (3457 icons) ships as a lazy-loaded JSON chunk rather than a curated subset:
 * curating would silently miss the next issuer the user adds, and the file
 * only loads when the 2FA overlay opens.
 *
 * Shape (kept lean — the path data is the bulk, everything else is derived at
 * runtime by lib/totp-icons.ts):
 *   { v: "<simple-icons version>",
 *     icons: { slug: [title, hex, path] },
 *     alias: { akaName: slug } }
 *
 * ⚠️ The aliases (aka) live ONLY in the package's data/simple-icons.json,
 * which is NOT an exported subpath — read it from disk. The JS exports carry
 * no alias info (verified against 16.29.0).
 *
 * Re-run after bumping the simple-icons devDependency:
 *   node scripts/gen-totp-icons.mjs
 */
import { createRequire } from "node:module";
import { readFileSync, writeFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";

const here = dirname(fileURLToPath(import.meta.url));
const feDir = join(here, "..", "core", "frontend");
const require = createRequire(join(feDir, "package.json"));

const si = require("simple-icons");
// ⚠️ "simple-icons/package.json" is not an exported subpath — read from disk.
const pkgDir = join(feDir, "node_modules", "simple-icons");
const version = JSON.parse(readFileSync(join(pkgDir, "package.json"), "utf8")).version;
const dataPath = join(pkgDir, "data", "simple-icons.json");
const data = JSON.parse(readFileSync(dataPath, "utf8"));

const icons = {};
let n = 0;
for (const key of Object.keys(si)) {
  const i = si[key];
  if (!i || typeof i !== "object" || !i.slug || !i.path) continue;
  icons[i.slug] = [i.title, i.hex, i.path];
  n++;
}

const alias = {};
for (const d of data) {
  const aka = d.aliases?.aka ?? [];
  const dup = (d.aliases?.dup ?? []).map((x) => x.title).filter(Boolean);
  for (const name of [...aka, ...dup]) {
    if (icons[d.slug]) alias[name] = d.slug;
  }
}

const out = { v: version, icons, alias };
const dest = join(feDir, "src", "lib", "totp-icons.json");
writeFileSync(dest, JSON.stringify(out));
console.log(
  `totp-icons.json: ${n} icons, ${Object.keys(alias).length} aliases, ` +
    `simple-icons ${version}, ${(JSON.stringify(out).length / 1024 / 1024).toFixed(2)} MB`,
);
if (n < 3000) {
  console.error("suspiciously few icons — refusing to ship a truncated set");
  process.exit(1);
}
