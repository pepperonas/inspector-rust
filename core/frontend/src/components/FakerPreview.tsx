import { useEffect, useMemo, useRef, useState } from "react";
import { RefreshCw, Dices } from "lucide-react";
import {
  parseFakerCommand,
  formatValues,
  type CatalogEntry,
  type FakerDefaults,
  type FakerFormat,
  type FakerGenResult,
} from "../lib/faker";
import { fakerGenerate } from "../lib/ipc";

const FORMATS: FakerFormat[] = ["plain", "json", "csv", "sql", "ts"];
const PREVIEW_N = 25; // above this the preview generates only a sample window

/**
 * Rich preview pane for a `faker` command (rendered when the `command` row's
 * kind is "faker", like the qr preview). Parses `arg`, generates a debounced
 * sample via one IPC call, and lets the user switch format chips (no regen) or
 * reroll (⌘/Ctrl+R, or `rerollSignal`) for a fresh seed. Results are cached per
 * (spec, seed) so re-renders never reroll/flicker.
 */
export function FakerPreview({
  arg,
  catalog,
  defaults,
  rerollSignal,
}: {
  arg: string;
  catalog: CatalogEntry[];
  defaults: FakerDefaults;
  rerollSignal: number;
}) {
  const parsed = useMemo(
    () => parseFakerCommand(arg, catalog, defaults),
    [arg, catalog, defaults],
  );
  const spec = parsed.kind === "spec" ? parsed.spec : null;

  // Format is chip-controllable without re-parsing the command.
  const [fmtOverride, setFmtOverride] = useState<FakerFormat | null>(null);
  const format = fmtOverride ?? spec?.format ?? "plain";

  // Reroll seed: undefined = use the spec's seed (or a backend-random one).
  const [rerollSeed, setRerollSeed] = useState<number | undefined>(undefined);
  useEffect(() => {
    // A new command resets the format + reroll overrides.
    setFmtOverride(null);
    setRerollSeed(undefined);
  }, [arg]);
  useEffect(() => {
    if (rerollSignal > 0) setRerollSeed(Math.floor(Math.random() * 2 ** 31));
  }, [rerollSignal]);

  const [result, setResult] = useState<FakerGenResult | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);
  const cache = useRef<Map<string, FakerGenResult>>(new Map());

  // The effective spec for generation (preview caps n; seed may be a reroll).
  const genKey = spec
    ? JSON.stringify({
        g: spec.generator,
        t: spec.template,
        a: spec.args,
        l: spec.locale,
        s: rerollSeed ?? spec.seed ?? "auto",
        n: Math.min(spec.n, PREVIEW_N),
      })
    : null;

  useEffect(() => {
    if (!spec || !genKey) {
      setResult(null);
      return;
    }
    const cached = cache.current.get(genKey);
    if (cached) {
      setResult(cached);
      setError(null);
      return;
    }
    let alive = true;
    setLoading(true);
    const t = window.setTimeout(() => {
      const previewSpec = {
        ...spec,
        n: Math.min(spec.n, PREVIEW_N),
        seed: rerollSeed ?? spec.seed,
      };
      fakerGenerate(previewSpec)
        .then((r) => {
          if (!alive) return;
          cache.current.set(genKey, r);
          setResult(r);
          setError(null);
        })
        .catch((e) => {
          if (alive) setError(String(e));
        })
        .finally(() => {
          if (alive) setLoading(false);
        });
    }, 120);
    return () => {
      alive = false;
      window.clearTimeout(t);
    };
  }, [genKey, spec, rerollSeed]);

  if (!spec) {
    return (
      <div className="p-4 text-sm text-[var(--color-muted)]">
        <div className="mb-2 flex items-center gap-2 font-semibold text-[var(--color-fg)]">
          <Dices size={16} className="text-rose-500" /> Fake data
        </div>
        {parsed.kind === "suggestion" ? (
          <p>
            {parsed.message}
            {parsed.didYouMean && (
              <>
                {" — did you mean "}
                <span className="font-mono text-rose-500">{parsed.didYouMean}</span>?
              </>
            )}
          </p>
        ) : (
          <p>Type a generator (e.g. <span className="font-mono">email</span>,{" "}
            <span className="font-mono">person 50 --csv</span>) or pick one from the list.</p>
        )}
      </div>
    );
  }

  const text = result ? formatValues(result, format) : "";
  const lines = text ? text.split("\n") : [];
  const capped = spec.n > PREVIEW_N;

  return (
    <div className="flex h-full flex-col p-3 text-sm">
      <div className="mb-2 flex items-center justify-between">
        <div className="flex items-center gap-2 font-semibold text-[var(--color-fg)]">
          <Dices size={16} className="text-rose-500" />
          {spec.mode === "template" ? "Template" : spec.generator}
          <span className="text-[var(--color-muted)]">· {spec.n}×</span>
        </div>
        <button
          onClick={() => setRerollSeed(Math.floor(Math.random() * 2 ** 31))}
          className="flex items-center gap-1 rounded px-2 py-1 text-xs text-[var(--color-muted)] hover:bg-[var(--color-surface)] hover:text-rose-500"
          title="Reroll (⌘/Ctrl+R)"
        >
          <RefreshCw size={13} /> Reroll
        </button>
      </div>

      {/* Format chips — switch without re-typing. */}
      <div className="mb-2 flex flex-wrap gap-1">
        {FORMATS.map((f) => (
          <button
            key={f}
            onClick={() => setFmtOverride(f)}
            className={
              "rounded px-2 py-0.5 text-[11px] font-medium uppercase tracking-wide " +
              (format === f
                ? "bg-rose-600 text-white"
                : "bg-[var(--color-surface)] text-[var(--color-muted)] hover:text-rose-500")
            }
          >
            {f}
          </button>
        ))}
      </div>

      {/* Meta chips: locale (fallback highlighted), seed. */}
      <div className="mb-2 flex flex-wrap items-center gap-2 text-[11px]">
        {result && (
          <>
            <span
              className={
                "rounded px-1.5 py-0.5 " +
                (result.fell_back
                  ? "bg-amber-500/20 text-amber-600 dark:text-amber-400"
                  : "bg-[var(--color-surface)] text-[var(--color-muted)]")
              }
              title={
                result.fell_back
                  ? `Not localised for this generator — fell back to EN`
                  : "Locale used"
              }
            >
              {result.locale_used}
              {result.fell_back && " (fallback)"}
            </span>
            <span
              className="rounded bg-[var(--color-surface)] px-1.5 py-0.5 font-mono text-[var(--color-muted)]"
              title="Seed — reuse with --seed= to reproduce"
            >
              seed {result.seed}
            </span>
          </>
        )}
        {loading && <span className="text-[var(--color-muted)]">…</span>}
      </div>

      {(spec.generator === "password" || spec.generator === "iban" || spec.generator === "credit_card") && (
        <p className="mb-2 rounded bg-amber-500/10 px-2 py-1 text-[11px] text-amber-600 dark:text-amber-400">
          {spec.generator === "password"
            ? "Toy value (seedable PRNG) — use pwgen for real passwords."
            : "Syntactically valid but fictional — never a real account."}
        </p>
      )}

      {error ? (
        <div className="rounded bg-red-500/10 px-2 py-1 text-xs text-red-500">{error}</div>
      ) : (
        <div className="min-h-0 flex-1 overflow-auto rounded bg-[var(--color-surface)] p-2 font-mono text-xs leading-relaxed">
          {lines.map((l, i) => (
            <div key={i} className="whitespace-pre-wrap break-all text-[var(--color-fg)]">
              {l || " "}
            </div>
          ))}
          {capped && (
            <div className="mt-1 text-[var(--color-muted)]">
              … preview of {PREVIEW_N} · Enter generates all {spec.n}
            </div>
          )}
        </div>
      )}
    </div>
  );
}
