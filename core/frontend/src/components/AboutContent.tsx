import { ExternalLink, Heart } from "lucide-react";
import { tSplit, useT } from "../lib/i18n";

interface Props {
  /** App version, e.g. "0.53.0". Pulled from `getVersion()` in the parent. */
  version?: string;
}

/**
 * About-this-app content, rendered **inline at the bottom of the Settings
 * tab** (it used to be a modal — `AboutModal` — but reference content reads
 * better appended to the page than behind a dialog). Static: version,
 * author, license, workflow pitch, tech stack, project links.
 */
export function AboutContent({ version }: Props) {
  const t = useT();
  return (
    <div className="text-[12px]">
      {/* Identity block */}
      <div className="mb-3 rounded-xl border border-[var(--color-border)] bg-[var(--color-surface)] p-3">
        <div className="flex items-baseline justify-between">
          <span className="text-[14px] font-semibold">Inspector Rust</span>
          {version && (
            <span className="font-[var(--font-mono)] text-[12px] text-[var(--color-muted)]">
              v{version}
            </span>
          )}
        </div>
        <div className="mt-0.5 text-[11px] leading-snug text-[var(--color-muted)]">
          {t("about.tagline")}
        </div>
      </div>

      {/* Meta table */}
      <table className="mb-3 w-full">
        <tbody>
          <Meta label={t("about.developer")} value="Martin Pfeffer" />
          <Meta label={t("about.license")} value="MIT" />
          <Meta label={t("about.year")} value="2026" />
          <Meta label={t("about.audience")} value={t("about.audience_value")} />
        </tbody>
      </table>

      {/* Workflow pitch */}
      <div className="mb-3 rounded-xl border border-[var(--color-border)] bg-[var(--color-surface)] p-3 text-[11px] leading-relaxed">
        <div className="mb-1 text-[10px] font-semibold uppercase tracking-wide text-[var(--color-accent)]">
          {t("about.workflow_title")}
        </div>
        {t("about.workflow")}
      </div>

      {/* Tech-stack mini table */}
      <div className="mb-3">
        <div className="mb-1.5 text-[10px] font-semibold uppercase tracking-wide text-[var(--color-muted)]">
          {t("about.tech")}
        </div>
        <table className="w-full text-[11px]">
          <tbody>
            <Tech label="Shell" value="Tauri 2 · Wry / WebView" />
            <Tech label="Backend" value="Rust (stable) · rusqlite · clipboard-rs · enigo" />
            <Tech label={t("about.tech_storage")} value="SQLite · AES-256-GCM · OS keychain" />
            <Tech label="Frontend" value="React 19 · TypeScript 5 · Vite 7 · Tailwind v4" />
            <Tech label={t("about.tech_image")} value="image 0.25 · ort (ONNX) · Apple Vision" />
            <Tech label={t("about.tech_displays")} value="ddc-hi (DDC/CI brightness)" />
            <Tech label={t("about.tech_formats")} value="PNG · JPEG · WebP · GIF · BMP" />
          </tbody>
        </table>
      </div>

      {/* Links */}
      <div className="flex flex-wrap items-center justify-between gap-2 border-t border-[var(--color-border)] pt-2.5 text-[11px] leading-none text-[var(--color-muted)]">
        <a
          href="https://github.com/pepperonas/inspector-rust"
          target="_blank"
          rel="noopener noreferrer"
          className="flex items-center gap-1 leading-none hover:text-[var(--color-accent)]"
        >
          <ExternalLink size={11} className="shrink-0" />
          github.com/pepperonas/inspector-rust
        </a>
        <span className="flex items-center gap-1 leading-none">
          {tSplit("about.made_with", "heart")[0]}<Heart size={10} className="shrink-0 text-red-400" />{tSplit("about.made_with", "heart")[1]}
        </span>
      </div>
    </div>
  );
}

function Meta({ label, value }: { label: string; value: string }) {
  return (
    <tr>
      <td className="w-[110px] py-0.5 pr-2 text-[var(--color-muted)]">{label}</td>
      <td className="py-0.5">{value}</td>
    </tr>
  );
}

function Tech({ label, value }: { label: string; value: string }) {
  return (
    <tr>
      <td className="w-[80px] py-0.5 pr-2 align-top text-[var(--color-muted)]">
        {label}
      </td>
      <td className="py-0.5 font-[var(--font-mono)]">{value}</td>
    </tr>
  );
}
