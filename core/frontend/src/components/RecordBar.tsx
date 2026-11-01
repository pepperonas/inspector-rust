import { useEffect, useRef, useState } from "react";
import { ChevronDown, Pause, Play, Square } from "lucide-react";
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
import { LogicalSize } from "@tauri-apps/api/dpi";
import { listen } from "@tauri-apps/api/event";
import { recordPause, recordResume, recordStatus, recordStop, type RecordStatus } from "../lib/ipc";
import { displayElapsed, formatElapsed } from "../lib/record";
import { useT } from "../lib/i18n";

const EXPANDED_W = 312;
const EXPANDED_H = 54;
const NUB_W = 36;
const NUB_H = 30;

/** Control bar of a running take (window `record-bar`): time, pause, stop, collapse. */
export function RecordBar() {
  const t = useT();
  const [status, setStatus] = useState<Pick<RecordStatus, "phase" | "elapsedMs" | "lowDiskMinutes">>({
    phase: "recording",
    elapsedMs: 0,
    lowDiskMinutes: null,
  });
  const [receivedAt, setReceivedAt] = useState(() => Date.now());
  const [now, setNow] = useState(() => Date.now());
  const [busy, setBusy] = useState(false);
  const [stopping, setStopping] = useState(false);
  const [collapsed, setCollapsed] = useState(false);
  const resizing = useRef(false);

  const apply = (s: RecordStatus) => {
    const n = Date.now();
    setReceivedAt(n);
    setNow(n);
    setStatus({ phase: s.phase, elapsedMs: s.elapsedMs, lowDiskMinutes: s.lowDiskMinutes });
  };

  useEffect(() => {
    let off: (() => void) | undefined;
    let dead = false;
    void listen<RecordStatus>("record-status", (e) => apply(e.payload)).then((u) => {
      if (dead) u();
      else off = u;
    });
    void recordStatus().then(apply).catch(() => undefined);
    return () => {
      dead = true;
      off?.();
    };
  }, []);

  // Local tick between events; only moves the readout while recording.
  useEffect(() => {
    if (status.phase !== "recording") return;
    const id = window.setInterval(() => setNow(Date.now()), 250);
    return () => window.clearInterval(id);
  }, [status.phase]);

  const paused = status.phase === "paused";
  const finishing = stopping || status.phase === "finishing";
  const elapsed = displayElapsed(status.elapsedMs, receivedAt, now, status.phase);

  const togglePause = async () => {
    if (busy || finishing) return;
    setBusy(true);
    try {
      apply(paused ? await recordResume() : await recordPause());
    } catch (e) {
      console.error("record pause/resume", e);
    } finally {
      setBusy(false);
    }
  };

  const stop = () => {
    if (finishing) return;
    setStopping(true);
    recordStop().catch(() => setStopping(false));
  };

  const toggleCollapsed = async () => {
    if (resizing.current) return;
    resizing.current = true;
    const next = !collapsed;
    setCollapsed(next);
    try {
      await getCurrentWebviewWindow().setSize(
        next ? new LogicalSize(NUB_W, NUB_H) : new LogicalSize(EXPANDED_W, EXPANDED_H),
      );
    } catch (e) {
      console.error("resize record bar", e);
      setCollapsed(!next);
    } finally {
      resizing.current = false;
    }
  };

  const chevron = (
    <button
      onClick={toggleCollapsed}
      title={collapsed ? t("rec2.bar.expand") : t("rec2.bar.collapse")}
      aria-label={collapsed ? t("rec2.bar.expand") : t("rec2.bar.collapse")}
      className="flex shrink-0 items-center justify-center rounded-md p-1 text-[var(--color-muted)] hover:bg-[var(--color-bg)] hover:text-[var(--color-fg)]"
    >
      <ChevronDown
        size={16}
        className={
          "transition-transform duration-(--duration-slow) ease-sharp motion-reduce:transition-none " +
          (collapsed ? "" : "rotate-180")
        }
      />
    </button>
  );

  if (collapsed) {
    return (
      <div className="flex h-screen w-screen items-center justify-center rounded-xl border border-[var(--color-border)] bg-[var(--color-surface)] text-[var(--color-fg)] shadow-2xl">
        {chevron}
      </div>
    );
  }

  return (
    <div className="flex h-screen w-screen items-center gap-2.5 rounded-xl border border-[var(--color-border)] bg-[var(--color-surface)] px-3 text-[var(--color-fg)] shadow-2xl">
      {chevron}
      <div data-tauri-drag-region className="flex flex-1 items-center gap-2.5">
        <span className={"h-3 w-3 shrink-0 rounded-full bg-red-600 " + (paused ? "opacity-50" : "recordPulse")} />
        <span className="text-[14px] font-medium tabular-nums" data-testid="rec-elapsed">
          {formatElapsed(elapsed)}
        </span>
        {status.lowDiskMinutes != null && (
          <span className="text-[10px] text-amber-500" title={t("rec2.bar.low_disk", { minutes: status.lowDiskMinutes })}>
            {status.lowDiskMinutes}′
          </span>
        )}
      </div>
      <button
        onClick={togglePause}
        disabled={busy || finishing}
        className="flex items-center gap-1 rounded-lg border border-[var(--color-border)] px-2.5 py-1.5 text-[12px] font-medium hover:bg-[var(--color-bg)] disabled:opacity-60"
      >
        {paused ? <Play size={12} fill="currentColor" /> : <Pause size={12} fill="currentColor" />}
        {paused ? t("rec2.bar.resume") : t("rec2.bar.pause")}
      </button>
      <button
        onClick={stop}
        disabled={finishing}
        className="flex items-center gap-1 rounded-lg bg-red-600 px-2.5 py-1.5 text-[12px] font-medium text-white hover:bg-red-500 disabled:opacity-60"
      >
        <Square size={12} fill="currentColor" /> {finishing ? t("rec2.bar.saving") : t("rec2.bar.stop")}
      </button>
    </div>
  );
}
