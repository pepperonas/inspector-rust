import { useEffect, useRef, useState } from "react";
import { writeText } from "@tauri-apps/plugin-clipboard-manager";
import { TerminalSquare, Copy, Check, PlusCircle } from "lucide-react";
import { buildAliasSetups, validAliasName, type AliasSetup } from "../lib/alias";
import { CURRENT_PLATFORM } from "../lib/platform";
import { aliasCreate } from "../lib/ipc";

/**
 * `alias` — guided shell-alias builder (v0.127.0). Two inputs (command +
 * alias name), then one card per OS showing the exact terminal one-liner that
 * creates the alias there (macOS/zsh · Linux/bash · Windows/PowerShell), each
 * with a copy button. The current OS's card carries an extra "Anlegen" button
 * that writes the alias directly into the shell config via the `alias_create`
 * IPC (duplicate-refusing). Shows while typing (`sound` pattern) — pure UI,
 * no side effects until a button is pressed.
 */
export function AliasPanel({ arg, focused, onExit }: { arg: string; focused: boolean; onExit: () => void }) {
  const [name, setName] = useState("");
  const [cmd, setCmd] = useState("");
  const [copied, setCopied] = useState<string | null>(null);
  const [creating, setCreating] = useState(false);
  const [result, setResult] = useState<{ ok: boolean; msg: string } | null>(null);
  const nameRef = useRef<HTMLInputElement>(null);
  const cmdRef = useRef<HTMLInputElement>(null);

  // `alias gs` pre-fills the name once; focus jumps to the command field.
  const prefilledRef = useRef(false);
  useEffect(() => {
    const a = arg.trim();
    if (!prefilledRef.current && a) {
      prefilledRef.current = true;
      setName(a);
    }
  }, [arg]);

  // Hand focus to the first empty field when the panel takes keyboard focus.
  useEffect(() => {
    if (focused) (name ? cmdRef : nameRef).current?.focus();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [focused]);

  // Esc exits the panel — but never while a field would rather blur first
  // (the global fallback blurs a focused field on the first Esc anyway; this
  // handler only runs when the event reaches the window unconsumed).
  useEffect(() => {
    if (!focused) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== "Escape") return;
      const t = e.target as HTMLElement | null;
      if (t && (t.tagName === "INPUT" || t.tagName === "TEXTAREA")) {
        // First Esc in a field: blur it, keep the panel.
        e.preventDefault();
        e.stopPropagation();
        (t as HTMLInputElement).blur();
        return;
      }
      e.preventDefault();
      e.stopPropagation();
      onExit();
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [focused, onExit]);

  const nameOk = validAliasName(name);
  const ready = nameOk && cmd.trim().length > 0;
  const setups: AliasSetup[] = ready ? buildAliasSetups(name, cmd.trim()) : [];
  const currentOs = CURRENT_PLATFORM === "mac" ? "macos" : CURRENT_PLATFORM === "win" ? "windows" : "linux";

  const copy = (key: string, text: string) => {
    writeText(text)
      .then(() => {
        setCopied(key);
        window.setTimeout(() => setCopied((c) => (c === key ? null : c)), 1400);
      })
      .catch(() => undefined);
  };

  const create = () => {
    if (!ready || creating) return;
    setCreating(true);
    setResult(null);
    aliasCreate(name, cmd.trim())
      .then((msg) => setResult({ ok: true, msg }))
      .catch((e) => setResult({ ok: false, msg: String(e) }))
      .finally(() => setCreating(false));
  };

  const inputCls =
    "w-full rounded-lg border border-[var(--color-border)] bg-[var(--color-bg)] px-2.5 py-1.5 " +
    "text-[13px] text-[var(--color-fg)] outline-none focus:border-[var(--color-accent)]";

  return (
    <div className="flex h-full flex-col gap-3 overflow-y-auto p-4 text-[var(--color-fg)]">
      <div className="flex items-center gap-2 text-[13px] font-medium">
        <TerminalSquare size={15} className="text-[var(--color-accent)]" /> Alias anlegen
      </div>

      <label className="flex flex-col gap-1 text-[11px] text-[var(--color-muted)]">
        Terminal-Befehl
        <input
          ref={cmdRef}
          value={cmd}
          onChange={(e) => {
            setCmd(e.target.value);
            setResult(null);
          }}
          placeholder="git status"
          spellCheck={false}
          className={inputCls}
        />
      </label>
      <label className="flex flex-col gap-1 text-[11px] text-[var(--color-muted)]">
        Alias
        <input
          ref={nameRef}
          value={name}
          onChange={(e) => {
            setName(e.target.value);
            setResult(null);
          }}
          placeholder="gs"
          spellCheck={false}
          className={inputCls}
        />
        {name !== "" && !nameOk && (
          <span className="text-amber-500">
            Nur Buchstaben, Ziffern, „-“ und „_“ — beginnend mit Buchstabe oder „_“.
          </span>
        )}
      </label>

      {ready ? (
        <div className="flex flex-col gap-2">
          {setups.map((s) => {
            const isCurrent = s.os === currentOs;
            return (
              <div
                key={s.os}
                className={
                  "rounded-xl border p-2.5 " +
                  (isCurrent
                    ? "border-[var(--color-accent)] bg-[color-mix(in_srgb,var(--color-accent)_8%,transparent)]"
                    : "border-[var(--color-border)]")
                }
              >
                <div className="flex items-center justify-between gap-2">
                  <span className="text-[11px] font-medium">
                    {s.label} <span className="text-[var(--color-muted)]">→ {s.target}</span>
                    {isCurrent && <span className="ml-1 text-[10px] text-[var(--color-accent)]">dieses System</span>}
                  </span>
                  <div className="flex shrink-0 items-center gap-1">
                    {isCurrent && (
                      <button
                        type="button"
                        onClick={create}
                        disabled={creating}
                        className="md3-press flex items-center gap-1 rounded-md bg-[var(--color-accent)] px-2 py-1 text-[11px] font-medium text-[var(--color-accent-fg)] disabled:opacity-50"
                        title="Alias auf diesem System anlegen"
                      >
                        <PlusCircle size={12} /> {creating ? "Lege an…" : "Anlegen"}
                      </button>
                    )}
                    <button
                      type="button"
                      onClick={() => copy(s.os, s.command)}
                      className="md3-press flex items-center gap-1 rounded-md border border-[var(--color-border)] px-2 py-1 text-[11px] text-[var(--color-muted)] hover:text-[var(--color-fg)]"
                      title="Befehl kopieren"
                    >
                      {copied === s.os ? <Check size={12} className="text-emerald-500" /> : <Copy size={12} />}
                      {copied === s.os ? "Kopiert" : "Kopieren"}
                    </button>
                  </div>
                </div>
                <code className="mt-1.5 block overflow-x-auto whitespace-pre rounded-md bg-[var(--color-bg)] px-2 py-1.5 font-mono text-[11px] leading-snug">
                  {s.command}
                </code>
              </div>
            );
          })}
          {result && (
            <p className={"text-[11px] " + (result.ok ? "text-emerald-500" : "text-amber-500")}>{result.msg}</p>
          )}
        </div>
      ) : (
        <p className="text-[12px] text-[var(--color-muted)]">
          Befehl und Alias eintragen — darunter erscheint für macOS, Linux und Windows der genaue
          Terminal-Befehl, der den Alias anlegt (Kopieren oder direkt auf diesem System anlegen).
        </p>
      )}

      <p className="mt-auto pt-1 text-[11px] text-[var(--color-muted)]">
        Aliasse gelten im nächsten Terminal (oder nach <code>source</code>). Esc schließt.
      </p>
    </div>
  );
}
