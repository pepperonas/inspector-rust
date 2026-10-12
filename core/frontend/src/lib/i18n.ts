/**
 * App language — English / German.
 *
 * ONE catalogue for the whole app: `src/locales/{en,de}.json` (Rust embeds the
 * same files for the tray, toasts, notifications and reports). Keys are flat
 * dotted strings; `{name}` placeholders are filled from `vars`.
 *
 * Only the ACTIVE language is loaded — each catalogue is its own chunk, so
 * the entry bundle every window parses doesn't carry both. `main.tsx` awaits
 * `initI18n()` before the first render, so nothing ever paints in the wrong
 * language.
 *
 * Re-rendering on a switch: a component that shows text calls `useT()`,
 * which subscribes it to the language store. Pure helpers (formatters) may
 * call `t()` / `locale()` directly — they run inside a subscribed render.
 *
 * Rules for new UI text (pinned by `i18n.test.ts`):
 *   - never a bare string literal in JSX or a user-visible attribute — use `t()`;
 *   - every key exists in BOTH en.json and de.json, with the same placeholders;
 *   - command keywords and syntax are NOT translated (they are what you type).
 */

import { useSyncExternalStore } from "react";
import type EN from "../locales/en.json";

export type Lang = "en" | "de";
export type LangSetting = "system" | Lang;
export type MessageKey = keyof typeof EN;
type Vars = Record<string, string | number>;
type Catalog = Record<string, string>;

export interface LanguageInfo {
  setting: LangSetting;
  effective: Lang;
  system: Lang;
}

const loaders: Record<Lang, () => Promise<{ default: Catalog }>> = {
  en: () => import("../locales/en.json"),
  de: () => import("../locales/de.json"),
};

let lang: Lang = "en";
let catalog: Catalog = {};
let version = 0;
const listeners = new Set<() => void>();

/** `t` bound to the current catalogue. A NEW function per language, so a
 *  component that gets it from `useT()` can list it as a hook dependency —
 *  memos that build text recompute on a switch, and the hooks linter keeps
 *  that dependency honest. */
let bound: (key: MessageKey, vars?: Vars) => string = (key, vars) => t(key, vars);

function emit() {
  version++;
  bound = (key, vars) => t(key, vars);
  for (const l of listeners) l();
}

/** Install a catalogue directly (tests, and after a load). */
export function setCatalog(next: Lang, messages: Catalog): void {
  lang = next;
  catalog = messages;
  if (typeof document !== "undefined") document.documentElement.lang = next;
  emit();
}

export async function loadLanguage(next: Lang): Promise<void> {
  const mod = await loaders[next]();
  setCatalog(next, mod.default);
}

export function currentLang(): Lang {
  return lang;
}

/** BCP-47 locale for Intl formatters, following the app language. */
export function locale(): string {
  return lang === "de" ? "de-DE" : "en-US";
}

export function fill(template: string, vars?: Vars): string {
  if (!vars) return template;
  return template.replace(/\{(\w+)\}/g, (m, name: string) =>
    name in vars ? String(vars[name]) : m,
  );
}

/** Translate `key` in the current language. A missing key returns the key
 *  itself (visible, never a crash) — the parity test keeps that from shipping. */
export function t(key: MessageKey, vars?: Vars): string {
  const template = catalog[key];
  return fill(template ?? key, vars);
}

/** Split a translation around ONE placeholder that must be a React node
 *  (a live counter, a link): returns the text before and after `{slot}`, with
 *  every other placeholder filled. Word order stays the translator's call. */
export function tSplit(key: MessageKey, slot: string, vars?: Vars): [string, string] {
  const template = catalog[key] ?? key;
  const marker = `{${slot}}`;
  const at = template.indexOf(marker);
  if (at < 0) return [fill(template, vars), ""];
  return [fill(template.slice(0, at), vars), fill(template.slice(at + marker.length), vars)];
}

/** Plural form: looks up `${key}.one` for n === 1, else `${key}.other`.
 *  `{n}` is always available as a placeholder. */
export function tn(key: string, n: number, vars?: Vars): string {
  const k = `${key}.${n === 1 ? "one" : "other"}` as MessageKey;
  return t(k, { n, ...vars });
}

function subscribe(cb: () => void): () => void {
  listeners.add(cb);
  return () => listeners.delete(cb);
}

/** Subscribe the calling component to language switches and return `t` —
 *  a fresh function after every switch (see `bound`). */
export function useT(): typeof t {
  return useSyncExternalStore(subscribe, () => bound);
}

/** The current language, re-rendering on a switch. */
export function useLang(): Lang {
  useSyncExternalStore(subscribe, () => version);
  return lang;
}

/**
 * Resolve the stored preference and load its catalogue. Runs in EVERY window
 * before the first render. Outside Tauri (tests, the mockup) or when the IPC
 * fails, English is used. Live switches arrive as `language-changed`.
 */
export async function initI18n(): Promise<void> {
  let effective: Lang = "en";
  try {
    const { invoke } = await import("@tauri-apps/api/core");
    const info = await Promise.race([
      invoke<LanguageInfo>("get_language"),
      new Promise<never>((_, reject) => setTimeout(() => reject(new Error("timeout")), 1500)),
    ]);
    effective = info.effective;
    const { listen } = await import("@tauri-apps/api/event");
    void listen<LanguageInfo>("language-changed", (e) => {
      void loadLanguage(e.payload.effective);
    }).catch(() => {});
  } catch {
    // Not in Tauri / IPC unavailable — stay English.
  }
  await loadLanguage(effective);
}
