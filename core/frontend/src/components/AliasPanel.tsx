import { useCallback, useEffect, useRef, useState } from "react";
import { writeText } from "@tauri-apps/plugin-clipboard-manager";
import { TerminalSquare, Copy, Check, PlusCircle, Pencil, Trash2, Search, X } from "lucide-react";
import { buildAliasSetups, validAliasName, filterAliases, type AliasSetup } from "../lib/alias";
import { CURRENT_PLATFORM } from "../lib/platform";
import { aliasCreate, aliasList, aliasDelete, type AliasEntry } from "../lib/ipc";

/**
 * `alias` — guided shell-alias builder + manager (v0.128.0). Top: two inputs
 * (command + alias name) → one card per OS with the exact create one-liner
 * (copy buttons; the current OS's card creates directly). Below: the aliases
 * already defined in this machine's rc file — searchable, alphabetical, with
 * edit (fills the builder; the create button flips to "Aktualisieren") and a
 * two-stage inline delete (never `window.confirm` — the TOTP lesson).
 * Taking focus (Enter or Tab-autocomplete on the command) selects the COMMAND
 * field so typing starts immediately.
 */
export function AliasPanel({ arg, focused, onExit }: { arg: string; focused: boolean; onExit: () => void }) {
  const [name, setName] = useState("");
  const [cmd, setCmd] = useState("");
  const [copied, setCopied] = useState<string | null>(null);
  const [creating, setCreating] = useState(false);
  const [result, setResult] = useState<{ ok: boolean; msg: string } | null>(null);
  const [list, setList] = useState<AliasEntry[] | null>(null);
  const [search, setSearch] = useState("");
  const [confirmDel, setConfirmDel] = useState<string | null>(null);
  const nameRef = useRef<HTMLInputElement>(null);
  const cmdRef = useRef<HTMLInputElement>(null);
  const aliveRef = useRef(true);

  useEffect(() => {
    aliveRef.current = true;
    return () => {
      aliveRef.current = false;
    };
  }, []);

  // The existing aliases from this machine's rc file (empty array on Windows).
  const refresh = useCallback(() => {
    aliasList()
      .then((l) => {
        if (aliveRef.current) setList(l);
      })
      .catch(() => {
        if (aliveRef.current) setList([]);
      });
  }, []);
  useEffect(() => {
    refresh();
  }, [refresh]);

  // `alias gs` pre-fills the name once.
  const prefilledRef = useRef(false);
  useEffect(() => {
    const a = arg.trim();
    if (!prefilledRef.current && a) {
      prefilledRef.current = true;
      setName(a);
    }
  }, [arg]);

  // Taking keyboard focus (Enter on the row / Tab-autocomplete) selects the
  // COMMAND field — the first thing to type is what the alias should run.
  useEffect(() => {
    if (!focused) return;
    const el = cmdRef.current;
    el?.focus();
    el?.select();
  }, [focused]);

  // Esc: blur a focused field first (back step), then exit the panel.
  useEffect(() => {
    if (!focused) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== "Escape") return;
      const t = e.target as HTMLElement | null;
      if (t && (t.tagName === "INPUT" || t.tagName === "TEXTAREA")) {
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
  // An existing name flips the create button into an update — `overwrite` is
  // passed to the backend, whose race guard still refuses a definition that
  // appeared since the last list refresh.
  const exists = (list ?? []).some((e) => e.name === name.trim());

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
    aliasCreate(name, cmd.trim(), exists)
      .then((msg) => {
        if (!aliveRef.current) return;
        setResult({ ok: true, msg });
        refresh();
      })
      .catch((e) => {
        if (aliveRef.current) setResult({ ok: false, msg: String(e) });
      })
      .finally(() => {
        if (aliveRef.current) setCreating(false);
      });
  };

  const edit = (e: AliasEntry) => {
    setName(e.name);
    setCmd(e.command);
    setResult(null);
    setConfirmDel(null);
    requestAnimationFrame(() => {
      cmdRef.current?.focus();
      cmdRef.current?.select();
    });
  };

  const remove = (aliasName: string) => {
    aliasDelete(aliasName)
      .then((msg) => {
        if (!aliveRef.current) return;
        setResult({ ok: true, msg });
        setConfirmDel(null);
        refresh();
      })
      .catch((e) => {
        if (!aliveRef.current) return;
        setResult({ ok: false, msg: String(e) });
        setConfirmDel(null);
        refresh();
      });
  };

  const shown = filterAliases(list ?? [], search);
  const manageable = currentOs !== "windows";

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
                        title={exists ? "Bestehenden Alias ersetzen" : "Alias auf diesem System anlegen"}
                      >
                        <PlusCircle size={12} /> {creating ? "Lege an…" : exists ? "Aktualisieren" : "Anlegen"}
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
        </div>
      ) : (
        <p className="text-[12px] text-[var(--color-muted)]">
          Befehl und Alias eintragen — darunter erscheint für macOS, Linux und Windows der genaue
          Terminal-Befehl, der den Alias anlegt (Kopieren oder direkt auf diesem System anlegen).
        </p>
      )}

      {result && (
        <p className={"text-[11px] " + (result.ok ? "text-emerald-500" : "text-amber-500")}>{result.msg}</p>
      )}

      {/* ── Existing aliases ─────────────────────────────────────────── */}
      {manageable && (
        <div className="flex flex-col gap-2 border-t border-[var(--color-border)] pt-3">
          <div className="flex items-center justify-between gap-2">
            <span className="text-[12px] font-medium">
              Bestehende Aliasse{list !== null && ` (${list.length})`}
            </span>
          </div>
          {(list?.length ?? 0) > 0 && (
            <div className="relative">
              <Search size={12} className="pointer-events-none absolute left-2 top-1/2 -translate-y-1/2 text-[var(--color-muted)]" />
              <input
                value={search}
                onChange={(e) => setSearch(e.target.value)}
                placeholder="Suchen…"
                spellCheck={false}
                className={inputCls + " pl-6"}
              />
              {search !== "" && (
                <button
                  type="button"
                  onClick={() => setSearch("")}
                  className="absolute right-1.5 top-1/2 -translate-y-1/2 rounded p-0.5 text-[var(--color-muted)] hover:text-[var(--color-fg)]"
                  title="Suche leeren"
                >
                  <X size={12} />
                </button>
              )}
            </div>
          )}
          {list === null ? (
            <p className="text-[11px] text-[var(--color-muted)]">Lese Shell-Config…</p>
          ) : list.length === 0 ? (
            <p className="text-[11px] text-[var(--color-muted)]">Noch keine Aliasse in der Shell-Config.</p>
          ) : shown.length === 0 ? (
            <p className="text-[11px] text-[var(--color-muted)]">Kein Alias passt zu „{search}“.</p>
          ) : (
            <div className="flex flex-col">
              {shown.map((e) => (
                <div
                  key={e.name}
                  className="group flex items-center gap-2 rounded-lg px-1.5 py-1 hover:bg-[var(--color-surface)]"
                >
                  <code className="shrink-0 font-mono text-[12px] font-medium text-[var(--color-accent)]">
                    {e.name}
                  </code>
                  <span className="min-w-0 flex-1 truncate font-mono text-[11px] text-[var(--color-muted)]" title={e.command}>
                    {e.command}
                  </span>
                  {confirmDel === e.name ? (
                    <span className="flex shrink-0 items-center gap-1">
                      <span className="text-[10px] text-rose-400">Löschen?</span>
                      <button
                        type="button"
                        onClick={() => remove(e.name)}
                        className="rounded p-1 text-rose-400 hover:bg-rose-500/15"
                        title="Ja, löschen"
                      >
                        <Check size={12} />
                      </button>
                      <button
                        type="button"
                        onClick={() => setConfirmDel(null)}
                        className="rounded p-1 text-[var(--color-muted)] hover:text-[var(--color-fg)]"
                        title="Abbrechen"
                      >
                        <X size={12} />
                      </button>
                    </span>
                  ) : (
                    <span className="flex shrink-0 items-center gap-0.5 opacity-0 transition-opacity group-hover:opacity-100">
                      <button
                        type="button"
                        onClick={() => edit(e)}
                        className="rounded p-1 text-[var(--color-muted)] hover:text-[var(--color-fg)]"
                        title="Bearbeiten (füllt die Felder oben)"
                      >
                        <Pencil size={12} />
                      </button>
                      <button
                        type="button"
                        onClick={() => setConfirmDel(e.name)}
                        className="rounded p-1 text-[var(--color-muted)] hover:text-rose-400"
                        title="Löschen"
                      >
                        <Trash2 size={12} />
                      </button>
                    </span>
                  )}
                </div>
              ))}
            </div>
          )}
        </div>
      )}
      {!manageable && (
        <p className="border-t border-[var(--color-border)] pt-3 text-[11px] text-[var(--color-muted)]">
          Die Verwaltung bestehender Aliasse gibt es auf macOS/Linux (liest die Shell-Config).
        </p>
      )}

      <p className="mt-auto pt-1 text-[11px] text-[var(--color-muted)]">
        Aliasse gelten im nächsten Terminal (oder nach <code>source</code>). Esc schließt.
      </p>
    </div>
  );
}
