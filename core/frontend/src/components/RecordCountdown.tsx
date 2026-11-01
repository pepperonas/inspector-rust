import { useEffect, useState } from "react";

/** Full-screen transparent, click-through 3-2-1 (window `record-countdown`). */
export function RecordCountdown() {
  const [n, setN] = useState(3);
  useEffect(() => {
    const id = window.setInterval(() => setN((v) => (v > 1 ? v - 1 : v)), 1000);
    return () => window.clearInterval(id);
  }, []);
  return (
    <div className="flex h-screen w-screen items-center justify-center bg-transparent">
      <span
        key={n}
        className="rec-count-digit select-none text-[220px] font-bold leading-none tabular-nums text-white"
        style={{ textShadow: "0 8px 48px rgba(0,0,0,0.55)" }}
        aria-live="assertive"
      >
        {n}
      </span>
    </div>
  );
}
