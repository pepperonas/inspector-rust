#!/usr/bin/env node
// Frontend bundle budget (PERFORMANCE-PLAN D3b). The eager App chunk is what
// the hidden popup parses + JITs at launch; A4 (v0.166.0) brought it from
// 952 KB down to ~363 KB by lazy-loading every panel/game/takeover. This guard
// keeps a new eager import from quietly growing it back — a lazy panel that
// someone imports statically "just for a type" lands here as a red check.
//
// Run after `pnpm --filter inspector-rust-frontend build` (scripts/check.sh
// does). Budgets are generous ceilings, not targets.
import { readdirSync, statSync } from "node:fs";
import { join } from "node:path";

const dist = new URL("../core/frontend/dist/assets/", import.meta.url).pathname;
const BUDGET_KB = { App: 400, entry: 340, css: 110 };

let files;
try {
  files = readdirSync(dist);
} catch {
  console.error(`check-bundle: no build at ${dist} — run the frontend build first`);
  process.exit(2);
}
const kb = (f) => Math.round(statSync(join(dist, f)).size / 1024);
const pick = (re) => files.filter((f) => re.test(f)).sort((a, b) => kb(b) - kb(a))[0];

const checks = [
  ["App", pick(/^App-.*\.js$/), BUDGET_KB.App],
  ["entry", pick(/^index-.*\.js$/), BUDGET_KB.entry],
  ["css", pick(/^index-.*\.css$/), BUDGET_KB.css],
];
let failed = false;
for (const [label, file, budget] of checks) {
  if (!file) {
    console.error(`check-bundle: ${label} chunk not found`);
    failed = true;
    continue;
  }
  const size = kb(file);
  const ok = size <= budget;
  console.log(`${ok ? "✓" : "✗"} ${label.padEnd(5)} ${String(size).padStart(5)} KB  (budget ${budget} KB)  ${file}`);
  if (!ok) failed = true;
}
if (failed) {
  console.error("check-bundle: over budget — an eager import crept into the start-up path? (see PERFORMANCE-PLAN.md A4)");
  process.exit(1);
}
