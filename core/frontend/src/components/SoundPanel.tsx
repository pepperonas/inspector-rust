import { useEffect, useState } from "react";
import { Volume2, Check } from "lucide-react";
import { listAudioOutputs, setAudioOutput, type AudioDevice } from "../lib/ipc";

/**
 * In-popup audio **output device** picker rendered in the right preview column
 * — same arrow-key model as `BrightnessPanel`, entered by pressing Enter on the
 * `sound` command row.
 *
 * Keyboard (when `focused`):
 *   ↑ / ↓   move the selection
 *   Enter   switch the system default output to the selected device
 *   Esc     leave sound mode (`onExit`)
 *
 * macOS uses CoreAudio; Windows uses the MMDevice + IPolicyConfig path.
 */
export function SoundPanel({
  focused,
  onExit,
}: {
  focused: boolean;
  onExit: () => void;
}) {
  const [devices, setDevices] = useState<AudioDevice[] | null>(null);
  const [sel, setSel] = useState(0);
  const [error, setError] = useState<string | null>(null);

  const load = (selectDefault: boolean) => {
    listAudioOutputs()
      .then((d) => {
        setDevices(d);
        if (selectDefault) {
          const i = d.findIndex((x) => x.is_default);
          if (i >= 0) setSel(i);
        }
      })
      .catch((e) => setError(String(e)));
  };

  useEffect(() => {
    load(true);
  }, []);

  // Arrow / Enter / Esc handling while the picker owns the keyboard. Inlined in
  // the effect (re-subscribes on sel/devices change) so it reads fresh state.
  useEffect(() => {
    if (!focused) return;
    const onKey = (e: KeyboardEvent) => {
      const list = devices ?? [];
      switch (e.key) {
        case "ArrowUp":
          e.preventDefault();
          e.stopPropagation();
          setSel((s) => Math.max(0, s - 1));
          break;
        case "ArrowDown":
          e.preventDefault();
          e.stopPropagation();
          setSel((s) => Math.min(list.length - 1, s + 1));
          break;
        case "Enter": {
          e.preventDefault();
          e.stopPropagation();
          const dev = list[sel];
          if (dev && !dev.is_default) {
            setAudioOutput(dev.id)
              .then(() => load(false)) // refresh to move the default marker
              .catch((err) => setError(String(err)));
          }
          break;
        }
        case "Escape":
          e.preventDefault();
          e.stopPropagation();
          onExit();
          break;
      }
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [focused, devices, sel, onExit]);

  // Keep the selection in range if the list shrinks.
  useEffect(() => {
    const len = devices?.length ?? 0;
    if (sel >= len && len > 0) setSel(len - 1);
  }, [devices, sel]);

  const clickDevice = (i: number) => {
    setSel(i);
    const dev = devices?.[i];
    if (dev && !dev.is_default) {
      setAudioOutput(dev.id)
        .then(() => load(false))
        .catch((err) => setError(String(err)));
    }
  };

  return (
    <div className="flex h-full flex-col gap-3 overflow-y-auto p-4 text-[var(--color-fg)]">
      <div className="flex items-center gap-2 text-[13px] font-medium">
        <Volume2 size={15} className="text-[var(--color-accent)]" /> Output device
      </div>

      {error ? (
        <p className="text-[12px] text-[var(--color-muted)]">{error}</p>
      ) : devices === null ? (
        <p className="text-[12px] text-[var(--color-muted)]">Reading devices…</p>
      ) : devices.length === 0 ? (
        <p className="text-[12px] text-[var(--color-muted)]">No output devices found.</p>
      ) : (
        <div className="flex flex-col gap-1">
          {devices.map((d, i) => {
            const active = focused && i === sel;
            return (
              <button
                key={d.id}
                type="button"
                onClick={() => clickDevice(i)}
                className={
                  "flex items-center justify-between gap-2 rounded-lg border px-3 py-2 text-left text-[12px] transition-colors " +
                  (active
                    ? "border-[var(--color-accent)] bg-[var(--color-accent)]/10"
                    : "border-[var(--color-border)] hover:bg-[var(--color-surface)]")
                }
              >
                <span className="truncate pr-2">{d.name}</span>
                {d.is_default && (
                  <span className="flex shrink-0 items-center gap-1 text-[11px] text-[var(--color-accent)]">
                    <Check size={13} /> default
                  </span>
                )}
              </button>
            );
          })}
          <p className="mt-1 text-[11px] text-[var(--color-muted)]">
            {focused ? (
              <>↑ ↓ select · Enter switch output · Esc close</>
            ) : (
              <>Press Enter on the sound row to pick the output.</>
            )}
          </p>
        </div>
      )}
    </div>
  );
}
