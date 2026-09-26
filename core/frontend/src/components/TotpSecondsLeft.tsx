import { useEffect, useState } from "react";
import { secondsRemaining } from "../lib/totp";

/** Live "N" seconds until the code rolls over. Ticks ONLY itself once a
 *  second — the surrounding row/list is no longer re-rendered per second. */
export function TotpSecondsLeft({ period }: { period: number }) {
  const [s, setS] = useState(() => secondsRemaining(period, Date.now()));
  useEffect(() => {
    const tick = () => setS(secondsRemaining(period, Date.now()));
    tick();
    const id = window.setInterval(tick, 1000);
    return () => window.clearInterval(id);
  }, [period]);
  return <>{s}</>;
}
