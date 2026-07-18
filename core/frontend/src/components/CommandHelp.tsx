/**
 * Inline command help — rendered in the right preview column when the search
 * query is a help request (`?`, `<command>?`, `<prefix>?`). See
 * `lib/commandHelp.ts` for the trigger grammar. Two shapes:
 *   - the global INDEX (`?`): every command grouped by category, each clickable
 *   - a single DOC (`kill?`): synopsis, description, args/flags, examples,
 *     tips, caveats, related-command chips, "since" + a docs/ link.
 *
 * Purely query-derived — the search bar keeps focus. Clicking a command (index
 * row, related chip) navigates by rewriting the query to `<keyword>?`, so help
 * browses without leaving the flow. Content comes from the CommandDoc registry;
 * this component owns no data.
 */
import {
  BookOpen,
  CornerDownLeft,
  Lightbulb,
  AlertTriangle,
  Link2,
  FileText,
} from "lucide-react";
import type { CommandDoc } from "../lib/commandDocs";
import { groupedIndex } from "../lib/commandDocs";
import type { HelpTarget } from "../lib/commandHelp";

interface Props {
  target: HelpTarget;
  /** Rewrite the search query (e.g. to `snitch?`) to browse another command. */
  onNavigate: (query: string) => void;
}

const chip =
  "rounded-full border border-[var(--color-border)] bg-[var(--color-surface)] " +
  "px-2 py-0.5 text-[11px] text-[var(--color-fg)] transition-colors " +
  "hover:border-[var(--color-accent)] hover:text-[var(--color-accent)] md3-press";

function SectionLabel({ children }: { children: React.ReactNode }) {
  return (
    <div className="mt-3 mb-1 text-[10px] font-semibold uppercase tracking-wide text-[var(--color-muted)] opacity-70">
      {children}
    </div>
  );
}

function DocView({ doc, onNavigate }: { doc: CommandDoc; onNavigate: Props["onNavigate"] }) {
  const names = [doc.command, ...doc.aliases];
  return (
    <div className="flex h-full flex-col overflow-y-auto p-4 text-[var(--color-fg)]">
      {/* Header */}
      <div className="flex items-baseline gap-2">
        <BookOpen size={15} className="shrink-0 self-center text-[var(--color-accent)]" />
        <h2 className="text-[15px] font-semibold">{doc.command}</h2>
        <span className="text-[11px] text-[var(--color-muted)]">since v{doc.version_added}</span>
      </div>
      <p className="mt-0.5 text-[12px] text-[var(--color-muted)]">{doc.tagline}</p>

      {/* Synopsis */}
      <SectionLabel>Synopsis</SectionLabel>
      <code className="block rounded-lg border border-[var(--color-border)] bg-[var(--color-surface)] px-2.5 py-1.5 text-[11px] leading-relaxed text-[var(--color-fg)]">
        {doc.synopsis}
      </code>

      {/* Description */}
      <p className="mt-2 text-[12px] leading-relaxed text-[var(--color-fg)]">{doc.description}</p>

      {/* Arguments */}
      {doc.arguments.length > 0 && (
        <>
          <SectionLabel>Arguments</SectionLabel>
          <ul className="flex flex-col gap-1">
            {doc.arguments.map((a) => (
              <li key={a.name} className="text-[12px] leading-snug">
                <code className="text-[var(--color-accent)]">{a.name}</code>
                {!a.required && <span className="ml-1 text-[10px] text-[var(--color-muted)]">optional</span>}
                <span className="text-[var(--color-fg)]"> — {a.description}</span>
                {a.default != null && (
                  <span className="text-[var(--color-muted)]"> (default: {a.default})</span>
                )}
              </li>
            ))}
          </ul>
        </>
      )}

      {/* Flags */}
      {doc.flags.length > 0 && (
        <>
          <SectionLabel>Flags</SectionLabel>
          <ul className="flex flex-col gap-1">
            {doc.flags.map((f) => (
              <li key={f.flag} className="text-[12px] leading-snug">
                <code className="text-[var(--color-accent)]">
                  {f.flag}
                  {f.value_type ? ` ${f.value_type}` : ""}
                </code>
                <span className="text-[var(--color-fg)]"> — {f.description}</span>
                {f.default != null && (
                  <span className="text-[var(--color-muted)]"> (default: {f.default})</span>
                )}
              </li>
            ))}
          </ul>
        </>
      )}

      {/* Examples */}
      <SectionLabel>Examples</SectionLabel>
      <ul className="flex flex-col gap-1.5">
        {doc.examples.map((ex, i) => (
          <li key={i} className="text-[12px] leading-snug">
            <code className="rounded bg-[var(--color-surface)] px-1.5 py-0.5 text-[11px] text-[var(--color-fg)]">
              {ex.input}
            </code>
            <span className="text-[var(--color-muted)]"> → {ex.result}</span>
            {ex.note && <span className="text-[var(--color-muted)] opacity-80"> ({ex.note})</span>}
          </li>
        ))}
      </ul>

      {/* Tips */}
      {doc.tips.length > 0 && (
        <>
          <SectionLabel>Tips</SectionLabel>
          <ul className="flex flex-col gap-1">
            {doc.tips.map((t, i) => (
              <li key={i} className="flex gap-1.5 text-[12px] leading-snug">
                <Lightbulb size={12} className="mt-0.5 shrink-0 text-[var(--color-accent)]" />
                <span>{t}</span>
              </li>
            ))}
          </ul>
        </>
      )}

      {/* Caveats */}
      {doc.caveats.length > 0 && (
        <>
          <SectionLabel>Caveats</SectionLabel>
          <ul className="flex flex-col gap-1">
            {doc.caveats.map((c, i) => (
              <li key={i} className="flex gap-1.5 text-[12px] leading-snug text-[var(--color-muted)]">
                <AlertTriangle size={12} className="mt-0.5 shrink-0 text-amber-400" />
                <span>{c}</span>
              </li>
            ))}
          </ul>
        </>
      )}

      {/* Aliases */}
      {doc.aliases.length > 0 && (
        <p className="mt-3 text-[11px] text-[var(--color-muted)]">
          Aliases: {names.slice(1).map((a) => (
            <code key={a} className="ml-1 text-[var(--color-fg)]">{a}</code>
          ))}
        </p>
      )}

      {/* Related + see also */}
      {(doc.related.length > 0 || doc.see_also) && (
        <div className="mt-2 flex flex-wrap items-center gap-1.5">
          {doc.related.length > 0 && (
            <Link2 size={12} className="text-[var(--color-muted)]" />
          )}
          {doc.related.map((r) => (
            <button key={r} className={chip} onClick={() => onNavigate(`${r}?`)}>
              {r}
            </button>
          ))}
          {doc.see_also && (
            <span className="ml-auto flex items-center gap-1 text-[10px] text-[var(--color-muted)]">
              <FileText size={11} />
              {doc.see_also}
            </span>
          )}
        </div>
      )}

      <p className="mt-auto pt-2 text-[11px] text-[var(--color-muted)]">
        Delete the <code className="text-[var(--color-fg)]">?</code> to run · type
        <code className="mx-1 text-[var(--color-fg)]">?</code>for the full index · Esc closes
      </p>
    </div>
  );
}

function IndexView({ onNavigate }: { onNavigate: Props["onNavigate"] }) {
  const groups = groupedIndex();
  return (
    <div className="flex h-full flex-col overflow-y-auto p-4 text-[var(--color-fg)]">
      <div className="flex items-center gap-2">
        <BookOpen size={15} className="text-[var(--color-accent)]" />
        <h2 className="text-[15px] font-semibold">Command index</h2>
      </div>
      <p className="mt-0.5 text-[12px] text-[var(--color-muted)]">
        Every search-bar command. Click one — or type{" "}
        <code className="text-[var(--color-fg)]">name?</code> — for its full help.
      </p>

      {groups.map((g) => (
        <div key={g.category}>
          <SectionLabel>{g.category}</SectionLabel>
          <div className="flex flex-col gap-0.5">
            {g.docs.map((d) => (
              <button
                key={d.command}
                onClick={() => onNavigate(`${d.command}?`)}
                className="flex items-baseline gap-2 rounded-lg px-2 py-1 text-left transition-colors hover:bg-[var(--color-surface)] md3-press"
              >
                <code className="shrink-0 text-[12px] font-medium text-[var(--color-accent)]">
                  {d.command}
                </code>
                <span className="truncate text-[11px] text-[var(--color-muted)]">{d.tagline}</span>
              </button>
            ))}
          </div>
        </div>
      ))}

      <p className="mt-auto flex items-center gap-1 pt-2 text-[11px] text-[var(--color-muted)]">
        <CornerDownLeft size={11} /> click a command for details · Esc closes
      </p>
    </div>
  );
}

export default function CommandHelp({ target, onNavigate }: Props) {
  return target.kind === "index" ? (
    <IndexView onNavigate={onNavigate} />
  ) : (
    <DocView doc={target.doc} onNavigate={onNavigate} />
  );
}
