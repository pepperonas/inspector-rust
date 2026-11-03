import { t } from "../lib/i18n";

export type LangChoice = "de" | "en" | "auto";

const CHOICES: { id: LangChoice; label: () => string }[] = [
  { id: "de", label: () => "DE" },
  { id: "en", label: () => "EN" },
  { id: "auto", label: () => t("mtg.lang.auto_short") },
];

/** German / English / automatic — switchable while a recording runs. */
export function LangChips({ value, onChange }: { value: LangChoice; onChange: (l: LangChoice) => void }) {
  return (
    <div className="inline-flex gap-1" role="radiogroup" aria-label={t("mtg.set.lang")} data-testid="lang-chips">
      {CHOICES.map((c) => (
        <button
          key={c.id}
          type="button"
          role="radio"
          aria-checked={value === c.id}
          data-testid={`lang-${c.id}`}
          onClick={() => value !== c.id && onChange(c.id)}
          className={`rounded-md px-2 py-0.5 text-[11px] font-semibold ${
            value === c.id
              ? "bg-[var(--color-accent)] text-[var(--color-accent-fg)]"
              : "border border-[var(--color-border)] text-[var(--color-muted)] hover:text-[var(--color-fg)]"
          }`}
        >
          {c.label()}
        </button>
      ))}
    </div>
  );
}

/** A dictation's language as stored on the job (`null` = the German default). Pure. */
export function liveLang(language: string | null | undefined): LangChoice {
  const l = (language ?? "").trim().toLowerCase();
  if (l === "en" || l === "english" || l === "englisch") return "en";
  if (l === "auto") return "auto";
  return "de";
}
