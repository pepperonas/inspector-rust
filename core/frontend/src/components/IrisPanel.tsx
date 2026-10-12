import { useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { Siren } from "lucide-react";
import { irisStatus } from "../lib/ipc";
import { formatSpl, meterFraction, METER_MIN_SPL, METER_MAX_SPL } from "../lib/iris";
import { t, useT } from "../lib/i18n";
import { InlineMd } from "./InlineMd";

/**
 * Calibration panel for the `iris` command, rendered in the right preview
 * column while iris is armed.
 *
 * It exists because a threshold in SPL is meaningless until you can see what
 * your room actually reads: the meter shows the live level against a marker at
 * your threshold, so picking a number is a two-second job instead of guesswork.
 *
 * Live levels come from the `iris-level` event, which only streams while the
 * capture is running — hence this panel is only ever shown for an *armed*
 * session. Monitoring is a background job, so closing the popup does not
 * disarm; `iris` again (or `iris 0`) plus Enter switches it off.
 *
 * **This panel deliberately takes no keyboard focus.** An earlier version gave
 * it focus on arm so ←/→ could retune — which disabled the list navigation, so
 * the second Enter never reached the dispatcher and the toggle appeared dead
 * (v0.102.2). The threshold is edited in the search bar instead (`iris 60`,
 * applied live and debounced), which is both more discoverable and keeps Enter
 * doing exactly one thing.
 */
export function IrisPanel() {
  useT();
  const [spl, setSpl] = useState<number | null>(null);
  const [over, setOver] = useState(false);
  const [threshold, setThreshold] = useState<number | null>(null);
  const [peak, setPeak] = useState(0);
  useEffect(() => {
    let alive = true;
    irisStatus()
      .then((s) => {
        if (!alive) return;
        setThreshold(s.threshold_spl);
        setOver(s.over);
      })
      .catch(() => {});
    const un = listen<{ spl: number; over: boolean; threshold: number }>(
      "iris-level",
      (e) => {
        if (!alive || !e.payload) return;
        setSpl(e.payload.spl);
        setOver(!!e.payload.over);
        // The backend is the source of truth for the threshold, so a retune
        // from anywhere (a second `iris 60`) reflects here too.
        setThreshold(e.payload.threshold);
        setPeak((p) => Math.max(p * 0.985, e.payload.spl));
      },
    );
    return () => {
      alive = false;
      un.then((f) => f()).catch(() => {});
    };
  }, []);


  const thr = threshold ?? 55;
  const level = meterFraction(spl ?? METER_MIN_SPL);
  const thrPos = meterFraction(thr);
  const peakPos = meterFraction(peak);

  return (
    <div className="flex h-full flex-col gap-4 overflow-y-auto p-4">
      <div className="flex items-center gap-2">
        <Siren
          size={18}
          className={over ? "text-rose-400" : "text-[var(--color-muted)]"}
        />
        <span className="text-sm font-semibold">{t("iris.iris")}</span>
        <span
          className={`ml-auto rounded-full px-2 py-0.5 text-[11px] font-medium ${
            over
              ? "bg-rose-600 text-white"
              : "bg-[var(--color-surface)] text-[var(--color-muted)]"
          }`}
        >
          {over ? t("iris.over_threshold") : t("iris.armed_quiet")}
        </span>
      </div>

      {/* Live level */}
      <div>
        <div className="flex items-baseline gap-2">
          <span
            className={`font-mono text-4xl font-bold tabular-nums ${
              over ? "text-rose-400" : ""
            }`}
          >
            {spl === null ? "—" : formatSpl(spl)}
          </span>
          <span className="text-xs text-[var(--color-muted)]">{t("iris.db_spl")}</span>
        </div>

        {/* Meter: fill = live level, tick = threshold, ghost = decaying peak. */}
        <div className="relative mt-2 h-4 w-full overflow-hidden rounded-full bg-[var(--color-surface)]">
          <div
            className={`h-full rounded-full transition-[width] duration-100 ${
              over ? "bg-rose-500" : "bg-[var(--color-accent)]"
            }`}
            style={{ width: `${level * 100}%` }}
          />
          <div
            className="absolute top-0 h-full w-px bg-[var(--color-fg)]/40"
            style={{ left: `${peakPos * 100}%` }}
          />
          <div
            className="absolute top-0 h-full w-0.5 bg-rose-300"
            style={{ left: `${thrPos * 100}%` }}
            title={t("iris.threshold_db", { v: formatSpl(thr) })}
          />
        </div>
        <div className="mt-1 flex justify-between text-[10px] text-[var(--color-muted)]">
          <span>{METER_MIN_SPL}</span>
          <span>{t("iris.threshold_db", { v: formatSpl(thr) })}</span>
          <span>{METER_MAX_SPL}</span>
        </div>
      </div>

      <div className="rounded-lg bg-[var(--color-surface)] p-3 text-xs leading-relaxed text-[var(--color-muted)]">
        <div className="mb-1 font-medium text-[var(--color-fg)]">
          {t("iris.change_the_threshold_type_a")}
        </div>
        {t("iris.when_the_level_exceeds_the")}
        <div className="mt-1">
          <InlineMd text={t("iris.off_hint")} />
        </div>
      </div>
    </div>
  );
}
