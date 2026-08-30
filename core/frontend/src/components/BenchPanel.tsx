import { useCallback, useEffect, useState } from "react";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { Cpu, Play, Trash2, Upload, Check } from "lucide-react";
import {
  benchPlan, benchRun, benchHistory, benchDelete, benchImport, benchExport,
  setSuppressHide, type BenchPlan,
} from "../lib/ipc";
import {
  compareRows, mismatchedSchemas, formatDelta, formatRate, machineLabel, osLabel,
  UNKNOWN, NOISE_FLOOR_PCT, type BenchRun,
} from "../lib/bench";
import { humanBytes } from "../lib/format-stats";
import { ExportRow } from "./ExportRow";
import { confirmDialog } from "../lib/confirm";

/**
 * `benchmark` / `performance` — a CPU benchmark in the shape of Geekbench.
 *
 * The flow is deliberately two-step: typing the command only shows a PREVIEW
 * of what would happen, and nothing is measured until the start button is
 * pressed. The run saturates every core for ~9 s, which is not something to
 * begin by accident.
 */
export function BenchPanel({ focused, onExit }: { focused: boolean; onExit: () => void }) {
  const [plan, setPlan] = useState<BenchPlan | null>(null);
  const [phase, setPhase] = useState<"preview" | "running" | "done">("preview");
  const [progress, setProgress] = useState<{ done: number; total: number; name: string } | null>(null);
  const [run, setRun] = useState<BenchRun | null>(null);
  const [history, setHistory] = useState<BenchRun[]>([]);
  const [picked, setPicked] = useState<string[]>([]);
  const [err, setErr] = useState<string | null>(null);
  const [exporting, setExporting] = useState<string | null>(null);
  const [exported, setExported] = useState<string | null>(null);

  const reload = useCallback(() => {
    benchHistory().then(setHistory).catch(() => undefined);
  }, []);

  useEffect(() => {
    benchPlan().then(setPlan).catch((e) => setErr(String(e)));
    reload();
  }, [reload]);

  useEffect(() => {
    let un: UnlistenFn | undefined;
    void listen<{ done: number; total: number; name: string }>("bench-progress", (e) =>
      setProgress(e.payload),
    ).then((f) => (un = f));
    return () => un?.();
  }, []);

  const start = useCallback(async () => {
    if (phase === "running") return;
    setErr(null);
    setPhase("running");
    setProgress(null);
    // The measurement must not be interrupted by the popup hiding on a stray
    // focus loss — that would abandon the run half-way and waste the 9 s.
    void setSuppressHide(true).catch(() => undefined);
    try {
      const r = await benchRun();
      setRun(r);
      setPicked([r.id]);
      setPhase("done");
      reload();
    } catch (e) {
      setErr(String(e));
      setPhase("preview");
    } finally {
      void setSuppressHide(false).catch(() => undefined);
    }
  }, [phase, reload]);

  useEffect(() => () => void setSuppressHide(false).catch(() => undefined), []);

  useEffect(() => {
    if (!focused) return;
    const onKey = (e: KeyboardEvent) => {
      const tgt = e.target as HTMLElement | null;
      const typing = tgt && (tgt.tagName === "INPUT" || tgt.isContentEditable);
      if (e.key === "Escape") {
        e.preventDefault();
        e.stopPropagation();
        onExit();
      } else if (!typing && (e.metaKey || e.ctrlKey) && (e.key === "b" || e.key === "B")) {
        e.preventDefault();
        void start();
      }
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [focused, onExit, start]);

  const doImport = async () => {
    const { open } = await import("@tauri-apps/plugin-dialog");
    void setSuppressHide(true).catch(() => undefined);
    try {
      const sel = await open({ multiple: false, filters: [{ name: "Benchmark", extensions: ["json"] }] });
      if (typeof sel === "string") {
        await benchImport(sel);
        reload();
      }
    } catch (e) {
      setErr(String(e));
    } finally {
      void setSuppressHide(false).catch(() => undefined);
    }
  };

  const toggle = (id: string) =>
    setPicked((cur) => (cur.includes(id) ? cur.filter((x) => x !== id) : [...cur, id]));

  const chosen = picked.map((id) => history.find((h) => h.id === id)).filter(Boolean) as BenchRun[];
  const stale = mismatchedSchemas(chosen);

  return (
    <div className="flex h-full flex-col gap-3 overflow-y-auto p-4 text-[var(--color-fg)]">
      <div className="flex items-center gap-2 text-[13px] font-medium">
        <Cpu size={15} className="shrink-0 text-[var(--color-accent)]" />
        <span>Benchmark</span>
        {plan && <span className="text-[11px] text-[var(--color-muted)]">{machineLabel(plan.machine)}</span>}
      </div>

      {err && <p className="text-[11px] text-rose-400">{err}</p>}

      {phase === "preview" && plan && (
        <>
          <div className="rounded-lg border border-[var(--color-border)] bg-[var(--color-surface)] p-3 text-[11px] leading-5">
            <p className="font-medium">Geplanter Lauf</p>
            <p className="text-[var(--color-muted)]">
              {plan.workloads.length} Disziplinen, je einmal auf einem Kern und einmal über{" "}
              {plan.threads} Threads — zusammen etwa {Math.round(plan.estimated_seconds)} Sekunden,
              in denen alle Kerne ausgelastet sind.
            </p>
            <ul className="mt-1.5 flex flex-wrap gap-1">
              {plan.workloads.map((w) => (
                <li key={w} className="rounded-full border border-[var(--color-border)] px-2 py-0.5 text-[10px]">
                  {w}
                </li>
              ))}
            </ul>
            <p className="mt-2 text-[var(--color-muted)]">
              1000 Punkte entsprechen <b>{plan.baseline_machine}</b>. Wiederholte Läufe streuen um
              etwa {NOISE_FLOOR_PCT} % — kleinere Unterschiede sind Rauschen.
            </p>
          </div>
          <MachineTable m={plan.machine} />
          <button
            type="button"
            onClick={() => void start()}
            className="md3-press flex items-center justify-center gap-1.5 rounded-lg bg-[var(--color-accent)] px-3 py-2 text-[12px] font-medium text-[var(--color-accent-fg)]"
          >
            <Play size={13} /> Benchmark starten
          </button>
          <p className="-mt-1 text-center text-[10px] text-[var(--color-muted)]">
            Nichts wird gemessen, bevor du hier bestätigst.
          </p>
        </>
      )}

      {phase === "running" && (
        <div className="rounded-lg border border-[var(--color-border)] p-3">
          <p className="text-[12px] font-medium">Messung läuft …</p>
          <p className="text-[11px] text-[var(--color-muted)]">
            {progress ? `${progress.done + 1} / ${progress.total} · ${progress.name}` : "startet …"}
          </p>
          <div className="mt-2 h-1.5 overflow-hidden rounded-full bg-[var(--color-surface)]">
            <div
              className="h-full rounded-full bg-[var(--color-accent)] transition-[width]"
              style={{ width: progress ? `${((progress.done + 1) / progress.total) * 100}%` : "4%" }}
            />
          </div>
        </div>
      )}

      {phase === "done" && run && (
        <>
          <div className="grid grid-cols-2 gap-2">
            <Score label="Single-Core" value={run.single.score} />
            <Score label="Multi-Core" value={run.multi.score} />
          </div>
          <WorkTable title="Single-Core" run={run} section="single" />
          <WorkTable title="Multi-Core" run={run} section="multi" />
          <MachineTable m={run.machine} />
        </>
      )}

      {history.length > 0 && (
        <section className="flex flex-col gap-1.5">
          <div className="flex items-center justify-between">
            <p className="text-[11px] uppercase tracking-wide text-[var(--color-muted)]">
              Gespeicherte Läufe — zum Vergleichen mehrere wählen
            </p>
            <button
              type="button"
              onClick={() => void doImport()}
              title="Lauf eines anderen Geräts einlesen (JSON)"
              className="rounded-md p-1 text-[var(--color-muted)] hover:text-[var(--color-fg)]"
            >
              <Upload size={13} />
            </button>
          </div>
          <ul className="flex flex-col gap-0.5">
            {history.map((h) => (
              <li key={h.id} className="flex items-center gap-2 text-[11px]">
                <button
                  type="button"
                  onClick={() => toggle(h.id)}
                  className={
                    "flex h-4 w-4 shrink-0 items-center justify-center rounded border " +
                    (picked.includes(h.id)
                      ? "border-[var(--color-accent)] bg-[var(--color-accent)] text-[var(--color-accent-fg)]"
                      : "border-[var(--color-border)]")
                  }
                >
                  {picked.includes(h.id) && <Check size={10} />}
                </button>
                <span className="min-w-0 flex-1 truncate">
                  {machineLabel(h.machine)}{" "}
                  <span className="text-[var(--color-muted)]">· {osLabel(h.machine)}</span>
                </span>
                <span className="shrink-0 font-[var(--font-mono)]">
                  {h.single.score} / {h.multi.score}
                </span>
                <button
                  type="button"
                  onClick={async () => {
                    if (await confirmDialog("Diesen Lauf löschen?", "Benchmark")) {
                      await benchDelete(h.id).catch((e) => setErr(String(e)));
                      setPicked((c) => c.filter((x) => x !== h.id));
                      reload();
                    }
                  }}
                  className="shrink-0 rounded p-0.5 text-[var(--color-muted)] hover:text-rose-400"
                >
                  <Trash2 size={11} />
                </button>
              </li>
            ))}
          </ul>
        </section>
      )}

      {chosen.length > 1 && <Comparison runs={chosen} stale={stale} />}

      {chosen.length > 0 && (
        <ExportRow
          formats={["html", "pdf"]}
          busy={exporting}
          done={exported}
          label={chosen.length > 1 ? "Vergleich exportieren:" : "Export:"}
          onExport={(f) => {
            setExporting(f);
            setExported(null);
            benchExport(picked, f)
              .then((p) => setExported(p.split("/").pop() ?? p))
              .catch((e) => setExported(String(e)))
              .finally(() => setExporting(null));
          }}
        />
      )}
    </div>
  );
}

function Score({ label, value }: { label: string; value: number }) {
  return (
    <div className="rounded-lg border border-[var(--color-border)] p-2.5">
      <p className="text-[10px] uppercase tracking-wide text-[var(--color-muted)]">{label}</p>
      <p className="font-[var(--font-mono)] text-[22px] font-semibold leading-tight">{value}</p>
    </div>
  );
}

function WorkTable({ title, run, section }: { title: string; run: BenchRun; section: "single" | "multi" }) {
  return (
    <section>
      <p className="mb-1 text-[11px] uppercase tracking-wide text-[var(--color-muted)]">{title}</p>
      <table className="w-full text-[11px]">
        <tbody>
          {run[section].workloads.map((w) => (
            <tr key={w.id} className="border-b border-[var(--color-border)] last:border-0">
              <td className="py-1">{w.name}</td>
              <td className="py-1 text-right text-[var(--color-muted)]">{formatRate(w.rate, w.unit)}</td>
              <td className="py-1 pl-3 text-right font-[var(--font-mono)] font-semibold">{w.score}</td>
            </tr>
          ))}
        </tbody>
      </table>
    </section>
  );
}

function MachineTable({ m }: { m: BenchRun["machine"] }) {
  const rows: [string, string][] = [
    ["Gerät", m.device_model ?? UNKNOWN],
    ["Betriebssystem", osLabel(m)],
    ["Architektur", m.arch ?? UNKNOWN],
    ["Prozessor", m.cpu_brand ?? UNKNOWN],
    ["Kerne", m.physical_cores !== null || m.logical_cores !== null
      ? `${m.physical_cores ?? "?"} physisch · ${m.logical_cores ?? "?"} logisch`
      : UNKNOWN],
    ["Arbeitsspeicher", m.mem_total_bytes !== null ? humanBytes(m.mem_total_bytes) : UNKNOWN],
  ];
  return (
    <section>
      <p className="mb-1 text-[11px] uppercase tracking-wide text-[var(--color-muted)]">Maschine</p>
      <table className="w-full text-[11px]">
        <tbody>
          {rows.map(([k, v]) => (
            <tr key={k}>
              <td className="py-0.5 text-[var(--color-muted)]">{k}</td>
              <td className="py-0.5 text-right">{v}</td>
            </tr>
          ))}
        </tbody>
      </table>
    </section>
  );
}

function Comparison({ runs, stale }: { runs: BenchRun[]; stale: BenchRun[] }) {
  return (
    <section className="flex flex-col gap-2">
      <p className="text-[11px] uppercase tracking-wide text-[var(--color-muted)]">
        Vergleich — Abweichung gegen den ersten Lauf
      </p>
      {stale.length > 0 && (
        <p className="rounded-md border border-amber-500/40 bg-amber-500/10 px-2 py-1 text-[10px] text-amber-500">
          {stale.length} Lauf/Läufe stammen aus einer anderen Test-Fassung — die Disziplinen sind
          nicht identisch, der Vergleich ist mit Vorsicht zu lesen.
        </p>
      )}
      {(["single", "multi"] as const).map((sec) => (
        <div key={sec}>
          <p className="mb-1 text-[10px] font-medium">{sec === "single" ? "Single-Core" : "Multi-Core"}</p>
          <table className="w-full text-[10px]">
            <thead>
              <tr className="text-[var(--color-muted)]">
                <th className="text-left font-normal">Disziplin</th>
                {runs.map((r) => (
                  <th key={r.id} className="max-w-[90px] truncate text-right font-normal">
                    {machineLabel(r.machine)}
                  </th>
                ))}
              </tr>
            </thead>
            <tbody>
              {compareRows(runs, sec).map((row) => (
                <tr key={row.id} className="border-b border-[var(--color-border)] last:border-0">
                  <td className="py-1">{row.name}</td>
                  {row.cells.map((c, i) => (
                    <td key={i} className="py-1 text-right font-[var(--font-mono)]">
                      {c === null ? (
                        <span className="text-[var(--color-muted)]">—</span>
                      ) : (
                        <>
                          {c.score}
                          {row.deltas[i] !== null && (
                            <span className="ml-1 text-[9px] text-[var(--color-muted)]">
                              {formatDelta(row.deltas[i])}
                            </span>
                          )}
                        </>
                      )}
                    </td>
                  ))}
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      ))}
    </section>
  );
}
