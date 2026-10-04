import { useCallback, useEffect, useState } from "react";
import { Bot, Check, KeyRound, Loader2, Terminal, X } from "lucide-react";
import { aiGetConfig, aiProviderStatus, aiSetConfig, aiSetKey, aiTestProvider } from "../lib/ipc";
import type { AiConfig, AiProviderId, AiProviderStatus } from "../lib/tasks";

/**
 * Settings → KI-Anbieter: API keys (keychain, never shown again — only a
 * `••••abcd` hint), the model per provider, the provider new tasks use, and a
 * "Testen" round trip. Claude can run either through the Anthropic API (key)
 * or through the local `claude` program with its own login.
 */
export function AiProvidersSection() {
  const [list, setList] = useState<AiProviderStatus[] | null>(null);
  const [cfg, setCfg] = useState<AiConfig | null>(null);
  const [keys, setKeys] = useState<Record<string, string>>({});
  const [busy, setBusy] = useState<string | null>(null);
  const [result, setResult] = useState<Record<string, { ok: boolean; msg: string }>>({});

  const refresh = useCallback(async () => {
    const [l, c] = await Promise.all([aiProviderStatus(), aiGetConfig()]);
    setList(l);
    setCfg(c);
  }, []);

  useEffect(() => {
    refresh().catch((e) => console.error("ai providers", e));
  }, [refresh]);

  if (!list || !cfg) return null;

  const saveConfig = async (next: AiConfig) => {
    setCfg(next);
    try {
      await aiSetConfig(next);
    } catch (e) {
      console.error("ai config", e);
    }
  };

  const saveKey = async (id: AiProviderId, value: string) => {
    setBusy(id);
    try {
      await aiSetKey(id, value);
      setKeys((k) => ({ ...k, [id]: "" }));
      setResult((r) => ({ ...r, [id]: { ok: true, msg: value.trim() ? "Schlüssel gespeichert." : "Schlüssel entfernt." } }));
      await refresh();
    } catch (e) {
      setResult((r) => ({ ...r, [id]: { ok: false, msg: String(e) } }));
    } finally {
      setBusy(null);
    }
  };

  const test = async (id: AiProviderId) => {
    setBusy(id);
    setResult((r) => {
      const n = { ...r };
      delete n[id];
      return n;
    });
    try {
      const msg = await aiTestProvider(id);
      setResult((r) => ({ ...r, [id]: { ok: true, msg } }));
    } catch (e) {
      setResult((r) => ({ ...r, [id]: { ok: false, msg: String(e) } }));
    } finally {
      setBusy(null);
    }
  };

  return (
    <div id="settings-ai" className="scroll-mt-3 rounded-lg">
      <div className="mb-1 flex items-center gap-2">
        <Bot size={16} className="text-[var(--color-accent)]" />
        <h2 className="text-[14px] font-semibold">KI-Anbieter</h2>
      </div>
      <p className="mb-4 text-[12px] text-[var(--color-muted)]">
        Für KI-Tasks (Befehl <code>task</code> bzw. <code>ki</code>). Schlüssel liegen im Schlüsselbund und werden nur an
        den jeweiligen Anbieter geschickt. Claude geht entweder per API-Schlüssel oder über das lokale Programm{" "}
        <code>claude</code> mit seinem eigenen Login.
      </p>
      <div className="rounded-lg border border-[var(--color-border)] p-4">
        <label className="mb-4 flex items-center gap-2 text-[12px]">
          <span className="font-medium">Standard für neue Tasks</span>
          <select
            value={cfg.provider}
            onChange={(e) => void saveConfig({ ...cfg, provider: e.target.value as AiProviderId })}
            className="rounded border border-[var(--color-border)] bg-[var(--color-bg)] px-2 py-1 text-[12px]"
          >
            {list.map((p) => (
              <option key={p.id} value={p.id}>
                {p.label}
                {p.configured ? "" : " (nicht eingerichtet)"}
              </option>
            ))}
          </select>
        </label>

        <div className="flex flex-col gap-3">
          {list.map((p) => {
            const isCli = p.id === "claude_cli";
            const r = result[p.id];
            return (
              <div key={p.id} className="rounded-md border border-[var(--color-border)] p-3">
                <div className="mb-2 flex items-center gap-2 text-[12px]">
                  {isCli ? <Terminal size={13} /> : <KeyRound size={13} />}
                  <span className="font-medium">{p.label}</span>
                  <span
                    className={`ml-auto rounded px-1.5 py-0.5 text-[10px] ${
                      p.configured ? "bg-emerald-500/15 text-emerald-500" : "bg-[var(--color-surface)] text-[var(--color-muted)]"
                    }`}
                  >
                    {p.configured ? (isCli ? "gefunden" : "Schlüssel hinterlegt") : isCli ? "nicht installiert" : "kein Schlüssel"}
                  </span>
                </div>
                {isCli ? (
                  <p className="mb-2 break-all text-[11px] text-[var(--color-muted)]">
                    {p.hint ?? "Programm `claude` nicht gefunden — Claude Code installieren und einmal anmelden."}
                  </p>
                ) : (
                  <div className="mb-2 flex items-center gap-2">
                    <input
                      type="password"
                      autoComplete="off"
                      spellCheck={false}
                      value={keys[p.id] ?? ""}
                      placeholder={p.hint ? `${p.hint} (gespeichert — zum Ersetzen tippen)` : "API-Schlüssel einfügen"}
                      onChange={(e) => setKeys((k) => ({ ...k, [p.id]: e.target.value }))}
                      onKeyDown={(e) => {
                        if (e.key === "Enter" && (keys[p.id] ?? "").trim()) void saveKey(p.id, keys[p.id]);
                      }}
                      className="min-w-0 flex-1 rounded border border-[var(--color-border)] bg-[var(--color-bg)] px-2 py-1 font-mono text-[12px]"
                    />
                    <button
                      disabled={busy === p.id || !(keys[p.id] ?? "").trim()}
                      onClick={() => void saveKey(p.id, keys[p.id] ?? "")}
                      className="rounded-md bg-[var(--color-accent)] px-2.5 py-1 text-[11px] font-medium text-[var(--color-accent-fg)] disabled:opacity-40"
                    >
                      Speichern
                    </button>
                    {p.configured && (
                      <button
                        disabled={busy === p.id}
                        onClick={() => void saveKey(p.id, "")}
                        title="Schlüssel entfernen"
                        className="rounded-md border border-[var(--color-border)] px-2 py-1 text-[11px] disabled:opacity-40"
                      >
                        Entfernen
                      </button>
                    )}
                  </div>
                )}
                <div className="flex items-center gap-2">
                  <span className="text-[11px] text-[var(--color-muted)]">Modell</span>
                  <input
                    value={cfg.models[p.id] ?? p.default_model}
                    spellCheck={false}
                    placeholder={p.default_model || "Standard von Claude Code"}
                    onChange={(e) => setCfg({ ...cfg, models: { ...cfg.models, [p.id]: e.target.value } })}
                    onBlur={() => void saveConfig(cfg)}
                    className="min-w-0 flex-1 rounded border border-[var(--color-border)] bg-[var(--color-bg)] px-2 py-1 font-mono text-[11px]"
                  />
                  <button
                    disabled={busy === p.id || !p.configured}
                    onClick={() => void test(p.id)}
                    className="flex items-center gap-1 rounded-md border border-[var(--color-border)] px-2 py-1 text-[11px] disabled:opacity-40"
                  >
                    {busy === p.id ? <Loader2 size={11} className="animate-spin" /> : null}
                    Testen
                  </button>
                </div>
                {r && (
                  <p className={`confirm-enter mt-2 flex items-start gap-1 text-[11px] ${r.ok ? "text-emerald-500" : "text-red-500"}`}>
                    {r.ok ? <Check size={11} className="mt-0.5 shrink-0" /> : <X size={11} className="mt-0.5 shrink-0" />}
                    <span className="break-words">{r.msg}</span>
                  </p>
                )}
              </div>
            );
          })}
        </div>
      </div>
    </div>
  );
}
