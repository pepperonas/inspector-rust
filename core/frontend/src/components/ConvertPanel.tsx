import { useEffect, useMemo, useRef, useState } from "react";
import { writeText } from "@tauri-apps/plugin-clipboard-manager";
import {
  ArrowRightLeft,
  Check,
  Clock,
  Coins,
  FlaskConical,
  Fuel,
  Gauge,
  HardDrive,
  Plug,
  RefreshCw,
  Ruler,
  Square,
  Thermometer,
  TriangleRight,
  Waves,
  Weight,
  Zap,
  type LucideIcon,
} from "lucide-react";
import {
  CATEGORIES,
  categoryById,
  convertAll,
  suggestUnits,
  unitById,
  unitsOf,
  type CategoryId,
  type Rates,
  type Unit,
} from "../lib/units";
import { buildArg, pasteNumber, type ConvertParse } from "../lib/convert-cmd";
import type { FxRates } from "../lib/ipc";

/**
 * `convert` / `cv` preview (v0.186.0). Everything the panel shows is derived
 * from the parsed search-bar argument — its own controls (category chips,
 * value field, unit + target selects) WRITE BACK into the search bar via
 * `onArgChange`, so the query stays the single source of truth and the
 * command row, the autocomplete and this panel can never disagree.
 */

const ICONS: Record<CategoryId, LucideIcon> = {
  length: Ruler,
  area: Square,
  volume: FlaskConical,
  mass: Weight,
  temperature: Thermometer,
  speed: Gauge,
  time: Clock,
  data: HardDrive,
  energy: Zap,
  power: Plug,
  pressure: Waves,
  fuel: Fuel,
  angle: TriangleRight,
  currency: Coins,
};

const DATE_FMT = new Intl.DateTimeFormat("de-DE", { day: "2-digit", month: "2-digit", year: "numeric" });

function ago(ms: number | null, now: number): string {
  if (!ms) return "—";
  const min = Math.max(0, Math.round((now - ms) / 60000));
  if (min < 1) return "gerade eben";
  if (min < 60) return `vor ${min} min`;
  const h = Math.round(min / 60);
  return h < 48 ? `vor ${h} h` : `vor ${Math.round(h / 24)} Tagen`;
}

function ecbDate(iso: string | null): string {
  if (!iso) return "—";
  const d = new Date(`${iso}T12:00:00`);
  return Number.isNaN(d.getTime()) ? iso : DATE_FMT.format(d);
}

export function ConvertPanel({
  parse,
  rates,
  fx,
  fxLoading,
  focused,
  onArgChange,
  onPick,
  onRefreshRates,
  onInteract,
  onEngage,
  onExit,
}: {
  parse: ConvertParse;
  /** EUR value per currency unit (from `fx`), or undefined while loading. */
  rates?: Rates;
  fx: FxRates | null;
  fxLoading: boolean;
  focused: boolean;
  onArgChange: (arg: string) => void;
  /** Enter on a row: paste this plain number (the parent hides the popup). */
  onPick: (text: string) => void;
  onRefreshRates: () => void;
  onInteract?: () => void;
  /** A control inside the panel took focus — the list keys must stand down. */
  onEngage?: () => void;
  onExit: () => void;
}) {
  const ready = parse.kind === "ready" ? parse : null;
  // An unfinished unit (`cv 165 mp`) previews the BEST completion — the same
  // one the list offers first — instead of an unrelated default.
  const guess = useMemo(
    () => (parse.kind === "partial" ? (suggestUnits(parse.unitText, { rates, limit: 1 })[0] ?? null) : null),
    [parse, rates],
  );
  // Browse mode keeps its own category until the user types a unit.
  const [browseCat, setBrowseCat] = useState<CategoryId>("length");
  const lead = ready?.unit ?? guess;
  const cat: CategoryId = lead ? lead.cat : browseCat;
  const category = categoryById(cat);

  const source: Unit = lead ?? unitById(category.defaultUnit)!;
  const withValue =
    parse.kind === "ready" || parse.kind === "value" || parse.kind === "partial" ? parse : null;
  const value = withValue ? withValue.value : 1;
  const valueText = withValue ? withValue.valueText : "";
  const target = ready?.target ?? null;

  const rows = useMemo(() => convertAll(value, source, rates), [value, source, rates]);
  const units = useMemo(() => unitsOf(cat, cat === "currency" ? rates : undefined), [cat, rates]);

  // "vor 3 min" needs a clock — read once, refreshed every 30 s (never
  // Date.now() during render: react-hooks/purity).
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    const t = window.setInterval(() => setNow(Date.now()), 30_000);
    return () => window.clearInterval(t);
  }, []);

  const [sel, setSel] = useState(0);
  const [copied, setCopied] = useState<string | null>(null);
  const listRef = useRef<HTMLDivElement>(null);

  // Keyboard selection starts on the target (or the source) whenever the
  // conversion itself changes.
  const anchorId = target?.id ?? source.id;
  const [anchorFor, setAnchorFor] = useState<string | null>(null);
  if (anchorFor !== `${cat}:${anchorId}`) {
    setAnchorFor(`${cat}:${anchorId}`);
    setSel(Math.max(0, rows.findIndex((r) => r.unit.id === anchorId)));
  }

  const write = (nextCat: CategoryId, unitId: string, targetId: string | null, vText = valueText) => {
    if (!ready && parse.kind !== "value" && parse.kind !== "partial") setBrowseCat(nextCat);
    onArgChange(buildArg(vText, unitId, targetId));
  };

  const pickCategory = (id: CategoryId) => {
    setBrowseCat(id);
    write(id, categoryById(id).defaultUnit, null);
  };

  const copyRow = (unit: Unit, v: number) => {
    const text = pasteNumber(v, unit);
    void writeText(text)
      .then(() => {
        setCopied(unit.id);
        window.setTimeout(() => setCopied((c) => (c === unit.id ? null : c)), 1200);
      })
      .catch(() => undefined);
  };

  // Keep the keyboard selection in view.
  useEffect(() => {
    const el = listRef.current?.querySelector<HTMLElement>(`[data-row="${sel}"]`);
    el?.scrollIntoView?.({ block: "nearest" });
  }, [sel]);

  useEffect(() => {
    if (!focused) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.preventDefault();
        e.stopPropagation();
        onExit();
        return;
      }
      // Never swallow typing: the search bar and the value field keep their keys.
      const t = e.target as HTMLElement | null;
      if (t && (t.tagName === "INPUT" || t.tagName === "SELECT" || t.isContentEditable)) return;
      const idx = CATEGORIES.findIndex((c) => c.id === cat);
      if (e.key === "ArrowRight" || e.key === "ArrowLeft") {
        e.preventDefault();
        e.stopPropagation();
        const step = e.key === "ArrowRight" ? 1 : -1;
        const next = CATEGORIES[(idx + step + CATEGORIES.length) % CATEGORIES.length];
        pickCategory(next.id);
      } else if (e.key === "ArrowDown" || e.key === "ArrowUp") {
        e.preventDefault();
        e.stopPropagation();
        setSel((s) => {
          const n = s + (e.key === "ArrowDown" ? 1 : -1);
          return Math.max(0, Math.min(rows.length - 1, n));
        });
      } else if (e.key === "Enter") {
        const row = rows[sel];
        if (!row) return;
        e.preventDefault();
        e.stopPropagation();
        onPick(pasteNumber(row.value, row.unit));
      }
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [focused, cat, rows, sel, onExit, onPick]);

  const Icon = ICONS[cat];
  const needsRates = cat === "currency";

  return (
    <div
      className="flex h-full flex-col gap-3 overflow-y-auto p-4 text-[var(--color-fg)] [contain:paint]"
      onPointerUp={() => onInteract?.()}
      onFocus={() => onEngage?.()}
    >
      <div className="flex items-center gap-2 text-[13px] font-medium">
        <ArrowRightLeft size={15} className="text-[var(--color-accent)]" />
        Umrechnen
        {focused && (
          <span className="ml-auto text-[10.5px] font-normal text-[var(--color-muted)]">
            ←→ Kategorie · ↑↓ Zeile · Enter einfügen · Esc
          </span>
        )}
      </div>

      {/* Category chips */}
      <div className="flex flex-wrap gap-1.5" role="tablist" aria-label="Kategorie">
        {CATEGORIES.map((c) => {
          const CIcon = ICONS[c.id];
          const active = c.id === cat;
          return (
            <button
              key={c.id}
              type="button"
              role="tab"
              aria-selected={active}
              data-cat={c.id}
              onClick={() => pickCategory(c.id)}
              className={
                "md3-press flex items-center gap-1 rounded-full border px-2 py-0.5 text-[11px] transition-colors " +
                (active
                  ? "border-[var(--color-accent)] bg-[var(--color-accent)] text-[var(--color-accent-fg)]"
                  : "border-[var(--color-border)] text-[var(--color-muted)] hover:text-[var(--color-fg)]")
              }
            >
              <CIcon size={11} />
              {c.label}
            </button>
          );
        })}
      </div>

      {/* Own input: value · unit → target */}
      <div className="flex items-center gap-2 rounded-lg border border-[var(--color-border)] px-2 py-1.5">
        <Icon size={14} className="shrink-0 text-[var(--color-accent)]" />
        <input
          aria-label="Wert"
          inputMode="decimal"
          value={valueText}
          placeholder="1"
          onChange={(e) => write(cat, source.id, target?.id ?? null, e.target.value)}
          className="w-24 min-w-0 flex-1 bg-transparent text-[13px] tabular-nums outline-none placeholder:text-[var(--color-muted)]"
        />
        <select
          aria-label="Einheit"
          value={source.id}
          onChange={(e) => write(cat, e.target.value, target && target.id !== e.target.value ? target.id : null)}
          className="rounded bg-[var(--color-surface)] px-1 py-0.5 text-[12px] outline-none"
        >
          {units.map((x) => (
            <option key={x.id} value={x.id}>
              {x.symbol} — {x.name}
            </option>
          ))}
        </select>
        <span className="text-[11px] text-[var(--color-muted)]">→</span>
        <select
          aria-label="Ziel"
          value={target?.id ?? ""}
          onChange={(e) => write(cat, source.id, e.target.value || null)}
          className="rounded bg-[var(--color-surface)] px-1 py-0.5 text-[12px] outline-none"
        >
          <option value="">alle</option>
          {units
            .filter((x) => x.id !== source.id)
            .map((x) => (
              <option key={x.id} value={x.id}>
                {x.symbol}
              </option>
            ))}
        </select>
      </div>

      {parse.kind === "partial" && (
        <p className="text-[11.5px] text-[var(--color-muted)]">
          {guess
            ? `„${parse.unitText}" ist noch keine Einheit — Vorschau für ${guess.symbol}, Tab übernimmt.`
            : `Unbekannte Einheit „${parse.unitText}".`}
        </p>
      )}
      {parse.kind === "invalid" && (
        <p className="text-[11.5px] text-[var(--color-muted)]">Das ist keine lesbare Zahl.</p>
      )}

      {needsRates && rows.length === 0 ? (
        <p className="text-[12px] text-[var(--color-muted)]">
          {fxLoading ? "Lade Kurse…" : (fx?.error ?? "Keine Kurse verfügbar.")}
        </p>
      ) : (
        <div ref={listRef} className="flex flex-col" role="listbox" aria-label={category.label}>
          {rows.map((r, i) => {
            const isSource = r.unit.id === source.id;
            const isTarget = r.unit.id === target?.id;
            const selected = focused && i === sel;
            return (
              <button
                key={r.unit.id}
                type="button"
                role="option"
                aria-selected={selected}
                data-row={i}
                data-unit={r.unit.id}
                onClick={() => {
                  setSel(i);
                  copyRow(r.unit, r.value);
                }}
                title="Klicken kopiert die Zahl"
                className={
                  "group flex items-baseline gap-2 rounded-md px-2 py-1 text-left transition-colors duration-(--duration-fast) " +
                  (selected
                    ? "bg-[var(--color-surface)] ring-1 ring-[var(--color-accent)] "
                    : "hover:bg-[var(--color-surface)] ") +
                  (isTarget ? "ring-1 ring-[var(--color-accent)] " : "")
                }
              >
                <span
                  className={
                    "min-w-0 flex-1 truncate text-right text-[14px] tabular-nums " +
                    (isSource || isTarget ? "font-semibold text-[var(--color-accent)]" : "")
                  }
                >
                  {r.text}
                </span>
                <span className="w-16 shrink-0 text-[12px] font-medium">{r.unit.symbol}</span>
                <span className="hidden w-40 shrink-0 truncate text-[11px] text-[var(--color-muted)] sm:inline">
                  {r.unit.name}
                  {isSource && " · Eingabe"}
                </span>
                <span className="w-3 shrink-0 text-[var(--color-accent)]">
                  {copied === r.unit.id && <Check size={12} className="md3-success-pop" />}
                </span>
              </button>
            );
          })}
        </div>
      )}

      {needsRates && (
        <div className="mt-auto flex items-center gap-2 text-[10.5px] text-[var(--color-muted)]">
          <span className="min-w-0 flex-1">
            EZB-Referenzkurs, Stand {ecbDate(fx?.ecb_date ?? null)} · BTC/ETH CoinGecko{" "}
            {ago(fx?.crypto_fetched_ms ?? null, now)}
            {fx?.stale && <span className="text-amber-500"> · veraltet ({fx.error})</span>}
          </span>
          <button
            type="button"
            aria-label="Kurse aktualisieren"
            onClick={onRefreshRates}
            className="md3-press rounded p-1 hover:text-[var(--color-fg)]"
          >
            <RefreshCw size={12} className={fxLoading ? "animate-spin" : ""} />
          </button>
        </div>
      )}
    </div>
  );
}
