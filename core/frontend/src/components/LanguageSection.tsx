import { useEffect, useState } from "react";
import { Languages } from "lucide-react";
import { Section } from "./SettingsSection";
import { getLanguage, setLanguage } from "../lib/ipc";
import { loadLanguage, useT, type LangSetting, type LanguageInfo } from "../lib/i18n";

/**
 * Settings → Language. Three options: follow the OS, English, Deutsch. The
 * language names are written in their own language (a German speaker stuck in
 * an English UI must still recognise "Deutsch"). Switching applies live in
 * every window: Rust emits `language-changed`, each window loads its catalogue.
 */
export function LanguageSection() {
  const t = useT();
  const [info, setInfo] = useState<LanguageInfo | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    getLanguage()
      .then(setInfo)
      .catch((e) => setError(String(e)));
  }, []);

  const change = async (setting: LangSetting) => {
    if (busy || info?.setting === setting) return;
    setBusy(true);
    setError(null);
    try {
      const next = await setLanguage(setting);
      setInfo(next);
      // The event reaches this window too; loading here as well makes the
      // switch instant even if the event is slow.
      await loadLanguage(next.effective);
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };

  const options: { value: LangSetting; label: string }[] = [
    {
      value: "system",
      label: t("settings.language.system", {
        lang: info ? t(info.system === "de" ? "language.de" : "language.en") : "…",
      }),
    },
    { value: "en", label: t("language.en") },
    { value: "de", label: t("language.de") },
  ];

  return (
    <div className="mb-6">
      <Section
        icon={<Languages size={16} className="text-[var(--color-accent)]" />}
        id="language"
        title={t("settings.language.title")}
        subtitle={t("settings.language.description")}
      >
        <div
          role="radiogroup"
          aria-label={t("settings.language.title")}
          className="flex w-fit overflow-hidden rounded-lg border border-[var(--color-border)]"
        >
          {options.map(({ value, label }) => {
            const active = info?.setting === value;
            return (
              <button
                key={value}
                type="button"
                role="radio"
                aria-checked={active}
                disabled={info === null || busy}
                onClick={() => void change(value)}
                className={
                  "px-3 py-1.5 text-[12px] transition-colors " +
                  "border-r border-[var(--color-border)] last:border-r-0 " +
                  (active
                    ? "bg-[var(--color-accent)] text-[var(--color-accent-fg)]"
                    : "bg-[var(--color-surface)] text-[var(--color-muted)] hover:text-[var(--color-fg)]")
                }
              >
                {label}
              </button>
            );
          })}
        </div>
        {error && <p className="mt-2 text-[11px] text-red-500">{error}</p>}
      </Section>
    </div>
  );
}
