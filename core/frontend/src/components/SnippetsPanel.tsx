import { useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { Plus, RotateCcw, Trash2, Upload, Zap } from "lucide-react";
import {
  deleteSnippet,
  importSnippetsFromFile,
  restoreDefaultPrompts,
  setSuppressHide,
  upsertSnippet,
  type ImportResult,
} from "../lib/ipc";
import type { Snippet } from "../lib/types";

interface Props {
  snippets: Snippet[];
  onRefresh: () => void;
}

interface FormState {
  id: number | null;
  abbreviation: string;
  title: string;
  body: string;
}

const EMPTY_FORM: FormState = { id: null, abbreviation: "", title: "", body: "" };

export function SnippetsPanel({ snippets, onRefresh }: Props) {
  const [form, setForm] = useState<FormState | null>(null);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [importStatus, setImportStatus] = useState<
    | { kind: "ok"; result: ImportResult }
    | { kind: "err"; message: string }
    | null
  >(null);
  const [importing, setImporting] = useState(false);

  const openNew = () => {
    setForm(EMPTY_FORM);
    setError(null);
  };

  const openEdit = (s: Snippet) => {
    setForm({ id: s.id, abbreviation: s.abbreviation, title: s.title, body: s.body });
    setError(null);
  };

  const cancel = () => {
    setForm(null);
    setError(null);
  };

  const save = async () => {
    if (!form) return;
    if (!form.abbreviation.trim()) {
      setError("Abbreviation is required.");
      return;
    }
    if (!form.body.trim()) {
      setError("Body text is required.");
      return;
    }
    setSaving(true);
    setError(null);
    try {
      await upsertSnippet(form.id, form.abbreviation, form.title, form.body);
      await onRefresh();
      setForm(null);
    } catch (e) {
      setError(String(e));
    } finally {
      setSaving(false);
    }
  };

  const remove = async (id: number) => {
    await deleteSnippet(id);
    if (form?.id === id) setForm(null);
    await onRefresh();
  };

  const onRestoreDefaults = async () => {
    if (
      !window.confirm(
        "Re-import the bundled default AI-prompt templates (~25 prompts).\n\nExisting snippets with the same abbreviation will be overwritten with the latest version. Your other snippets stay untouched.\n\nContinue?",
      )
    ) {
      return;
    }
    setImportStatus(null);
    setImporting(true);
    try {
      const result = await restoreDefaultPrompts();
      setImportStatus({ kind: "ok", result });
      await onRefresh();
    } catch (err) {
      setImportStatus({ kind: "err", message: String(err) });
    } finally {
      setImporting(false);
    }
  };

  const onPickFile = async () => {
    setImportStatus(null);
    setImporting(true);
    // Suppress the popup's hide-on-blur while the modal file dialog owns
    // focus — otherwise the popup vanishes (and may even tear down the
    // dialog with it) the instant NSOpenPanel opens.
    await setSuppressHide(true).catch(() => {});
    try {
      const selected = await open({
        multiple: false,
        directory: false,
        filters: [{ name: "JSON", extensions: ["json"] }],
        title: "Select snippets JSON file",
      });
      if (!selected) return; // user cancelled
      const result = await importSnippetsFromFile(selected);
      setImportStatus({ kind: "ok", result });
      await onRefresh();
    } catch (err) {
      setImportStatus({ kind: "err", message: String(err) });
    } finally {
      await setSuppressHide(false).catch(() => {});
      setImporting(false);
    }
  };

  return (
    <div className="flex min-h-0 flex-1">
      {/* Left: snippet list */}
      <div className="flex w-2/5 flex-col border-r border-[var(--color-border)]">
        <div className="flex border-b border-[var(--color-border)]">
          <button
            onClick={openNew}
            className="flex flex-1 items-center gap-1.5 px-3 py-2 text-left text-[12px] text-[var(--color-accent)] hover:bg-[var(--color-surface)]"
          >
            <Plus size={13} />
            New Snippet
          </button>
          <button
            onClick={() => void onPickFile()}
            disabled={importing}
            title="Import snippets from JSON file"
            className="flex items-center gap-1.5 border-l border-[var(--color-border)] px-3 py-2 text-[12px] text-[var(--color-muted)] hover:bg-[var(--color-surface)] hover:text-[var(--color-accent)] disabled:opacity-50"
          >
            <Upload size={13} />
            {importing ? "Importing…" : "Import"}
          </button>
          <button
            onClick={() => void onRestoreDefaults()}
            disabled={importing}
            title="Re-import the bundled default AI-prompt templates. Existing snippets sharing an abbreviation will be overwritten; your other snippets are untouched."
            className="flex items-center gap-1.5 border-l border-[var(--color-border)] px-3 py-2 text-[12px] text-[var(--color-muted)] hover:bg-[var(--color-surface)] hover:text-[var(--color-accent)] disabled:opacity-50"
          >
            <RotateCcw size={13} />
            Restore defaults
          </button>
        </div>

        {importStatus && (
          <div
            className={
              "border-b border-[var(--color-border)] px-3 py-1.5 text-[11px] " +
              (importStatus.kind === "ok"
                ? "text-[var(--color-muted)]"
                : "text-red-400")
            }
          >
            {importStatus.kind === "ok" ? (
              <>
                Imported <b>{importStatus.result.imported}</b>
                {importStatus.result.skipped > 0 && (
                  <>
                    , skipped <b>{importStatus.result.skipped}</b>
                  </>
                )}
                {importStatus.result.errors.length > 0 && (
                  <>
                    {" — "}
                    <span className="text-red-400">
                      {importStatus.result.errors[0]}
                      {importStatus.result.errors.length > 1 &&
                        ` (+${importStatus.result.errors.length - 1} more)`}
                    </span>
                  </>
                )}
              </>
            ) : (
              <>Import failed: {importStatus.message}</>
            )}
          </div>
        )}

        <div className="flex-1 overflow-auto">
          {snippets.length === 0 && (
            <div className="flex h-full items-center justify-center text-[12px] text-[var(--color-muted)]">
              No snippets yet
            </div>
          )}
          {snippets.map((s) => {
            const isActive = form?.id === s.id;
            return (
              <div
                key={s.id}
                onClick={() => openEdit(s)}
                className={
                  "group flex cursor-pointer items-start gap-2 px-3 py-2 text-[12px] " +
                  (isActive
                    ? "bg-[var(--color-accent)] text-[var(--color-accent-fg)]"
                    : "hover:bg-[var(--color-surface)]")
                }
              >
                <Zap
                  size={12}
                  className={
                    "mt-0.5 shrink-0 " +
                    (isActive ? "text-white/80" : "text-[var(--color-accent)]")
                  }
                />
                <div className="min-w-0 flex-1">
                  <div className="truncate font-[var(--font-mono)] font-medium">
                    {s.abbreviation}
                  </div>
                  <div
                    className={
                      "truncate text-[11px] " +
                      (isActive ? "text-white/70" : "text-[var(--color-muted)]")
                    }
                  >
                    {s.title || s.body.split("\n")[0]}
                  </div>
                </div>
                <button
                  onClick={(e) => {
                    e.stopPropagation();
                    void remove(s.id);
                  }}
                  className={
                    "shrink-0 rounded p-0.5 opacity-0 group-hover:opacity-100 " +
                    (isActive
                      ? "text-white/80 hover:bg-white/20"
                      : "text-[var(--color-muted)] hover:bg-[var(--color-border)] hover:text-red-400")
                  }
                  title="Delete snippet"
                >
                  <Trash2 size={12} />
                </button>
              </div>
            );
          })}
        </div>
      </div>

      {/* Right: edit form */}
      <div className="flex w-3/5 flex-col p-4">
        {form === null ? (
          <div className="flex h-full flex-col items-center justify-center gap-2 text-[12px] text-[var(--color-muted)]">
            <Zap size={24} className="opacity-30" />
            <span>Select a snippet to edit or click&nbsp;<b>+ New Snippet</b></span>
          </div>
        ) : (
          <div className="flex h-full flex-col gap-3">
            <div className="text-[11px] font-semibold uppercase tracking-wide text-[var(--color-muted)]">
              {form.id === null ? "New Snippet" : "Edit Snippet"}
            </div>

            <label className="flex flex-col gap-1">
              <span className="text-[11px] text-[var(--color-muted)]">Abbreviation *</span>
              <input
                autoFocus
                value={form.abbreviation}
                onChange={(e) => setForm({ ...form, abbreviation: e.target.value })}
                placeholder="e.g. mfg"
                className="rounded border border-[var(--color-border)] bg-[var(--color-surface)] px-2 py-1 font-[var(--font-mono)] text-[13px] outline-none focus:border-[var(--color-accent)]"
                onKeyDown={(e) => {
                  if (e.key === "Enter") void save();
                  if (e.key === "Escape") cancel();
                }}
              />
            </label>

            <label className="flex flex-col gap-1">
              <span className="text-[11px] text-[var(--color-muted)]">Title (optional)</span>
              <input
                value={form.title}
                onChange={(e) => setForm({ ...form, title: e.target.value })}
                placeholder="e.g. Signing off"
                className="rounded border border-[var(--color-border)] bg-[var(--color-surface)] px-2 py-1 text-[13px] outline-none focus:border-[var(--color-accent)]"
                onKeyDown={(e) => {
                  if (e.key === "Escape") cancel();
                }}
              />
            </label>

            <label className="flex min-h-0 flex-1 flex-col gap-1">
              <span className="text-[11px] text-[var(--color-muted)]">Body *</span>
              <textarea
                value={form.body}
                onChange={(e) => setForm({ ...form, body: e.target.value })}
                placeholder="Template text that gets pasted…"
                className="flex-1 resize-none rounded border border-[var(--color-border)] bg-[var(--color-surface)] px-2 py-1.5 font-[var(--font-mono)] text-[12px] leading-5 outline-none focus:border-[var(--color-accent)]"
                onKeyDown={(e) => {
                  if (e.key === "Escape") cancel();
                }}
              />
            </label>

            {error && (
              <div className="text-[11px] text-red-400">{error}</div>
            )}

            <div className="flex justify-end gap-2">
              <button
                onClick={cancel}
                className="rounded px-3 py-1 text-[12px] text-[var(--color-muted)] hover:bg-[var(--color-surface)]"
              >
                Cancel
              </button>
              <button
                onClick={() => void save()}
                disabled={saving}
                className="rounded bg-[var(--color-accent)] px-3 py-1 text-[12px] text-[var(--color-accent-fg)] hover:opacity-90 disabled:opacity-50"
              >
                {saving ? "Saving…" : "Save"}
              </button>
            </div>
          </div>
        )}
      </div>
    </div>
  );
}
