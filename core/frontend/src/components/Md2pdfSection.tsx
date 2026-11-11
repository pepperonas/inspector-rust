import { useEffect, useState } from "react";
import { FileText } from "lucide-react";
import { Section } from "./SettingsSection";
import { InlineMd } from "./InlineMd";
import { getMd2pdfFooter, setMd2pdfFooter, type Md2pdfFooter } from "../lib/ipc";
import { useT } from "../lib/i18n";

/**
 * Settings → Markdown → PDF: the footer line `md2pdf --sign` prints on every
 * page. The field saves on blur or Enter; the preview comes from Rust (the
 * one place that fills the placeholders), so it is what the PDF will carry.
 */
export function Md2pdfSection() {
  const t = useT();
  const [info, setInfo] = useState<Md2pdfFooter | null>(null);
  const [draft, setDraft] = useState("");
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    getMd2pdfFooter()
      .then((i) => {
        setInfo(i);
        setDraft(i.template);
      })
      .catch((e) => setError(String(e)));
  }, []);

  const save = async (value: string) => {
    if (info && value.trim() === info.template) return;
    setError(null);
    try {
      const next = await setMd2pdfFooter(value);
      setInfo(next);
      setDraft(next.template);
    } catch (e) {
      setError(String(e));
    }
  };

  return (
    <Section
      icon={<FileText size={16} className="text-[var(--color-accent)]" />}
      id="md2pdf"
      title={t("settings.section.md2pdf")}
      subtitle={t("set.md2pdf.subtitle")}
    >
      <label className="mb-1 block text-[11px] text-[var(--color-muted)]" htmlFor="md2pdf-footer">
        {t("set.md2pdf.footer")}
      </label>
      <div className="flex items-center gap-2">
        <input
          id="md2pdf-footer"
          type="text"
          value={draft}
          placeholder={info?.default_template ?? ""}
          onChange={(e) => setDraft(e.target.value)}
          onBlur={() => void save(draft)}
          onKeyDown={(e) => {
            if (e.key === "Enter") {
              e.preventDefault();
              void save(draft);
            }
          }}
          className="min-w-0 flex-1 rounded-lg border border-[var(--color-border)] bg-[var(--color-surface)] px-3 py-1.5 text-[12px] outline-none focus:border-[var(--color-accent)]"
          data-testid="md2pdf-footer"
        />
        {info && info.template !== "" && (
          <button
            type="button"
            onClick={() => void save("")}
            className="md3-press shrink-0 rounded-lg border border-[var(--color-border)] px-3 py-1.5 text-[12px] hover:border-[var(--color-accent)]"
          >
            {t("set.md2pdf.reset")}
          </button>
        )}
      </div>
      {info && (
        <p className="mt-2 text-[12px]" data-testid="md2pdf-footer-preview">
          <span className="text-[var(--color-muted)]">{t("set.md2pdf.preview")} </span>
          {info.preview}
        </p>
      )}
      <p className="mt-1 text-[11px] leading-5 text-[var(--color-muted)]">
        <InlineMd text={t("set.md2pdf.placeholders")} />
      </p>
      {error && <p className="mt-1 text-[11px] text-rose-400">{error}</p>}
    </Section>
  );
}
