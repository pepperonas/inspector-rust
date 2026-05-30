import { useEffect, useMemo, useState } from "react";
import {
  Clipboard,
  Download,
  KeyRound,
  Plus,
  Trash2,
  Upload,
  X,
} from "lucide-react";
import {
  totpAdd,
  totpCurrentCodesAll,
  totpDelete,
  totpExport,
  totpImport,
  totpList,
} from "../lib/ipc";
import type { TotpCode, TotpEntry } from "../lib/totp";

/**
 * Full-overlay TOTP / 2FA management. Replaces the popup body, owns
 * keyboard input (Esc to exit). Three tabs:
 *
 *   - **List**: every entry with live code + countdown ring + copy
 *     + delete buttons.
 *   - **Add**: manual form (Issuer / Account / Secret, plus the
 *     advanced trio digits/period/algorithm hidden behind a toggle).
 *   - **Import / Export**: paste-and-go for any supported format,
 *     plus a copy-all-as-otpauth-URIs button.
 *
 * Live codes are fetched every 1 s via `totpCurrentCodesAll`. The
 * countdown ring on each row interpolates locally between fetches
 * so it looks smooth instead of ticking once a second.
 */

interface Props {
  onExit: () => void;
}

type Tab = "list" | "add" | "import";

export function TotpOverlay({ onExit }: Props) {
  const [tab, setTab] = useState<Tab>("list");
  const [entries, setEntries] = useState<TotpEntry[]>([]);
  const [codes, setCodes] = useState<Map<number, TotpCode>>(new Map());
  // Bumped on every 1s tick; used to drive the smooth countdown ring.
  const [tickNow, setTickNow] = useState(() => Date.now());
  const [busy, setBusy] = useState(false);
  const [toast, setToast] = useState<{ kind: "ok" | "err"; message: string } | null>(null);

  // Add-form state
  const [addIssuer, setAddIssuer] = useState("");
  const [addAccount, setAddAccount] = useState("");
  const [addSecret, setAddSecret] = useState("");
  const [addAdvanced, setAddAdvanced] = useState(false);
  const [addDigits, setAddDigits] = useState(6);
  const [addPeriod, setAddPeriod] = useState(30);
  const [addAlgorithm, setAddAlgorithm] = useState("SHA1");

  // Import-tab state
  const [importText, setImportText] = useState("");

  // Esc to exit — owned by this overlay.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.preventDefault();
        e.stopPropagation();
        onExit();
      }
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [onExit]);

  // Initial fetch + 1 s polling of live codes.
  useEffect(() => {
    let cancelled = false;
    const refresh = async () => {
      try {
        const [list, currentCodes] = await Promise.all([
          totpList(),
          totpCurrentCodesAll(),
        ]);
        if (cancelled) return;
        setEntries(list);
        setCodes(new Map(currentCodes.map((c) => [c.id, c])));
      } catch (e) {
        if (!cancelled) {
          console.error("totp refresh failed", e);
        }
      }
    };
    void refresh();
    const interval = setInterval(refresh, 1000);
    const tick = setInterval(() => setTickNow(Date.now()), 100);
    return () => {
      cancelled = true;
      clearInterval(interval);
      clearInterval(tick);
    };
  }, []);

  const doAdd = async () => {
    if (!addIssuer.trim() || !addSecret.trim()) {
      setToast({ kind: "err", message: "Issuer und Secret sind Pflicht." });
      return;
    }
    setBusy(true);
    setToast(null);
    try {
      await totpAdd({
        issuer: addIssuer.trim(),
        account: addAccount.trim(),
        secret: addSecret.trim(),
        digits: addDigits,
        period: addPeriod,
        algorithm: addAlgorithm,
      });
      setAddIssuer("");
      setAddAccount("");
      setAddSecret("");
      setAddAdvanced(false);
      setAddDigits(6);
      setAddPeriod(30);
      setAddAlgorithm("SHA1");
      setToast({ kind: "ok", message: "Eintrag hinzugefügt." });
      // Refresh visible list immediately.
      const [list, currentCodes] = await Promise.all([totpList(), totpCurrentCodesAll()]);
      setEntries(list);
      setCodes(new Map(currentCodes.map((c) => [c.id, c])));
      setTab("list");
    } catch (e) {
      setToast({ kind: "err", message: String(e) });
    } finally {
      setBusy(false);
    }
  };

  const doDelete = async (id: number, label: string) => {
    if (!window.confirm(`Eintrag "${label}" wirklich löschen?`)) return;
    setBusy(true);
    try {
      await totpDelete(id);
      setEntries((es) => es.filter((e) => e.id !== id));
      setCodes((c) => {
        const next = new Map(c);
        next.delete(id);
        return next;
      });
      setToast({ kind: "ok", message: `"${label}" gelöscht.` });
    } catch (e) {
      setToast({ kind: "err", message: String(e) });
    } finally {
      setBusy(false);
    }
  };

  const doCopy = async (id: number, label: string) => {
    const code = codes.get(id)?.code;
    if (!code) return;
    try {
      const { writeText } = await import("@tauri-apps/plugin-clipboard-manager");
      await writeText(code);
      setToast({ kind: "ok", message: `${label} → Zwischenablage` });
    } catch (e) {
      setToast({ kind: "err", message: String(e) });
    }
  };

  const doImport = async () => {
    if (!importText.trim()) {
      setToast({ kind: "err", message: "Nichts zum Importieren." });
      return;
    }
    setBusy(true);
    try {
      const result = await totpImport(importText);
      if (result.error) {
        setToast({ kind: "err", message: result.error });
      } else {
        setToast({ kind: "ok", message: `${result.added} Eintrag/Einträge importiert.` });
        setImportText("");
        // Refresh.
        const [list, currentCodes] = await Promise.all([totpList(), totpCurrentCodesAll()]);
        setEntries(list);
        setCodes(new Map(currentCodes.map((c) => [c.id, c])));
        setTab("list");
      }
    } catch (e) {
      setToast({ kind: "err", message: String(e) });
    } finally {
      setBusy(false);
    }
  };

  const doExport = async () => {
    try {
      const uris = await totpExport();
      if (!uris.trim()) {
        setToast({ kind: "err", message: "Keine Einträge zum Exportieren." });
        return;
      }
      const { writeText } = await import("@tauri-apps/plugin-clipboard-manager");
      await writeText(uris);
      setToast({
        kind: "ok",
        message: `${entries.length} Einträge als otpauth-URIs in der Zwischenablage. Achtung: Plaintext!`,
      });
    } catch (e) {
      setToast({ kind: "err", message: String(e) });
    }
  };

  return (
    <div className="flex h-full w-full flex-col bg-[var(--color-bg)] text-[var(--color-fg)]">
      {/* Top bar */}
      <div className="flex items-center justify-between border-b border-[var(--color-border)] px-4 py-2 text-[12px]">
        <div className="flex items-center gap-2">
          <KeyRound size={14} className="text-[var(--color-accent)]" />
          <span className="font-semibold">2FA · TOTP</span>
          <span className="text-[var(--color-muted)]">·</span>
          <TabButton active={tab === "list"} onClick={() => setTab("list")}>
            Liste {entries.length > 0 ? `(${entries.length})` : ""}
          </TabButton>
          <TabButton active={tab === "add"} onClick={() => setTab("add")}>
            Hinzufügen
          </TabButton>
          <TabButton active={tab === "import"} onClick={() => setTab("import")}>
            Import / Export
          </TabButton>
        </div>
        <button
          onClick={onExit}
          className="flex items-center gap-1 rounded border border-[var(--color-border)] px-2 py-0.5 text-[10px] hover:border-[var(--color-accent)] hover:text-[var(--color-accent)]"
        >
          <X size={11} />
          Esc
        </button>
      </div>

      {/* Body */}
      <div className="flex-1 overflow-auto p-4">
        {tab === "list" && (
          <ListTab
            entries={entries}
            codes={codes}
            tickNow={tickNow}
            onCopy={doCopy}
            onDelete={doDelete}
            busy={busy}
          />
        )}
        {tab === "add" && (
          <AddTab
            issuer={addIssuer}
            setIssuer={setAddIssuer}
            account={addAccount}
            setAccount={setAddAccount}
            secret={addSecret}
            setSecret={setAddSecret}
            advanced={addAdvanced}
            setAdvanced={setAddAdvanced}
            digits={addDigits}
            setDigits={setAddDigits}
            period={addPeriod}
            setPeriod={setAddPeriod}
            algorithm={addAlgorithm}
            setAlgorithm={setAddAlgorithm}
            onSubmit={doAdd}
            busy={busy}
          />
        )}
        {tab === "import" && (
          <ImportExportTab
            importText={importText}
            setImportText={setImportText}
            onImport={doImport}
            onExport={doExport}
            entryCount={entries.length}
            busy={busy}
          />
        )}
      </div>

      {/* Toast */}
      {toast && (
        <div
          className={
            "mx-4 mb-3 rounded border px-3 py-2 text-[12px] " +
            (toast.kind === "ok"
              ? "border-emerald-500/40 bg-emerald-500/10"
              : "border-rose-500/40 bg-rose-500/10")
          }
        >
          {toast.message}
        </div>
      )}
    </div>
  );
}

function TabButton({
  active,
  onClick,
  children,
}: {
  active: boolean;
  onClick: () => void;
  children: React.ReactNode;
}) {
  return (
    <button
      onClick={onClick}
      className={
        "rounded px-2 py-0.5 text-[12px] " +
        (active
          ? "bg-[var(--color-accent)] text-[var(--color-accent-fg)]"
          : "text-[var(--color-muted)] hover:bg-[var(--color-surface)]")
      }
    >
      {children}
    </button>
  );
}

// ── List tab ─────────────────────────────────────────────────────────

function ListTab({
  entries,
  codes,
  tickNow,
  onCopy,
  onDelete,
  busy,
}: {
  entries: TotpEntry[];
  codes: Map<number, TotpCode>;
  tickNow: number;
  onCopy: (id: number, label: string) => void;
  onDelete: (id: number, label: string) => void;
  busy: boolean;
}) {
  if (entries.length === 0) {
    return (
      <div className="flex h-full flex-col items-center justify-center gap-3 text-center text-[var(--color-muted)]">
        <KeyRound size={32} className="opacity-50" />
        <div className="text-[14px]">Noch keine 2FA-Einträge.</div>
        <div className="text-[12px]">
          Wechsle zu <b>Hinzufügen</b> oder <b>Import</b>, um deine
          Authenticator-Konten anzulegen.
        </div>
      </div>
    );
  }

  return (
    <div className="flex flex-col gap-2">
      {entries.map((e) => {
        const c = codes.get(e.id);
        const label = `${e.issuer || "?"}${e.account ? " · " + e.account : ""}`;
        return (
          <div
            key={e.id}
            className="flex items-center gap-3 rounded-lg border border-[var(--color-border)] bg-[var(--color-surface)] p-3"
          >
            {/* Countdown ring */}
            <CountdownRing
              secondsRemaining={c?.seconds_remaining ?? 0}
              period={e.period}
              tickNow={tickNow}
            />
            {/* Label + code */}
            <div className="flex flex-1 flex-col">
              <div className="text-[13px] font-semibold">{e.issuer || "(no issuer)"}</div>
              {e.account && (
                <div className="text-[11px] text-[var(--color-muted)]">{e.account}</div>
              )}
            </div>
            <button
              onClick={() => onCopy(e.id, label)}
              disabled={busy || !c}
              className="rounded border border-[var(--color-border)] px-3 py-1.5 font-[var(--font-mono)] text-[20px] font-semibold tracking-[0.15em] tabular-nums hover:border-[var(--color-accent)] hover:text-[var(--color-accent)] disabled:opacity-40"
              title="Code in die Zwischenablage kopieren"
            >
              {c?.code ?? "…"}
            </button>
            <button
              onClick={() => onCopy(e.id, label)}
              disabled={busy || !c}
              title="Kopieren"
              className="rounded border border-[var(--color-border)] p-1.5 text-[var(--color-muted)] hover:border-[var(--color-accent)] hover:text-[var(--color-accent)] disabled:opacity-40"
            >
              <Clipboard size={14} />
            </button>
            <button
              onClick={() => onDelete(e.id, label)}
              disabled={busy}
              title="Löschen"
              className="rounded border border-[var(--color-border)] p-1.5 text-[var(--color-muted)] hover:border-rose-500/60 hover:text-rose-500 disabled:opacity-40"
            >
              <Trash2 size={14} />
            </button>
          </div>
        );
      })}
    </div>
  );
}

/** SVG-based circular countdown indicator. Smoothly interpolates
 *  between server-side `seconds_remaining` values (which only update
 *  once per 1 s IPC poll) using the local `tickNow` clock. */
function CountdownRing({
  secondsRemaining,
  period,
  tickNow,
}: {
  secondsRemaining: number;
  period: number;
  tickNow: number;
}) {
  // We track a "reference" wall-clock of when we last got a fresh
  // server value, plus the value at that moment. Local interpolation
  // = server_value - elapsed_since_reference.
  // (useState would be overkill — we just compute from props each render.)
  const drift = (tickNow % 1000) / 1000;
  const localRemaining = Math.max(0, secondsRemaining - drift);
  const fraction = localRemaining / period;
  const r = 14;
  const c = 2 * Math.PI * r;
  const offset = c * (1 - fraction);
  // Color: green when fresh, amber when ≤5s, red when ≤2s.
  const color =
    localRemaining <= 2
      ? "stroke-rose-500"
      : localRemaining <= 5
        ? "stroke-amber-500"
        : "stroke-emerald-500";
  return (
    <div className="relative h-10 w-10 shrink-0">
      <svg viewBox="0 0 32 32" className="absolute inset-0 -rotate-90">
        <circle
          cx="16"
          cy="16"
          r={r}
          fill="none"
          className="stroke-[var(--color-border)]"
          strokeWidth="2.5"
        />
        <circle
          cx="16"
          cy="16"
          r={r}
          fill="none"
          className={color + " transition-[stroke-dashoffset] duration-100"}
          strokeWidth="2.5"
          strokeDasharray={c}
          strokeDashoffset={offset}
          strokeLinecap="round"
        />
      </svg>
      <div className="absolute inset-0 flex items-center justify-center font-[var(--font-mono)] text-[10px] font-semibold tabular-nums text-[var(--color-muted)]">
        {Math.ceil(localRemaining)}
      </div>
    </div>
  );
}

// ── Add tab ──────────────────────────────────────────────────────────

function AddTab(props: {
  issuer: string;
  setIssuer: (s: string) => void;
  account: string;
  setAccount: (s: string) => void;
  secret: string;
  setSecret: (s: string) => void;
  advanced: boolean;
  setAdvanced: (b: boolean) => void;
  digits: number;
  setDigits: (n: number) => void;
  period: number;
  setPeriod: (n: number) => void;
  algorithm: string;
  setAlgorithm: (s: string) => void;
  onSubmit: () => void;
  busy: boolean;
}) {
  return (
    <div className="mx-auto flex max-w-md flex-col gap-3">
      <label className="flex flex-col gap-1 text-[12px]">
        <span className="font-semibold">Issuer / Service</span>
        <input
          type="text"
          value={props.issuer}
          onChange={(e) => props.setIssuer(e.target.value)}
          placeholder="Amazon, GitHub, …"
          className="rounded border border-[var(--color-border)] bg-[var(--color-surface)] px-2 py-1.5 text-[13px] focus:border-[var(--color-accent)] focus:outline-none"
          autoFocus
        />
      </label>
      <label className="flex flex-col gap-1 text-[12px]">
        <span className="font-semibold">Konto (optional)</span>
        <input
          type="text"
          value={props.account}
          onChange={(e) => props.setAccount(e.target.value)}
          placeholder="user@example.com"
          className="rounded border border-[var(--color-border)] bg-[var(--color-surface)] px-2 py-1.5 text-[13px] focus:border-[var(--color-accent)] focus:outline-none"
        />
      </label>
      <label className="flex flex-col gap-1 text-[12px]">
        <span className="font-semibold">Secret (Base32)</span>
        <textarea
          value={props.secret}
          onChange={(e) => props.setSecret(e.target.value)}
          placeholder="JBSW Y3DP EHPK 3PXP"
          rows={2}
          className="rounded border border-[var(--color-border)] bg-[var(--color-surface)] px-2 py-1.5 font-[var(--font-mono)] text-[13px] focus:border-[var(--color-accent)] focus:outline-none"
        />
        <span className="text-[10px] text-[var(--color-muted)]">
          Leerzeichen + Bindestriche werden automatisch entfernt; Padding (=) optional.
        </span>
      </label>

      <button
        type="button"
        onClick={() => props.setAdvanced(!props.advanced)}
        className="self-start text-[11px] text-[var(--color-muted)] hover:text-[var(--color-accent)]"
      >
        {props.advanced ? "▾ Erweitert ausblenden" : "▸ Erweitert (Digits / Period / Algorithm)"}
      </button>
      {props.advanced && (
        <div className="grid grid-cols-3 gap-3">
          <label className="flex flex-col gap-1 text-[12px]">
            <span className="font-semibold">Digits</span>
            <select
              value={props.digits}
              onChange={(e) => props.setDigits(Number(e.target.value))}
              className="rounded border border-[var(--color-border)] bg-[var(--color-surface)] px-2 py-1.5 text-[13px]"
            >
              <option value={6}>6</option>
              <option value={7}>7</option>
              <option value={8}>8</option>
            </select>
          </label>
          <label className="flex flex-col gap-1 text-[12px]">
            <span className="font-semibold">Period (s)</span>
            <select
              value={props.period}
              onChange={(e) => props.setPeriod(Number(e.target.value))}
              className="rounded border border-[var(--color-border)] bg-[var(--color-surface)] px-2 py-1.5 text-[13px]"
            >
              <option value={15}>15</option>
              <option value={30}>30</option>
              <option value={60}>60</option>
            </select>
          </label>
          <label className="flex flex-col gap-1 text-[12px]">
            <span className="font-semibold">Algorithm</span>
            <select
              value={props.algorithm}
              onChange={(e) => props.setAlgorithm(e.target.value)}
              className="rounded border border-[var(--color-border)] bg-[var(--color-surface)] px-2 py-1.5 text-[13px]"
            >
              <option value="SHA1">SHA1</option>
              <option value="SHA256">SHA256</option>
              <option value="SHA512">SHA512</option>
            </select>
          </label>
        </div>
      )}

      <button
        onClick={props.onSubmit}
        disabled={props.busy || !props.issuer.trim() || !props.secret.trim()}
        className="mt-2 flex items-center justify-center gap-2 rounded bg-[var(--color-accent)] px-4 py-2 text-[13px] font-semibold text-[var(--color-accent-fg)] hover:opacity-90 disabled:opacity-40"
      >
        <Plus size={14} />
        Hinzufügen
      </button>
    </div>
  );
}

// ── Import / Export tab ─────────────────────────────────────────────

function ImportExportTab({
  importText,
  setImportText,
  onImport,
  onExport,
  entryCount,
  busy,
}: {
  importText: string;
  setImportText: (s: string) => void;
  onImport: () => void;
  onExport: () => void;
  entryCount: number;
  busy: boolean;
}) {
  const supportedFormats = useMemo(
    () => [
      "otpauth://totp/… (einzelnes QR-Code-URI)",
      "otpauth-migration://offline?data=… (Google Authenticator Bulk-Export)",
      "Aegis JSON (Android Aegis Authenticator, unverschlüsselt)",
      "2FAS JSON (2FAS Auth)",
      "Plain-Text: ein otpauth://-URI pro Zeile",
    ],
    [],
  );

  return (
    <div className="mx-auto flex max-w-2xl flex-col gap-5">
      <section>
        <h3 className="mb-2 flex items-center gap-2 text-[13px] font-semibold">
          <Upload size={14} className="text-[var(--color-accent)]" />
          Import
        </h3>
        <textarea
          value={importText}
          onChange={(e) => setImportText(e.target.value)}
          placeholder="otpauth://totp/...  oder JSON-Export hier einfügen"
          rows={8}
          className="w-full rounded border border-[var(--color-border)] bg-[var(--color-surface)] px-3 py-2 font-[var(--font-mono)] text-[12px] focus:border-[var(--color-accent)] focus:outline-none"
        />
        <div className="mt-2 flex items-center gap-3">
          <button
            onClick={onImport}
            disabled={busy || !importText.trim()}
            className="flex items-center gap-2 rounded bg-[var(--color-accent)] px-3 py-1.5 text-[12px] font-semibold text-[var(--color-accent-fg)] hover:opacity-90 disabled:opacity-40"
          >
            <Upload size={12} />
            Import starten
          </button>
          <span className="text-[11px] text-[var(--color-muted)]">
            Format wird automatisch erkannt.
          </span>
        </div>
        <details className="mt-3 text-[11px] text-[var(--color-muted)]">
          <summary className="cursor-pointer hover:text-[var(--color-fg)]">
            Unterstützte Formate
          </summary>
          <ul className="mt-2 list-disc space-y-1 pl-5">
            {supportedFormats.map((f, i) => (
              <li key={i}>{f}</li>
            ))}
          </ul>
        </details>
      </section>

      <section>
        <h3 className="mb-2 flex items-center gap-2 text-[13px] font-semibold">
          <Download size={14} className="text-[var(--color-accent)]" />
          Export
        </h3>
        <p className="mb-2 text-[12px] text-[var(--color-muted)]">
          Exportiert alle {entryCount} Einträge als Liste von{" "}
          <code className="rounded bg-[var(--color-surface)] px-1 font-[var(--font-mono)] text-[11px]">
            otpauth://
          </code>
          -URIs in der Zwischenablage.
        </p>
        <div className="mb-3 rounded border border-amber-500/40 bg-amber-500/10 px-3 py-2 text-[11px]">
          <b>Achtung:</b> Plaintext. Die Secrets sind die langfristige
          Wurzel deiner 2FA — speichere die Datei verschlüsselt
          (z.B. macOS Keychain, 1Password) und lösche danach die
          Zwischenablage.
        </div>
        <button
          onClick={onExport}
          disabled={busy || entryCount === 0}
          className="flex items-center gap-2 rounded border border-[var(--color-border)] px-3 py-1.5 text-[12px] hover:border-[var(--color-accent)] hover:text-[var(--color-accent)] disabled:opacity-40"
        >
          <Download size={12} />
          In Zwischenablage exportieren
        </button>
      </section>
    </div>
  );
}
