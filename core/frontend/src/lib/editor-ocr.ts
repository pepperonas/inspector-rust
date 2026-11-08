/**
 * Text-aware editing for the screenshot editor: find sensitive text in the
 * recognised lines (e-mail, phone, IBAN, card, keys, …) and snap a drag to the
 * words under it. Pure — the OCR itself runs in Rust (`editor_ocr_words`,
 * `ir-ocr` helper); its JSON comes in here.
 */

import type { Annotation, Rect } from "./editor-geometry";

/** [x, y, w, h], normalised 0..1, origin top-left. */
export type Box4 = [number, number, number, number];
/** One word: its UTF-16 range in the line text (= JS string indices). */
export type OcrWord = { s: number; e: number; b: Box4 };
export type OcrLine = { t: string; b: Box4; w: OcrWord[] };

export type FindingKind = "email" | "phone" | "iban" | "card" | "secret" | "ip" | "url-token";
export type Match = { kind: FindingKind; start: number; end: number };
export type Finding = { kind: FindingKind; text: string; rect: Rect };

const isNum = (v: unknown): v is number => typeof v === "number" && Number.isFinite(v);
const isBox = (v: unknown): v is Box4 => Array.isArray(v) && v.length === 4 && v.every(isNum);

/** The helper's JSON; malformed entries are dropped, garbage gives []. */
export function parseOcrLines(json: string): OcrLine[] {
  let raw: unknown;
  try {
    raw = JSON.parse(json);
  } catch {
    return [];
  }
  if (!Array.isArray(raw)) return [];
  const out: OcrLine[] = [];
  for (const l of raw) {
    if (!l || typeof l !== "object") continue;
    const { t, b, w } = l as Record<string, unknown>;
    if (typeof t !== "string" || !isBox(b) || !Array.isArray(w)) continue;
    const words: OcrWord[] = [];
    for (const x of w) {
      if (!x || typeof x !== "object") continue;
      const { s, e, b: wb } = x as Record<string, unknown>;
      if (isNum(s) && isNum(e) && e > s && isBox(wb)) words.push({ s, e, b: wb });
    }
    out.push({ t, b, w: words });
  }
  return out;
}

// ── Detection ────────────────────────────────────────────────────────

const RE_URL = /https?:\/\/[^\s"'<>]+/g;
const RE_URL_SECRET = /[?&#](?:access_token|id_token|token|api[_-]?key|apikey|key|secret|password|pass|pwd|sig|signature|auth|code)=/i;
const RE_KV =
  /\b(?:pass(?:wor[dt])?|pwd|secret|token|api[_-]?key|apikey|access[_-]?key|client[_-]?secret|auth)\s*[:=]\s*(["']?)(\S+?)\1(?=\s|$|[,;])/gi;
const RE_PREFIX =
  /\b(?:sk-[A-Za-z0-9_-]{16,}|sk_(?:live|test)_[A-Za-z0-9]{16,}|[pr]k_(?:live|test)_[A-Za-z0-9]{16,}|gh[pousr]_[A-Za-z0-9]{20,}|github_pat_[A-Za-z0-9_]{20,}|xox[abprs]-[A-Za-z0-9-]{10,}|AKIA[0-9A-Z]{16}|AIza[0-9A-Za-z_-]{35}|glpat-[A-Za-z0-9_-]{20,}|-----BEGIN[A-Z ]*PRIVATE KEY-----)/g;
const RE_JWT = /eyJ[A-Za-z0-9_-]{5,}\.eyJ[A-Za-z0-9_-]{5,}\.[A-Za-z0-9_-]{10,}/g;
const RE_EMAIL = /[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}/g;
const RE_IBAN = /\b[A-Z]{2}\d{2}(?: ?[A-Z0-9]){11,30}\b/g;
const RE_CARD = /\b(?:\d[ -]?){12,18}\d\b/g;
const RE_TOKEN = /(?<![A-Za-z0-9_\-+/=])[A-Za-z0-9_\-+/=]{24,}(?![A-Za-z0-9_\-+/=])/g;
const RE_IP = /\b(?:\d{1,3}\.){3}\d{1,3}\b/g;
// A phone number starts with +, 00 or 0 and is not the tail of a digit group
// (so "3704 0044 0532" inside a number is never read as "0044 …").
const RE_PHONE = /(?<!\d[ \-/]?)(?<![\w+])(?:\+|00|0)\d[\d \-/()]{4,}\d(?![\w])/g;

const digits = (s: string) => s.replace(/\D/g, "");

function ibanValid(compact: string): boolean {
  if (compact.length < 15 || compact.length > 34) return false;
  const moved = compact.slice(4) + compact.slice(0, 4);
  let rem = 0;
  for (const ch of moved) {
    const v = /\d/.test(ch) ? ch : String(ch.charCodeAt(0) - 55);
    for (const d of v) rem = (rem * 10 + Number(d)) % 97;
  }
  return rem === 1;
}

function luhn(ds: string): boolean {
  let sum = 0;
  for (let i = 0; i < ds.length; i++) {
    let d = Number(ds[ds.length - 1 - i]);
    if (i % 2 === 1) {
      d *= 2;
      if (d > 9) d -= 9;
    }
    sum += d;
  }
  return sum % 10 === 0;
}

function looksRandom(tok: string): boolean {
  if (!/[a-z]/.test(tok) || !/[A-Z]/.test(tok) || !/\d/.test(tok)) return false;
  return new Set(tok).size >= 12;
}

function each(re: RegExp, s: string, f: (m: RegExpExecArray) => void) {
  re.lastIndex = 0;
  let m: RegExpExecArray | null;
  while ((m = re.exec(s))) {
    f(m);
    if (m[0].length === 0) re.lastIndex++;
  }
}

/** Every sensitive span in `s`, each character claimed at most once
 *  (rules earlier in the list win), sorted by position. */
export function findSensitive(s: string): Match[] {
  const cands: Match[] = [];
  const add = (kind: FindingKind, start: number, end: number) => cands.push({ kind, start, end });

  each(RE_URL, s, (m) => {
    if (RE_URL_SECRET.test(m[0])) add("url-token", m.index, m.index + m[0].length);
  });
  each(RE_KV, s, (m) => {
    // The value (group 2) is followed only by its closing quote (group 1).
    const end = m.index + m[0].length - m[1].length;
    add("secret", end - m[2].length, end);
  });
  each(RE_PREFIX, s, (m) => add("secret", m.index, m.index + m[0].length));
  each(RE_JWT, s, (m) => add("secret", m.index, m.index + m[0].length));
  each(RE_EMAIL, s, (m) => add("email", m.index, m.index + m[0].length));
  each(RE_IBAN, s, (m) => {
    // Greedy may swallow a following capitalised word: take the longest prefix
    // that ends on a character and passes the checksum.
    for (let len = m[0].length; len >= 15; len--) {
      const part = m[0].slice(0, len);
      if (part.endsWith(" ")) continue;
      if (ibanValid(part.replace(/ /g, ""))) {
        add("iban", m.index, m.index + len);
        break;
      }
    }
  });
  each(RE_CARD, s, (m) => {
    const d = digits(m[0]);
    if (d.length >= 13 && d.length <= 19 && luhn(d)) add("card", m.index, m.index + m[0].length);
  });
  each(RE_TOKEN, s, (m) => {
    if (looksRandom(m[0])) add("secret", m.index, m.index + m[0].length);
  });
  each(RE_IP, s, (m) => {
    if (m[0].split(".").every((o) => Number(o) <= 255)) add("ip", m.index, m.index + m[0].length);
  });
  each(RE_PHONE, s, (m) => {
    const n = digits(m[0]).length;
    if (n >= 7 && n <= 15) add("phone", m.index, m.index + m[0].length);
  });

  const taken: Match[] = [];
  for (const c of cands) {
    if (!taken.some((t) => c.start < t.end && t.start < c.end)) taken.push(c);
  }
  return taken.sort((a, b) => a.start - b.start);
}

// ── Geometry ─────────────────────────────────────────────────────────

function toPx(b: Box4, W: number, H: number): Rect {
  return { x: b[0] * W, y: b[1] * H, w: b[2] * W, h: b[3] * H };
}

function union(rs: Rect[]): Rect {
  const x0 = Math.min(...rs.map((r) => r.x));
  const y0 = Math.min(...rs.map((r) => r.y));
  const x1 = Math.max(...rs.map((r) => r.x + r.w));
  const y1 = Math.max(...rs.map((r) => r.y + r.h));
  return { x: x0, y: y0, w: x1 - x0, h: y1 - y0 };
}

/** Grow a text box so the cover hides ascenders, descenders and anti-aliasing. */
function pad(r: Rect): Rect {
  const p = Math.max(2, Math.round(r.h * 0.15));
  return { x: r.x - p, y: r.y - p, w: r.w + 2 * p, h: r.h + 2 * p };
}

/** Sensitive text in image pixels (W×H = the source image), one rectangle
 *  per finding (a finding never spans lines — detection is per line). */
export function detectFindings(lines: OcrLine[], W: number, H: number): Finding[] {
  const out: Finding[] = [];
  for (const line of lines) {
    for (const m of findSensitive(line.t)) {
      const ws = line.w.filter((w) => w.s < m.end && w.e > m.start);
      // Every word of the match must have its box; if Vision left one out, a
      // cover built from the rest would leave part of the secret visible — so
      // cover the whole line instead of risking that.
      const boxed = tokens(line.t).filter(([s, e]) => s < m.end && e > m.start).every(([s, e]) =>
        ws.some((w) => w.s === s && w.e === e),
      );
      const rect = boxed && ws.length > 0 ? union(ws.map((w) => toPx(w.b, W, H))) : toPx(line.b, W, H);
      out.push({ kind: m.kind, text: line.t.slice(m.start, m.end), rect: pad(rect) });
    }
  }
  return out;
}

/** Whitespace-separated tokens as [start, end) — the helper's word split. */
function tokens(t: string): [number, number][] {
  const out: [number, number][] = [];
  for (const m of t.matchAll(/\S+/g)) out.push([m.index!, m.index! + m[0].length]);
  return out;
}

/** The words whose centre lies inside `drag`, as one padded box per line.
 *  [] when the drag covers no word (the caller keeps the plain rectangle). */
export function snapRectToWords(lines: OcrLine[], drag: Rect, W: number, H: number): Rect[] {
  const out: Rect[] = [];
  for (const line of lines) {
    const hit = line.w
      .map((w) => toPx(w.b, W, H))
      .filter((r) => {
        const cx = r.x + r.w / 2;
        const cy = r.y + r.h / 2;
        return cx >= drag.x && cx <= drag.x + drag.w && cy >= drag.y && cy <= drag.y + drag.h;
      });
    if (hit.length > 0) out.push(pad(union(hit)));
  }
  return out;
}

/** True when one of `existing` already covers `r` (3 px tolerance). */
export function isCovered(r: Rect, existing: Rect[], tol = 3): boolean {
  return existing.some(
    (e) => e.x <= r.x + tol && e.y <= r.y + tol && e.x + e.w >= r.x + r.w - tol && e.y + e.h >= r.y + r.h - tol,
  );
}

/** [kind, count] pairs, most frequent first (ties: first seen first). */
export function summarizeFindings(fs: Finding[]): [FindingKind, number][] {
  const order: FindingKind[] = [];
  const n = new Map<FindingKind, number>();
  for (const f of fs) {
    if (!n.has(f.kind)) order.push(f.kind);
    n.set(f.kind, (n.get(f.kind) ?? 0) + 1);
  }
  return order.map((k) => [k, n.get(k)!] as [FindingKind, number]).sort((a, b) => b[1] - a[1]);
}

/** A freshly drawn highlight or redaction over words becomes one tight box
 *  per line (same style, fresh ids). Anything else — another shape, or a drag
 *  over no word — comes back unchanged as `[a]`. */
export function snapDrawn(a: Annotation, lines: OcrLine[], W: number, H: number, nextId: () => string): Annotation[] {
  if (a.type !== "highlight" && a.type !== "redact") return [a];
  const rects = snapRectToWords(lines, { x: a.x, y: a.y, w: a.w, h: a.h }, W, H);
  if (rects.length === 0) return [a];
  return rects.map((r) => ({ ...a, id: nextId(), x: r.x, y: r.y, w: r.w, h: r.h }));
}

/** `fresh`: the findings that got a new block (`added[i]` covers `fresh[i]`). */
export type AutoRedact = { added: Annotation[]; findings: Finding[]; fresh: Finding[]; alreadyCovered: number };

/** Redaction blocks for every sensitive finding not yet covered by an existing
 *  redaction (a highlight or blur does not count — text stays readable). */
export function autoRedact(
  annotations: Annotation[],
  lines: OcrLine[],
  W: number,
  H: number,
  nextId: () => string,
): AutoRedact {
  const findings = detectFindings(lines, W, H);
  const covers: Rect[] = annotations.filter((a) => a.type === "redact").map((a) => a as Rect & Annotation);
  const added: Annotation[] = [];
  const fresh: Finding[] = [];
  let alreadyCovered = 0;
  for (const f of findings) {
    if (isCovered(f.rect, covers)) {
      alreadyCovered++;
      continue;
    }
    const a: Annotation = { id: nextId(), type: "redact", ...f.rect };
    added.push(a);
    fresh.push(f);
    covers.push(f.rect);
  }
  return { added, findings, fresh, alreadyCovered };
}
