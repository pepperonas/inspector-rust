/**
 * The snippet edit form — abbreviation, title, group, body.
 *
 * ONE component, two callers (v0.84.265): the Snippets tab's right column, and
 * the history-tab preview (✏️ / Cmd+E on a selected snippet row), so editing a
 * prompt you just searched for doesn't mean leaving the search. Extracted from
 * SnippetsPanel rather than copied — a second form would drift.
 *
 * Owns its own draft state + save (`upsertSnippet`, plus `createSnippetCategory`
 * for the inline "＋ New group…"); the caller only says where it started from
 * (`initial`) and what to do afterwards (`onSaved` / `onCancel`).
 *
 * Keys: **Cmd/Ctrl+Enter saves from anywhere** in the form (plain Enter in the
 * body inserts a newline — it's a template, multi-line is the point), Enter in
 * the single-line fields saves, Esc cancels. The caller must disable the global
 * list navigation while this is open (see App.tsx `snippetEditing`), otherwise
 * Enter would paste the snippet instead of saving it.
 */
import { useState } from "react";
import { X } from "lucide-react";
import { createSnippetCategory, upsertSnippet, type SnippetCategory } from "../lib/ipc";

export interface SnippetDraft {
  /** null = a new snippet. */
  id: number | null;
  abbreviation: string;
  title: string;
  body: string;
  categoryId: number | null;
  /** Current content revision (cue-compatible); shown in the header. Undefined
   *  for a new snippet. */
  version?: number;
}

export const EMPTY_DRAFT: SnippetDraft = {
  id: null,
  abbreviation: "",
  title: "",
  body: "",
  categoryId: null,
};

export function SnippetEditor({
  initial,
  categories,
  onSaved,
  onCancel,
  autoFocus = "abbreviation",
}: {
  initial: SnippetDraft;
  categories: SnippetCategory[];
  /** Saved successfully — the caller refreshes its list and closes the editor. */
  onSaved: () => void | Promise<void>;
  onCancel: () => void;
  /** Which field takes focus on open. The preview opens on an existing snippet,
   *  where the body is what you came to change. */
  autoFocus?: "abbreviation" | "body";
}) {
  const [form, setForm] = useState<SnippetDraft>(initial);
  const [newGroupName, setNewGroupName] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const save = async () => {
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
      let categoryId = form.categoryId;
      // Pending "new group" typed but not yet created → create it now.
      if (newGroupName !== null && newGroupName.trim()) {
        categoryId = await createSnippetCategory(newGroupName.trim());
      }
      await upsertSnippet(form.id, form.abbreviation, form.title, form.body, categoryId);
      await onSaved();
    } catch (e) {
      setError(String(e));
    } finally {
      setSaving(false);
    }
  };

  /** Cmd/Ctrl+Enter saves from any field; Esc always cancels. */
  const onFormKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === "Enter" && (e.metaKey || e.ctrlKey)) {
      e.preventDefault();
      e.stopPropagation();
      void save();
    } else if (e.key === "Escape") {
      e.preventDefault();
      e.stopPropagation();
      onCancel();
    }
  };

  return (
    <div className="flex h-full flex-col gap-3" onKeyDown={onFormKeyDown}>
      <div className="flex items-baseline gap-2 text-[11px] font-semibold uppercase tracking-wide text-[var(--color-muted)]">
        <span>{form.id === null ? "New Snippet" : "Edit Snippet"}</span>
        {form.id !== null && (
          <span
            className="font-[var(--font-mono)] normal-case tracking-normal text-[var(--color-muted)]/70"
            title={`Content revision ${form.version ?? 1} — bumps only when abbreviation, title or body change`}
          >
            v{form.version ?? 1}
          </span>
        )}
      </div>

      <label className="flex flex-col gap-1">
        <span className="text-[11px] text-[var(--color-muted)]">Abbreviation *</span>
        <input
          autoFocus={autoFocus === "abbreviation"}
          value={form.abbreviation}
          onChange={(e) => setForm({ ...form, abbreviation: e.target.value })}
          placeholder="e.g. mfg"
          className="rounded border border-[var(--color-border)] bg-[var(--color-surface)] px-2 py-1 font-[var(--font-mono)] text-[13px] outline-none focus:border-[var(--color-accent)]"
          onKeyDown={(e) => {
            if (e.key === "Enter" && !e.metaKey && !e.ctrlKey) void save();
          }}
        />
      </label>

      <div className="flex gap-2">
        <label className="flex flex-1 flex-col gap-1">
          <span className="text-[11px] text-[var(--color-muted)]">Title (optional)</span>
          <input
            value={form.title}
            onChange={(e) => setForm({ ...form, title: e.target.value })}
            placeholder="e.g. Signing off"
            className="rounded border border-[var(--color-border)] bg-[var(--color-surface)] px-2 py-1 text-[13px] outline-none focus:border-[var(--color-accent)]"
            onKeyDown={(e) => {
              if (e.key === "Enter" && !e.metaKey && !e.ctrlKey) void save();
            }}
          />
        </label>

        <label className="flex flex-col gap-1">
          <span className="text-[11px] text-[var(--color-muted)]">Group</span>
          {newGroupName !== null ? (
            <div className="flex items-center gap-1">
              <input
                autoFocus
                value={newGroupName}
                onChange={(e) => setNewGroupName(e.target.value)}
                placeholder="New group name"
                className="w-32 rounded border border-[var(--color-border)] bg-[var(--color-surface)] px-2 py-1 text-[13px] outline-none focus:border-[var(--color-accent)]"
                onKeyDown={(e) => {
                  // Esc here only cancels the inline group input, not the form.
                  if (e.key === "Escape") {
                    e.stopPropagation();
                    setNewGroupName(null);
                  }
                }}
              />
              <button
                onClick={() => setNewGroupName(null)}
                title="Cancel new group"
                className="rounded p-1 text-[var(--color-muted)] hover:bg-[var(--color-surface)]"
              >
                <X size={13} />
              </button>
            </div>
          ) : (
            <select
              value={form.categoryId ?? ""}
              onChange={(e) => {
                const v = e.target.value;
                if (v === "__new__") {
                  setNewGroupName("");
                } else {
                  setForm({ ...form, categoryId: v === "" ? null : Number(v) });
                }
              }}
              className="w-40 rounded border border-[var(--color-border)] bg-[var(--color-surface)] px-2 py-1 text-[13px] outline-none focus:border-[var(--color-accent)]"
            >
              <option value="">No group</option>
              {categories.map((c) => (
                <option key={c.id} value={c.id}>
                  {c.name}
                </option>
              ))}
              <option value="__new__">＋ New group…</option>
            </select>
          )}
        </label>
      </div>

      <label className="flex min-h-0 flex-1 flex-col gap-1">
        <span className="text-[11px] text-[var(--color-muted)]">Body *</span>
        <textarea
          autoFocus={autoFocus === "body"}
          value={form.body}
          onChange={(e) => setForm({ ...form, body: e.target.value })}
          placeholder="Template text that gets pasted…"
          className="flex-1 resize-none rounded border border-[var(--color-border)] bg-[var(--color-surface)] px-2 py-1.5 font-[var(--font-mono)] text-[12px] leading-5 outline-none focus:border-[var(--color-accent)]"
        />
        <span className="text-[10px] leading-snug text-[var(--color-muted)]">
          Placeholders (expanded on paste):{" "}
          <code className="text-[var(--color-fg)]">{"{date}"}</code>{" "}
          <code className="text-[var(--color-fg)]">{"{time}"}</code>{" "}
          <code className="text-[var(--color-fg)]">{"{datetime}"}</code>{" "}
          <code className="text-[var(--color-fg)]">{"{clipboard}"}</code>{" "}
          <code className="text-[var(--color-fg)]">{"{cursor}"}</code>
          {" — "}custom format e.g.{" "}
          <code className="text-[var(--color-fg)]">{"{date:%d.%m.%Y}"}</code>;{" "}
          <code className="text-[var(--color-fg)]">{"{{"}</code>/
          <code className="text-[var(--color-fg)]">{"}}"}</code> for literal braces.
        </span>
      </label>

      {error && <div className="text-[11px] text-red-400">{error}</div>}

      <div className="flex items-center justify-end gap-2">
        <span className="mr-auto text-[10px] text-[var(--color-muted)]">
          ⌘/Ctrl+↵ save · Esc cancel
        </span>
        <button
          onClick={onCancel}
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
  );
}
