// Security command builders — pure parser + command builder + POSIX quoter.
//
// Everything here is frontend-side (no IPC per keystroke): the Rust registry
// (single source of truth) is fetched once via sec_catalog(); this file turns
// a preset + user input into a syntactically correct, shell-SAFE command line.
// Two outputs (App.tsx): Enter → clipboard, ⌘/Ctrl+Enter → terminal hand-off.
//
// The one thing that must be bullet-proof is the quoting: a target/param with
// a space, ;, &, $, `, ' or " must NEVER break the command — on the clipboard
// path and (where it actually reaches a shell) the hand-off path.

// ── Types (mirror the Rust serde shapes) ─────────────────────────────────────

export type Segment =
  | { kind: "lit"; text: string }
  | { kind: "field"; key: string }
  | { kind: "flag"; flag: string; key: string }
  | { kind: "joined"; prefix: string; key: string };

export interface FieldSpec {
  key: string;
  label: string;
  placeholder: string;
  help: string;
  required: boolean;
}

export interface PresetSpec {
  name: string;
  aliases: string[];
  purpose: string;
  segments: Segment[];
  fields: string[];
  sharp: boolean;
  tags: string[];
  category: string;
}

export interface ToolSpec {
  name: string;
  aliases: string[];
  binary: string;
  presets: PresetSpec[];
  fields: FieldSpec[];
  flag_help: [string, string][];
  notes: string[];
}

export interface SecCatalog {
  tools: ToolSpec[];
}

export interface SecDefaults {
  wordlist: string;
  output_dir: string;
  timing: string;
  threads: number;
  rate: number;
  john_line: "jumbo" | "core";
  terminal: "iterm" | "terminal";
  auto_enter: boolean;
  scope_note: string;
  save_history: boolean;
}

// ── POSIX shell quoting (shlex.quote-style; mirrors Rust sec::shell_quote) ────

/**
 * Quote a value for a POSIX sh/bash command line. A value made only of "safe"
 * characters is returned as-is (so `10.0.0.5` stays readable); anything else is
 * wrapped in single quotes with embedded `'` escaped as `'\''`. This makes any
 * user input inert: `a; rm -rf /` → `'a; rm -rf /'`, `O'Brien` → `'O'\''Brien'`.
 */
export function posixQuote(s: string): string {
  if (s === "") return "''";
  if (/^[A-Za-z0-9_@%+=:,./-]+$/.test(s)) return s;
  return "'" + s.replace(/'/g, "'\\''") + "'";
}

// ── Command builder ──────────────────────────────────────────────────────────

/** Value for a segment key: the user's value, else a Settings default for the
 *  default-backed keys (wordlist / timing / threads / rate). */
function resolveValue(
  key: string,
  values: Record<string, string>,
  defaults: SecDefaults,
): string {
  const v = (values[key] ?? "").trim();
  if (v) return v;
  switch (key) {
    case "wordlist":
      return defaults.wordlist.trim();
    case "timing":
      return defaults.timing.trim();
    case "threads":
      return defaults.threads > 0 ? String(defaults.threads) : "";
    case "rate":
      return defaults.rate > 0 ? String(defaults.rate) : "";
    default:
      return "";
  }
}

export interface BuiltCommand {
  command: string;
  /** Required fields with no value yet (rendered as ‹key› placeholders). */
  missing: string[];
}

/**
 * Build the command line for a preset from the collected field values. Literal
 * tokens go through verbatim; a required `field` with no value becomes a
 * ‹key› placeholder (so the command is still shown, amber, never a crash); an
 * optional `flag`/`joined` with no value is dropped. Every value is
 * `posixQuote`d.
 */
export function buildCommand(
  preset: PresetSpec,
  values: Record<string, string>,
  defaults: SecDefaults,
  requiredKeys: ReadonlySet<string> = new Set(),
): BuiltCommand {
  const parts: string[] = [];
  const missing: string[] = [];
  for (const seg of preset.segments) {
    if (seg.kind === "lit") {
      parts.push(seg.text);
      continue;
    }
    const raw = resolveValue(seg.key, values, defaults);
    const required = seg.kind === "field" || requiredKeys.has(seg.key);
    if (!raw) {
      // A required field (positional or a required flag like ferox -u/-w) is
      // shown as a ‹key› placeholder; an optional flag/joined is dropped.
      if (required) {
        if (seg.kind === "field") parts.push(`‹${seg.key}›`);
        else if (seg.kind === "flag") parts.push(seg.flag, `‹${seg.key}›`);
        else parts.push(`${seg.prefix}‹${seg.key}›`);
        missing.push(seg.key);
      }
      continue;
    }
    if (seg.kind === "field") parts.push(posixQuote(raw));
    else if (seg.kind === "flag") parts.push(seg.flag, posixQuote(raw));
    else parts.push(seg.prefix + posixQuote(raw));
  }
  return { command: parts.join(" "), missing };
}

/** Required field keys for a tool (drives placeholder-vs-drop in buildCommand). */
export function requiredKeys(tool: ToolSpec): Set<string> {
  return new Set(tool.fields.filter((f) => f.required).map((f) => f.key));
}

// ── Catalogue lookup + fuzzy ─────────────────────────────────────────────────

export function findTool(name: string, catalog: SecCatalog): ToolSpec | undefined {
  const n = name.toLowerCase();
  return (
    catalog.tools.find((t) => t.name === n) ??
    catalog.tools.find((t) => t.aliases.some((a) => a.toLowerCase() === n))
  );
}

/** Presets available for a tool given the John Core/Jumbo line + prepare mode. */
export function visiblePresets(
  tool: ToolSpec,
  defaults: SecDefaults,
  prepare: boolean,
): PresetSpec[] {
  return tool.presets.filter((p) => {
    const isPrepare = p.category === "prepare";
    if (prepare !== isPrepare) return false;
    // Core John hides Jumbo-only presets (mask + the *2john helpers).
    if (defaults.john_line === "core" && p.tags.includes("jumbo-only")) return false;
    return true;
  });
}

function presetScore(p: PresetSpec, q: string): number | null {
  const targets = [p.name, ...p.aliases];
  let best: number | null = null;
  for (const t of targets) {
    const s = scoreString(t, q);
    if (s != null && (best == null || s < best)) best = s;
  }
  if (best == null && p.purpose.toLowerCase().includes(q)) best = 500;
  return best;
}

function scoreString(target: string, q: string): number | null {
  const t = target.toLowerCase();
  if (t === q) return -1000;
  if (t.startsWith(q)) return -500 + t.length;
  if (t.includes(q)) return -100 + t.length;
  if (q.length >= 3 && t[0] === q[0] && isSubsequence(q, t)) return t.length;
  return null;
}

function isSubsequence(needle: string, hay: string): boolean {
  let i = 0;
  for (const c of hay) {
    if (c === needle[i]) i++;
    if (i === needle.length) return true;
  }
  return i === needle.length;
}

export function matchPresets(query: string, presets: PresetSpec[]): PresetSpec[] {
  const q = query.trim().toLowerCase();
  if (q === "") return [...presets];
  return presets
    .map((p) => ({ p, s: presetScore(p, q) }))
    .filter((x): x is { p: PresetSpec; s: number } => x.s != null)
    .sort((a, b) => a.s - b.s)
    .map((x) => x.p);
}

// ── Command parsing ──────────────────────────────────────────────────────────

export type SecParse =
  | { kind: "tool-overview" }
  | { kind: "preset-list"; tool: ToolSpec; query: string; prepare: boolean }
  | {
      kind: "built";
      tool: ToolSpec;
      preset: PresetSpec;
      command: string;
      missing: string[];
      sharp: boolean;
      tags: string[];
      flagHelp: [string, string][];
    }
  | { kind: "suggestion"; message: string }
  /** A bare tool keyword followed by prose that isn't a preset (`nmap output
   *  parsen`) — NOT a command; the caller surfaces nothing so history wins. */
  | { kind: "not-command" };

/** Flag explanations actually used by a preset (for the preview cheat-sheet). */
export function presetFlagHelp(tool: ToolSpec, preset: PresetSpec): [string, string][] {
  const used = new Set<string>();
  for (const seg of preset.segments) {
    if (seg.kind === "lit" && seg.text.startsWith("-")) used.add(seg.text);
    if (seg.kind === "flag") used.add(seg.flag);
    if (seg.kind === "joined") used.add(seg.prefix);
  }
  return tool.flag_help.filter(([f]) => used.has(f));
}

/**
 * Parse a `sec` / `<tool>` invocation. `keyword` is the matched command keyword
 * (`sec`, `nmap`, `ferox`, …); `arg` is everything after it. Argument order is
 * irrelevant: bare tokens fill the preset's positional fields, `--k=v` / `--k v`
 * set any field by key.
 */
export function parseSecCommand(
  keyword: string,
  arg: string,
  catalog: SecCatalog,
  defaults: SecDefaults,
): SecParse {
  const kw = keyword.toLowerCase();
  let toolName: string;
  let rest: string;
  // `sec …` is always an explicit command; a bare tool keyword (`nmap …`) only
  // engages the builder on clear intent (empty rest / a preset / `prepare`).
  const viaSec = kw === "sec";
  if (viaSec) {
    const trimmed = arg.trim();
    if (trimmed === "") return { kind: "tool-overview" };
    const sp = trimmed.indexOf(" ");
    toolName = sp === -1 ? trimmed : trimmed.slice(0, sp);
    rest = sp === -1 ? "" : trimmed.slice(sp + 1);
  } else {
    toolName = kw;
    rest = arg;
  }

  const tool = findTool(toolName, catalog);
  if (!tool) {
    return viaSec
      ? { kind: "suggestion", message: `Unknown tool '${toolName}' — try nmap, sqlmap, ferox, john` }
      : { kind: "not-command" };
  }

  const tokens = rest.trim().split(/\s+/).filter(Boolean);

  // Collision gate for the bare tool keyword: `nmap output parsen` (prose, not
  // a preset) must NOT take over — let the history search win.
  if (!viaSec && tokens.length > 0 && tokens[0].toLowerCase() !== "prepare") {
    const first = tokens[0].toLowerCase();
    const known =
      tool.presets.some((p) => p.name === first || p.aliases.includes(first)) ||
      matchPresets(tokens[0], visiblePresets(tool, defaults, false)).length > 0;
    if (!known) return { kind: "not-command" };
  }

  // `<tool> prepare` → the *2john helper list (John only, but generic).
  if (tokens[0]?.toLowerCase() === "prepare") {
    return { kind: "preset-list", tool, query: tokens.slice(1).join(" "), prepare: true };
  }

  // Resolve the preset from the first token.
  const presets = visiblePresets(tool, defaults, false);
  if (tokens.length === 0) {
    return { kind: "preset-list", tool, query: "", prepare: false };
  }
  const presetToken = tokens[0].toLowerCase();
  const preset =
    presets.find((p) => p.name === presetToken || p.aliases.includes(presetToken)) ??
    // Not an exact preset name yet → keep showing the (fuzzy) list.
    undefined;
  if (!preset) {
    return { kind: "preset-list", tool, query: tokens[0], prepare: false };
  }

  // Collect field values from the remaining tokens (order-free). Bare tokens
  // fill the preset's REQUIRED fields in segment order — whether they're
  // positional (`field`) or flag-based (ferox `-u`/`-w`); `--k=v` sets any key.
  const values: Record<string, string> = {};
  const req = requiredKeys(tool);
  const positional: string[] = [];
  for (const s of preset.segments) {
    if (s.kind !== "lit" && req.has(s.key) && !positional.includes(s.key)) {
      positional.push(s.key);
    }
  }
  let posIdx = 0;
  for (let i = 1; i < tokens.length; i++) {
    const tok = tokens[i];
    if (tok.startsWith("--")) {
      const body = tok.slice(2);
      const eq = body.indexOf("=");
      if (eq >= 0) {
        values[body.slice(0, eq)] = body.slice(eq + 1);
      } else if (i + 1 < tokens.length && !tokens[i + 1].startsWith("-")) {
        values[body] = tokens[++i];
      } else {
        values[body] = "true";
      }
    } else if (posIdx < positional.length) {
      values[positional[posIdx++]] = tok;
    }
  }

  const built = buildCommand(preset, values, defaults, requiredKeys(tool));
  return {
    kind: "built",
    tool,
    preset,
    command: built.command,
    missing: built.missing,
    sharp: preset.sharp,
    tags: preset.tags,
    flagHelp: presetFlagHelp(tool, preset),
  };
}
