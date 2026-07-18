import { useEffect, useState } from "react";
import { ShieldAlert, Terminal, AlertTriangle, ShieldCheck } from "lucide-react";
import { parseSecCommand, type SecCatalog, type SecDefaults } from "../lib/sec";
import { secPathExists } from "../lib/ipc";

const ACK_KEY = "sec-authorization-acknowledged";

const TAG_LABEL: Record<string, string> = {
  "long-running": "long-running",
  loud: "loud / noisy",
  "writes-data": "writes data out",
  "needs-root": "needs root",
  "jumbo-only": "John Jumbo only",
};

/**
 * Preview pane for a `sec` command builder (rendered when the `command` row's
 * kind is "sec", like FakerPreview). Shows the built command line, a plain-
 * English flag cheat-sheet, sharp/scope chips, tool notes, and a one-time
 * authorization reminder. Enter copies; ⌘/Ctrl+Enter hands off to the terminal.
 */
export function SecPreview({
  keyword,
  arg,
  catalog,
  defaults,
}: {
  keyword: string;
  arg: string;
  catalog: SecCatalog;
  defaults: SecDefaults;
}) {
  const [ackd, setAckd] = useState(true);
  useEffect(() => {
    try {
      setAckd(localStorage.getItem(ACK_KEY) === "1");
    } catch {
      setAckd(true);
    }
  }, []);
  const dismiss = () => {
    try {
      localStorage.setItem(ACK_KEY, "1");
    } catch {
      /* best-effort */
    }
    setAckd(true);
  };

  const parsed = parseSecCommand(keyword, arg, catalog, defaults);

  // Debounced existence check for the wordlist this command would use.
  const wl = parsed.kind === "built" ? (parsed.values.wordlist ?? "").trim() : "";
  const [wlExists, setWlExists] = useState<boolean | null>(null);
  useEffect(() => {
    if (!wl) {
      setWlExists(null);
      return;
    }
    const t = window.setTimeout(() => {
      secPathExists(wl)
        .then(setWlExists)
        .catch(() => setWlExists(null));
    }, 250);
    return () => window.clearTimeout(t);
  }, [wl]);

  const Header = (
    <>
      {defaults.scope_note.trim() && (
        <div className="mb-2 flex items-start gap-2 rounded bg-[var(--color-surface)] px-2 py-1 text-[11px] text-[var(--color-muted)]">
          <ShieldCheck size={13} className="mt-0.5 shrink-0 text-emerald-500" />
          <span>Scope: {defaults.scope_note}</span>
        </div>
      )}
      {!ackd && (
        <div className="mb-2 rounded border border-amber-500/40 bg-amber-500/10 px-2 py-1.5 text-[11px] text-amber-600 dark:text-amber-400">
          Only run these against systems you have written authorization to test.
          <button onClick={dismiss} className="ml-2 underline hover:no-underline">
            Got it
          </button>
        </div>
      )}
    </>
  );

  if (parsed.kind !== "built") {
    return (
      <div className="flex h-full flex-col p-3 text-sm">
        {Header}
        <div className="mb-2 flex items-center gap-2 font-semibold text-[var(--color-fg)]">
          <ShieldAlert size={16} className="text-rose-500" /> Security builders
        </div>
        <p className="text-[var(--color-muted)]">
          {parsed.kind === "suggestion"
            ? parsed.message
            : parsed.kind === "tool-overview"
              ? "Pick a tool: nmap · sqlmap · ferox · john — then a preset, then the target."
              : parsed.kind === "preset-list"
                ? `Pick an ${parsed.tool.name} preset from the list, then add the target.`
                : "Type a tool + preset (e.g. nmap service 10.0.0.5)."}
        </p>
      </div>
    );
  }

  const { tool, preset, command, missing, sharp, tags, flagHelp } = parsed;

  return (
    <div className="flex h-full flex-col p-3 text-sm">
      {Header}

      <div className="mb-2 flex items-center gap-2 font-semibold text-[var(--color-fg)]">
        <ShieldAlert size={16} className="text-rose-500" />
        {tool.name} · {preset.name}
        {sharp && (
          <span className="flex items-center gap-1 rounded bg-amber-500/20 px-1.5 py-0.5 text-[10px] font-medium text-amber-600 dark:text-amber-400">
            <AlertTriangle size={11} /> confirm before run
          </span>
        )}
      </div>

      {/* The built command line. */}
      <div className="mb-2 rounded bg-[var(--color-surface)] p-2 font-mono text-[13px] break-all text-[var(--color-fg)]">
        {command}
      </div>
      {missing.length > 0 && (
        <p className="mb-2 text-[11px] text-amber-600 dark:text-amber-400">
          Fill in: {missing.map((m) => `‹${m}›`).join(", ")} — Enter still copies the template.
        </p>
      )}
      {wl && wlExists === false && (
        <p className="mb-2 text-[11px] text-amber-600 dark:text-amber-400">
          Wordlist not found on this machine: {wl}
        </p>
      )}
      {tool.name === "john" && catalog.john_formats.length > 0 && (
        <p className="mb-2 text-[11px] text-[var(--color-muted)]">
          <span className="font-medium">--format</span> ({defaults.john_line}):{" "}
          {catalog.john_formats
            .filter((f) => defaults.john_line === "jumbo" || !f.jumbo)
            .slice(0, 12)
            .map((f) => f.name)
            .join(" · ")}
          {defaults.john_line === "jumbo" ? " · …" : ""}
        </p>
      )}

      {/* Honest safety tags. */}
      {tags.length > 0 && (
        <div className="mb-2 flex flex-wrap gap-1">
          {tags.map((t) => (
            <span key={t} className="rounded bg-[var(--color-surface)] px-1.5 py-0.5 text-[10px] text-[var(--color-muted)]">
              {TAG_LABEL[t] ?? t}
            </span>
          ))}
        </div>
      )}

      {/* Flag cheat-sheet — the point: understand, don't blind-copy. */}
      <div className="min-h-0 flex-1 overflow-auto">
        <table className="w-full text-[12px]">
          <tbody>
            {flagHelp.map(([flag, explain]) => (
              <tr key={flag} className="align-top">
                <td className="whitespace-nowrap py-0.5 pr-3 font-mono text-rose-500">{flag}</td>
                <td className="py-0.5 text-[var(--color-muted)]">{explain}</td>
              </tr>
            ))}
          </tbody>
        </table>
        {tool.notes.map((n, i) => (
          <p key={i} className="mt-2 text-[11px] leading-snug text-[var(--color-muted)]">
            {n}
          </p>
        ))}
      </div>

      <div className="mt-2 flex items-center gap-3 border-t border-[var(--color-border)] pt-2 text-[11px] text-[var(--color-muted)]">
        <span>⏎ Copy</span>
        <span className="flex items-center gap-1">
          <Terminal size={12} /> ⌘⏎ Run in terminal
        </span>
      </div>
    </div>
  );
}
