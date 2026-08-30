#!/usr/bin/env node
// update-badges.mjs — recompute the README headline metrics (lines-of-code +
// unit-test counts) from the REAL sources and rewrite the badges in README.md
// and README.de.md in place. Idempotent: the patterns match whatever numbers
// are currently in the files, so re-running is always safe.
//
//   npm run update-badges        # or: node scripts/update-badges.mjs
//   (also runs automatically as the `posttest` hook)
//
// What it computes:
//   • Lines of code — all workspace Rust (`core/rust-lib/src` + the three
//     2-line platform shells) INCLUDING the file-final `#[cfg(test)]` modules,
//     plus every frontend `src/**/*.ts(x)` INCLUDING `*.test.ts(x)` — tests
//     are code we write and maintain (policy change v0.126.1; the count was
//     source-only before, which understated the repo by ~40k lines). Only the
//     generated `openers-data.ts` is excluded. node_modules / target / dist
//     never enter the count — only the explicit source dirs are scanned.
//   • Test counts — from the ACTUAL runners, not by grepping `it(` / `#[test]`
//     (which would miscount skips/todos): the summed `N passed` of every
//     `test result:` line of `cargo test --workspace`, and the `Tests N passed`
//     summary of a direct `vitest run`. ABORTS if either suite is red — a badge
//     must never advertise a passing suite that isn't.
//
// The runners are invoked DIRECTLY (`cargo test`, `vitest run`), never via the
// npm `test` script, so wiring this as `posttest` cannot recurse.
//
// Set IR_SKIP_BADGES=1 to make the posttest hook a no-op (fast local `npm test`).

import { execFileSync } from "node:child_process";
import { readdirSync, readFileSync, statSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..");

if (process.env.IR_SKIP_BADGES === "1") {
  console.log("update-badges: IR_SKIP_BADGES=1 → skipping.");
  process.exit(0);
}

// ── Line counting ────────────────────────────────────────────────────────────

/** Count lines the way `awk` does: a trailing newline does not add an empty
 *  record. Empty file → 0. */
function countLines(text) {
  if (text.length === 0) return 0;
  const parts = text.split("\n");
  if (parts[parts.length - 1] === "") parts.pop();
  return parts.length;
}

/** Recursively collect files under `dir` whose name passes `keep(name)`. */
function walk(dir, keep, out = []) {
  let entries;
  try {
    entries = readdirSync(dir, { withFileTypes: true });
  } catch {
    return out; // dir may not exist on a partial checkout
  }
  for (const e of entries) {
    const p = join(dir, e.name);
    if (e.isDirectory()) walk(p, keep, out);
    else if (e.isFile() && keep(e.name)) out.push(p);
  }
  return out;
}

/** Rust LOC: every line, test modules included (they're maintained code). */
function rustLoc() {
  const dirs = [
    "core/rust-lib/src",
    "win/src-tauri/src",
    "macos/src-tauri/src",
    "linux/src-tauri/src",
  ].map((d) => join(ROOT, d));
  let n = 0;
  for (const dir of dirs) {
    for (const file of walk(dir, (name) => name.endsWith(".rs"))) {
      n += countLines(readFileSync(file, "utf8"));
    }
  }
  return n;
}

/** Frontend LOC: all `.ts`/`.tsx` incl. tests, minus the generated openers. */
function frontendLoc() {
  const dir = join(ROOT, "core/frontend/src");
  let n = 0;
  for (const file of walk(
    dir,
    (name) =>
      (name.endsWith(".ts") || name.endsWith(".tsx")) &&
      name !== "openers-data.ts",
  )) {
    n += countLines(readFileSync(file, "utf8"));
  }
  return n;
}

// ── Test running ─────────────────────────────────────────────────────────────

// Runners colourise even when their output is a pipe (vitest does), and the
// escape sequences sit *between* the words we match on ("Tests \e[1m\e[32m907
// passed") — so strip them before any parsing.
const stripAnsi = (s) => s.replace(/\[[0-9;]*m/g, "");

function run(cmd, args, cwd) {
  try {
    return stripAnsi(
      execFileSync(cmd, args, {
        cwd,
        encoding: "utf8",
        stdio: ["ignore", "pipe", "pipe"],
        maxBuffer: 64 * 1024 * 1024,
      }),
    );
  } catch (err) {
    // A non-zero exit still carries the captured output on stdout/stderr.
    const out = stripAnsi(`${err.stdout ?? ""}${err.stderr ?? ""}`);
    return { failed: true, out };
  }
}

function rustTests() {
  const res = run("cargo", ["test", "--workspace"], ROOT);
  const out = typeof res === "string" ? res : res.out;
  if (typeof res !== "string" && res.failed) {
    console.error(out.split("\n").slice(-30).join("\n"));
    fail("Rust tests failed — badges NOT updated.");
  }
  // Sum "test result: ok. N passed" across every test binary.
  let sum = 0;
  let matched = false;
  for (const m of out.matchAll(/test result:\s+ok\.\s+(\d+)\s+passed/g)) {
    sum += Number(m[1]);
    matched = true;
  }
  if (!matched) fail("Could not parse cargo test output — badges NOT updated.");
  return sum;
}

function frontendTests() {
  // Direct `vitest run` (via the frontend workspace bin) — NOT `npm test`, so a
  // posttest hook can't recurse.
  const feDir = join(ROOT, "core/frontend");
  const res = run("npx", ["vitest", "run"], feDir);
  const out = typeof res === "string" ? res : res.out;
  if (typeof res !== "string" && res.failed) {
    console.error(out.split("\n").slice(-30).join("\n"));
    fail("Frontend tests failed — badges NOT updated.");
  }
  const m = out.match(/Tests\s+(\d+)\s+passed/);
  if (!m) fail("Could not parse vitest output — badges NOT updated.");
  return Number(m[1]);
}

function fail(msg) {
  console.error(`✗ ${msg}`);
  process.exit(1);
}

// ── Further repo metrics ─────────────────────────────────────────────────────
//
// ⚠️ Every badge added here must be COMPUTED, never typed. A hand-written
// count is a claim that quietly stops being true — which is worse than no
// badge at all, because a reader trusts it.

function countMetrics() {
  const read = (p) => readFileSync(join(ROOT, p), "utf8");

  // Documented commands = entries in the CommandDoc registry, the same
  // registry that generates the README matrix and the in-app help.
  const commands = (read("core/frontend/src/lib/commandDocs.ts").match(/^    command: "/gm) ?? [])
    .length;

  // Catalogue lines in features.txt — one line per feature, by contract.
  const features = read("features.txt").split("\n").filter((l) => l.trim().length > 0).length;

  // Reference pages under docs/.
  const docs = readdirSync(join(ROOT, "docs")).filter((f) => f.endsWith(".md")).length;

  // Rust modules in the core library (one file = one module).
  const modules = readdirSync(join(ROOT, "core/rust-lib/src")).filter((f) =>
    f.endsWith(".rs"),
  ).length;

  // Crates in the resolved dependency graph — the honest number for a Rust
  // binary, not the count of direct dependencies.
  const crates = (read("Cargo.lock").match(/^name = /gm) ?? []).length;

  return { commands, docs, modules, crates, features };
}

// ── Badge rewriting (idempotent) ─────────────────────────────────────────────

function rewriteBadges({ locK, rustK, tsK, total, rust, fe, commands, docs, modules, crates, features }) {
  const edits = {
    "README.md": (s) =>
      s
        .replace(/lines%20of%20code-~\d+k/g, `lines%20of%20code-~${locK}k`)
        .replace(/badge\/commands-\d+/g, `badge/commands-${commands}`)
        .replace(/badge\/docs-\d+%20pages/g, `badge/docs-${docs}%20pages`)
        .replace(/badge\/rust%20modules-\d+/g, `badge/rust%20modules-${modules}`)
        .replace(/badge\/crates-\d+/g, `badge/crates-${crates}`)
        .replace(/badge\/features-\d+/g, `badge/features-${features}`)
        // ⚠️ These three were hand-typed once and drifted — the exact failure
        // the header forbids. LOC policy includes tests (v0.126.1), so the
        // "source" label is gone; the split badges carry the same totals the
        // main badge sums.
        .replace(/badge\/Rust-~\d+k%20LoC/g, `badge/Rust-~${rustK}k%20LoC`)
        .replace(/badge\/TypeScript-~\d+k%20LoC/g, `badge/TypeScript-~${tsK}k%20LoC`)
        .replace(/unit%20tests-\d+%20passing/g, `unit%20tests-${total}%20passing`)
        .replace(/badge\/tests-\d+%20passing/g, `badge/tests-${total}%20passing`)
        // Quality-section per-runner badges (cargo test / vitest counts).
        .replace(/cargo%20test-\d+%20passing/g, `cargo%20test-${rust}%20passing`)
        .replace(/badge\/vitest-\d+%20passing/g, `badge/vitest-${fe}%20passing`)
        .replace(
          /unit%20tests-\d+%20\(\d+%20Rust%20%2B%20\d+%20TS\)/g,
          `unit%20tests-${total}%20(${rust}%20Rust%20%2B%20${fe}%20TS)`,
        )
        .replace(
          /\*\*\d+ unit tests \(\d+ Rust \+ \d+ frontend\)\.\*\*/g,
          `**${total} unit tests (${rust} Rust + ${fe} frontend).**`,
        )
        .replace(
          /title="Unit tests — \d+ Rust \+ \d+ frontend, all passing"/g,
          `title="Unit tests — ${rust} Rust + ${fe} frontend, all passing"`,
        )
        // Prose in the Tests section (EN "tests" / DE "Tests"): the frontend
        // count after "vitest + happy-dom) —" and the Rust count after the
        // "cargo test --workspace  # …" comment.
        .replace(/(vitest \+ happy-dom\) — )\d+( [Tt]ests)/g, `$1${fe}$2`)
        .replace(
          /(cargo test --workspace {2}# [^—\n]*— )\d+( [Tt]ests)/g,
          `$1${rust}$2`,
        ),
    "README.de.md": (s) =>
      s
        .replace(/lines%20of%20code-~\d+k/g, `lines%20of%20code-~${locK}k`)
        // ⚠️ The German README carries the SAME computed badges — these rules
        // were missing here, and docs-22/modules-84 sat beside the English
        // 24/87 for weeks. A rule that exists in only one language IS the
        // drift it was written to prevent.
        .replace(/badge\/commands-\d+/g, `badge/commands-${commands}`)
        .replace(/badge\/docs-\d+%20pages/g, `badge/docs-${docs}%20pages`)
        .replace(/badge\/docs-\d+%20Seiten/g, `badge/docs-${docs}%20Seiten`)
        .replace(/badge\/rust%20modules-\d+/g, `badge/rust%20modules-${modules}`)
        .replace(/badge\/crates-\d+/g, `badge/crates-${crates}`)
        .replace(/badge\/features-\d+/g, `badge/features-${features}`)
        .replace(/badge\/Rust-~\d+k%20LoC/g, `badge/Rust-~${rustK}k%20LoC`)
        .replace(/badge\/TypeScript-~\d+k%20LoC/g, `badge/TypeScript-~${tsK}k%20LoC`)
        .replace(/unit%20tests-\d+%20passing/g, `unit%20tests-${total}%20passing`)
        .replace(/badge\/tests-\d+%20passing/g, `badge/tests-${total}%20passing`)
        // Quality-section per-runner badges (cargo test / vitest counts).
        .replace(/cargo%20test-\d+%20passing/g, `cargo%20test-${rust}%20passing`)
        .replace(/badge\/vitest-\d+%20passing/g, `badge/vitest-${fe}%20passing`)
        .replace(
          /unit%20tests-\d+%20\(\d+%20Rust%20%2B%20\d+%20TS\)/g,
          `unit%20tests-${total}%20(${rust}%20Rust%20%2B%20${fe}%20TS)`,
        )
        .replace(
          /\*\*\d+ Unit-Tests \(\d+ Rust \+ \d+ Frontend\)\.\*\*/g,
          `**${total} Unit-Tests (${rust} Rust + ${fe} Frontend).**`,
        )
        .replace(
          /title="Unit-Tests — \d+ Rust \+ \d+ Frontend, alle grün"/g,
          `title="Unit-Tests — ${rust} Rust + ${fe} Frontend, alle grün"`,
        )
        // Tests-section prose (DE "Tests"): frontend + Rust counts.
        .replace(/(vitest \+ happy-dom\) — )\d+( [Tt]ests)/g, `$1${fe}$2`)
        .replace(
          /(cargo test --workspace {2}# [^—\n]*— )\d+( [Tt]ests)/g,
          `$1${rust}$2`,
        ),
  };

  let changed = 0;
  for (const [name, edit] of Object.entries(edits)) {
    const path = join(ROOT, name);
    let before;
    try {
      before = readFileSync(path, "utf8");
    } catch {
      continue; // README.de.md is optional
    }
    const after = edit(before);
    if (after !== before) {
      writeFileSync(path, after);
      changed++;
      console.log(`   updated ${name}`);
    } else {
      console.log(`   ${name} already current`);
    }
  }
  return changed;
}

// ── Main ─────────────────────────────────────────────────────────────────────

console.log("── Counting lines of code…");
const rLoc = rustLoc();
const fLoc = frontendLoc();
const loc = rLoc + fLoc;
const locK = Math.round(loc / 1000);
console.log(`   Rust ${rLoc} · Frontend ${fLoc} → ${loc} (~${locK}k)`);

console.log("── Running cargo test --workspace…");
const rust = rustTests();
console.log(`   Rust: ${rust} passed`);

console.log("── Running vitest run…");
const fe = frontendTests();
console.log(`   Frontend: ${fe} passed`);

const total = rust + fe;
console.log(
  `── Rewriting badges (~${locK}k LOC · ${total} tests = ${rust} Rust + ${fe} frontend)…`,
);
rewriteBadges({
  locK,
  rustK: Math.round(rLoc / 1000),
  tsK: Math.round(fLoc / 1000),
  total,
  rust,
  fe,
  ...countMetrics(),
});
console.log(
  `✓ Badges: ~${locK}k LOC · ${total} tests (${rust} Rust + ${fe} frontend).`,
);
