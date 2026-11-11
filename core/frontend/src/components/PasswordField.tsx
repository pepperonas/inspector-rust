import { useState } from "react";
import { CheckCircle2 } from "lucide-react";
import { useT } from "../lib/i18n";

/**
 * A password that lives in the keychain: the field never shows it, so the
 * only proof that it is there is this row's state — a green check once it is
 * stored. `onSave` stores the password ("" removes it) and resolves to whether
 * one is stored AFTERWARDS, read back from the backend; a save that did not
 * stick is an error, never a silent nothing.
 */
export function PasswordField({
  stored,
  onSave,
  testId = "password-field",
}: {
  stored: boolean;
  onSave: (password: string) => Promise<boolean>;
  testId?: string;
}) {
  const t = useT();
  const [value, setValue] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  // Counts successful saves, so the check replays its pop each time.
  const [savedTick, setSavedTick] = useState(0);

  const run = async (password: string) => {
    if (busy) return;
    setBusy(true);
    setError(null);
    try {
      const nowStored = await onSave(password);
      if (password !== "" && !nowStored) {
        setError(t("set.pw.not_stored"));
      } else {
        setValue("");
        if (password !== "") setSavedTick((n) => n + 1);
      }
    } catch (e) {
      setError(t("set.pw.failed", { error: String(e) }));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div data-testid={testId}>
      <div className="flex items-center gap-2">
        <input
          type="password"
          className="w-full rounded border border-[var(--color-border)] bg-transparent px-2 py-1 text-[12px]"
          placeholder={stored ? t("set.pw.replace_ph") : t("set.pw.new_ph")}
          value={value}
          autoComplete="new-password"
          onChange={(e) => setValue(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter" && value !== "") {
              e.preventDefault();
              void run(value);
            }
          }}
        />
        <button
          type="button"
          disabled={busy || value === ""}
          className="shrink-0 rounded border border-[var(--color-border)] px-2 py-1 text-[12px] disabled:opacity-40"
          onClick={() => void run(value)}
        >
          {t("common.save")}
        </button>
      </div>
      <div className="mt-1 flex items-center gap-2 text-[11px]">
        {stored ? (
          <>
            <span className="flex items-center gap-1 text-emerald-500" data-testid={`${testId}-stored`}>
              <CheckCircle2 key={savedTick} size={14} className={savedTick > 0 ? "md3-success-pop" : undefined} />
              {t("set.pw.stored")}
            </span>
            <button
              type="button"
              disabled={busy}
              className="text-[var(--color-muted)] underline decoration-dotted hover:text-rose-400 disabled:opacity-40"
              onClick={() => void run("")}
            >
              {t("set.pw.remove")}
            </button>
          </>
        ) : (
          <span className="text-amber-500" data-testid={`${testId}-missing`}>
            {t("set.pw.missing")}
          </span>
        )}
      </div>
      {error && (
        <p className="mt-1 text-[11px] text-rose-400" role="alert">
          {error}
        </p>
      )}
    </div>
  );
}
