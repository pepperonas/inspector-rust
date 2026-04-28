import { useEffect, useMemo, useState } from "react";
import {
  Bookmark,
  FileCode2,
  FileText,
  Files,
  FolderOpen,
  Image as ImageIcon,
  Plus,
  Send,
  Trash2,
  Type,
} from "lucide-react";
import {
  clearNotes,
  createNote,
  deleteNote,
  pasteNote,
  updateNote,
} from "../lib/ipc";
import type { ContentType, Note } from "../lib/types";
import { relativeTime, truncateOneLine } from "../lib/format";

interface Props {
  notes: Note[];
  categories: string[];
  onRefresh: () => Promise<void> | void;
}

/** Sentinel category strings used in the sidebar (real categories never start with `__`). */
const ALL = "__all__";
const UNCATEGORIZED = "__none__";

interface FormState {
  id: number | null;
  title: string;
  body: string;
  category: string;
  /** Determines whether the body editor is read-only. */
  contentType: ContentType;
  contentData: string;
}

const EMPTY_FORM: FormState = {
  id: null,
  title: "",
  body: "",
  category: "",
  contentType: "text",
  contentData: "",
};

function isBodyEditable(ct: ContentType): boolean {
  return ct === "text" || ct === "html" || ct === "rtf";
}

function ContentTypeIcon({ type }: { type: ContentType }) {
  const cls = "shrink-0";
  const size = 12;
  switch (type) {
    case "text":  return <Type size={size} className={cls} />;
    case "image": return <ImageIcon size={size} className={cls} />;
    case "files": return <Files size={size} className={cls} />;
    case "html":  return <FileCode2 size={size} className={cls} />;
    case "rtf":   return <FileText size={size} className={cls} />;
  }
}

export function NotesPanel({ notes, categories, onRefresh }: Props) {
  const [activeCategory, setActiveCategory] = useState<string>(ALL);
  const [form, setForm] = useState<FormState | null>(null);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const filtered = useMemo(() => {
    if (activeCategory === ALL) return notes;
    if (activeCategory === UNCATEGORIZED) return notes.filter((n) => !n.category);
    return notes.filter((n) => n.category === activeCategory);
  }, [notes, activeCategory]);

  // If the active category disappears (last note in it deleted/moved), bounce back to All.
  useEffect(() => {
    if (
      activeCategory !== ALL &&
      activeCategory !== UNCATEGORIZED &&
      !categories.includes(activeCategory)
    ) {
      setActiveCategory(ALL);
    }
  }, [activeCategory, categories]);

  // Drop the open form if its row vanishes (e.g. after Clear All).
  useEffect(() => {
    if (form?.id != null && !notes.some((n) => n.id === form.id)) {
      setForm(null);
    }
  }, [form, notes]);

  const counts = useMemo(() => {
    const m = new Map<string, number>();
    let uncategorized = 0;
    for (const n of notes) {
      if (!n.category) uncategorized++;
      else m.set(n.category, (m.get(n.category) ?? 0) + 1);
    }
    return { all: notes.length, uncategorized, byCategory: m };
  }, [notes]);

  const openNew = () => {
    const cat =
      activeCategory === ALL || activeCategory === UNCATEGORIZED
        ? ""
        : activeCategory;
    setForm({ ...EMPTY_FORM, category: cat });
    setError(null);
  };

  const openEdit = (n: Note) => {
    setForm({
      id: n.id,
      title: n.title,
      body: n.content_text,
      category: n.category,
      contentType: n.content_type,
      contentData: n.content_data,
    });
    setError(null);
  };

  const cancel = () => {
    setForm(null);
    setError(null);
  };

  const save = async () => {
    if (!form) return;
    if (form.id === null) {
      // From-scratch text note
      if (!form.body.trim()) {
        setError("Body text is required.");
        return;
      }
      setSaving(true);
      setError(null);
      try {
        await createNote(form.title, form.body, form.category);
        await onRefresh();
        setForm(null);
      } catch (e) {
        setError(String(e));
      } finally {
        setSaving(false);
      }
      return;
    }
    // Update existing note
    setSaving(true);
    setError(null);
    try {
      await updateNote(form.id, form.title, form.body, form.category);
      await onRefresh();
    } catch (e) {
      setError(String(e));
    } finally {
      setSaving(false);
    }
  };

  const remove = async (id: number) => {
    await deleteNote(id);
    if (form?.id === id) setForm(null);
    await onRefresh();
  };

  const paste = async (id: number) => {
    try {
      await pasteNote(id);
    } catch (e) {
      setError(String(e));
    }
  };

  const onClearAll = async () => {
    if (notes.length === 0) return;
    const ok = window.confirm(
      `Delete all ${notes.length} note${notes.length === 1 ? "" : "s"}? This cannot be undone.`,
    );
    if (!ok) return;
    await clearNotes();
    setForm(null);
    await onRefresh();
  };

  // Backup Export/Import lives in the Settings tab now (with checkboxes
  // for selective export). NotesPanel no longer owns those buttons.

  return (
    <div className="flex min-h-0 flex-1">
      {/* Sidebar — categories + actions */}
      <div className="flex w-1/4 min-w-[180px] flex-col border-r border-[var(--color-border)]">
        <div className="border-b border-[var(--color-border)] px-3 py-2 text-[11px] font-semibold uppercase tracking-wide text-[var(--color-muted)]">
          Categories
        </div>
        <div className="flex-1 overflow-auto">
          <CategoryRow
            label="All"
            count={counts.all}
            active={activeCategory === ALL}
            onClick={() => setActiveCategory(ALL)}
          />
          <CategoryRow
            label="Uncategorized"
            count={counts.uncategorized}
            active={activeCategory === UNCATEGORIZED}
            onClick={() => setActiveCategory(UNCATEGORIZED)}
            muted
          />
          {categories.map((c) => (
            <CategoryRow
              key={c}
              label={c}
              count={counts.byCategory.get(c) ?? 0}
              active={activeCategory === c}
              onClick={() => setActiveCategory(c)}
            />
          ))}
        </div>
        <div className="border-t border-[var(--color-border)]">
          <SidebarAction icon={<Plus size={13} />} label="New Note" onClick={openNew} />
          <SidebarAction
            icon={<Trash2 size={13} />}
            label="Clear All"
            onClick={() => void onClearAll()}
            danger
            disabled={notes.length === 0}
          />
        </div>
      </div>

      {/* Note list */}
      <div className="flex w-2/5 flex-col border-r border-[var(--color-border)]">
        <div className="flex-1 overflow-auto">
          {filtered.length === 0 ? (
            <div className="flex h-full items-center justify-center px-4 text-center text-[12px] text-[var(--color-muted)]">
              {notes.length === 0
                ? "No notes yet. Star a clipboard entry or click + New Note."
                : "No notes in this category."}
            </div>
          ) : (
            filtered.map((n) => {
              const isActive = form?.id === n.id;
              const subtitle = n.title || truncateOneLine(n.content_text, 60) || "(empty)";
              return (
                <div
                  key={n.id}
                  onClick={() => openEdit(n)}
                  onDoubleClick={() => void paste(n.id)}
                  className={
                    "group flex cursor-pointer items-start gap-2 px-3 py-2 text-[12px] " +
                    (isActive
                      ? "bg-[var(--color-accent)] text-[var(--color-accent-fg)]"
                      : "hover:bg-[var(--color-surface)]")
                  }
                >
                  <span
                    className={
                      "mt-0.5 " +
                      (isActive ? "text-white/80" : "text-[var(--color-accent)]")
                    }
                  >
                    <ContentTypeIcon type={n.content_type} />
                  </span>
                  <div className="min-w-0 flex-1">
                    <div className="truncate font-medium">{subtitle}</div>
                    <div
                      className={
                        "flex items-center gap-1.5 text-[11px] " +
                        (isActive ? "text-white/70" : "text-[var(--color-muted)]")
                      }
                    >
                      {n.category && <span className="truncate">{n.category}</span>}
                      {n.category && <span>·</span>}
                      <span>{relativeTime(n.updated_at)}</span>
                    </div>
                  </div>
                  <button
                    onClick={(e) => {
                      e.stopPropagation();
                      void remove(n.id);
                    }}
                    className={
                      "shrink-0 rounded p-0.5 opacity-0 group-hover:opacity-100 " +
                      (isActive
                        ? "text-white/80 hover:bg-white/20"
                        : "text-[var(--color-muted)] hover:bg-[var(--color-border)] hover:text-red-400")
                    }
                    title="Delete note"
                  >
                    <Trash2 size={12} />
                  </button>
                </div>
              );
            })
          )}
        </div>
      </div>

      {/* Detail / edit pane */}
      <div className="flex flex-1 min-w-0 flex-col">
        {form === null ? (
          <div className="flex h-full flex-col items-center justify-center gap-2 text-[12px] text-[var(--color-muted)]">
            <Bookmark size={24} className="opacity-30" />
            <span>Select a note or click&nbsp;<b>+ New Note</b></span>
          </div>
        ) : (
          <NoteEditor
            form={form}
            setForm={setForm}
            categories={categories}
            error={error}
            saving={saving}
            onCancel={cancel}
            onSave={() => void save()}
            onPaste={form.id !== null ? () => void paste(form.id!) : undefined}
            onDelete={form.id !== null ? () => void remove(form.id!) : undefined}
          />
        )}
      </div>
    </div>
  );
}

function CategoryRow({
  label,
  count,
  active,
  onClick,
  muted,
}: {
  label: string;
  count: number;
  active: boolean;
  onClick: () => void;
  muted?: boolean;
}) {
  return (
    <button
      onClick={onClick}
      className={
        "flex w-full items-center gap-2 px-3 py-1.5 text-left text-[12px] " +
        (active
          ? "bg-[var(--color-accent)] text-[var(--color-accent-fg)]"
          : "hover:bg-[var(--color-surface)] " +
            (muted ? "text-[var(--color-muted)]" : ""))
      }
    >
      <FolderOpen
        size={12}
        className={active ? "text-white/80" : "text-[var(--color-accent)]"}
      />
      <span className="flex-1 truncate">{label}</span>
      <span
        className={
          "rounded px-1 text-[10px] tabular-nums " +
          (active ? "bg-white/20 text-white/80" : "text-[var(--color-muted)]")
        }
      >
        {count}
      </span>
    </button>
  );
}

function SidebarAction({
  icon,
  label,
  onClick,
  disabled,
  danger,
}: {
  icon: React.ReactNode;
  label: string;
  onClick: () => void;
  disabled?: boolean;
  danger?: boolean;
}) {
  return (
    <button
      onClick={onClick}
      disabled={disabled}
      className={
        "flex w-full items-center gap-2 px-3 py-1.5 text-left text-[12px] hover:bg-[var(--color-surface)] disabled:opacity-50 " +
        (danger
          ? "text-[var(--color-muted)] hover:text-red-400"
          : "text-[var(--color-accent)]")
      }
    >
      {icon}
      <span>{label}</span>
    </button>
  );
}

interface EditorProps {
  form: FormState;
  setForm: (f: FormState | null) => void;
  categories: string[];
  error: string | null;
  saving: boolean;
  onCancel: () => void;
  onSave: () => void;
  onPaste?: () => void;
  onDelete?: () => void;
}

function NoteEditor({
  form,
  setForm,
  categories,
  error,
  saving,
  onCancel,
  onSave,
  onPaste,
  onDelete,
}: EditorProps) {
  const editable = isBodyEditable(form.contentType);
  const isImage = form.contentType === "image";
  const filesList = useMemo<string[] | null>(() => {
    if (form.contentType !== "files") return null;
    try {
      return JSON.parse(form.contentData) as string[];
    } catch {
      return null;
    }
  }, [form.contentType, form.contentData]);

  return (
    <div className="flex h-full min-h-0 flex-col gap-3 p-4">
      <div className="flex items-center gap-2 text-[11px] uppercase tracking-wide text-[var(--color-muted)]">
        <ContentTypeIcon type={form.contentType} />
        <span>{form.contentType}</span>
        <span className="ml-auto">{form.id === null ? "New Note" : `Note #${form.id}`}</span>
      </div>

      <label className="flex flex-col gap-1">
        <span className="text-[11px] text-[var(--color-muted)]">Title (optional)</span>
        <input
          autoFocus={form.id === null}
          value={form.title}
          onChange={(e) => setForm({ ...form, title: e.target.value })}
          placeholder="e.g. Server credentials, API key"
          className="rounded border border-[var(--color-border)] bg-[var(--color-surface)] px-2 py-1 text-[13px] outline-none focus:border-[var(--color-accent)]"
          onKeyDown={(e) => {
            if (e.key === "Escape") onCancel();
          }}
        />
      </label>

      <label className="flex flex-col gap-1">
        <span className="text-[11px] text-[var(--color-muted)]">
          Category (free-form, autocomplete from existing)
        </span>
        <input
          list="note-categories"
          value={form.category}
          onChange={(e) => setForm({ ...form, category: e.target.value })}
          placeholder="e.g. Work, Snippets, …"
          className="rounded border border-[var(--color-border)] bg-[var(--color-surface)] px-2 py-1 text-[13px] outline-none focus:border-[var(--color-accent)]"
          onKeyDown={(e) => {
            if (e.key === "Escape") onCancel();
          }}
        />
        <datalist id="note-categories">
          {categories.map((c) => (
            <option key={c} value={c} />
          ))}
        </datalist>
      </label>

      <label className="flex min-h-0 flex-1 flex-col gap-1">
        <span className="text-[11px] text-[var(--color-muted)]">
          Body {editable ? (form.contentType !== "text" ? `(raw ${form.contentType})` : "") : "(read-only)"}
        </span>
        {isImage ? (
          <div className="flex flex-1 items-center justify-center overflow-hidden rounded border border-[var(--color-border)] bg-[var(--color-surface)]">
            <img
              src={`data:image/png;base64,${form.contentData}`}
              alt="note image"
              className="max-h-full max-w-full object-contain"
            />
          </div>
        ) : filesList ? (
          <div className="flex-1 overflow-auto rounded border border-[var(--color-border)] bg-[var(--color-surface)] p-2 font-[var(--font-mono)] text-[12px]">
            {filesList.map((p, i) => (
              <div key={i} className="truncate py-0.5">
                {p}
              </div>
            ))}
          </div>
        ) : (
          <textarea
            value={form.body}
            readOnly={!editable}
            onChange={(e) => setForm({ ...form, body: e.target.value })}
            placeholder={editable ? "Note text…" : ""}
            className={
              "flex-1 resize-none rounded border border-[var(--color-border)] bg-[var(--color-surface)] px-2 py-1.5 font-[var(--font-mono)] text-[12px] leading-5 outline-none focus:border-[var(--color-accent)] " +
              (!editable ? "cursor-default" : "")
            }
            onKeyDown={(e) => {
              if (e.key === "Escape") onCancel();
            }}
          />
        )}
      </label>

      {error && <div className="text-[11px] text-red-400">{error}</div>}

      <div className="flex items-center justify-end gap-2">
        {onDelete && (
          <button
            onClick={onDelete}
            title="Delete note"
            className="mr-auto rounded p-1 text-[var(--color-muted)] hover:bg-[var(--color-surface)] hover:text-red-400"
          >
            <Trash2 size={14} />
          </button>
        )}
        {onPaste && (
          <button
            onClick={onPaste}
            className="flex items-center gap-1 rounded border border-[var(--color-border)] px-3 py-1 text-[12px] text-[var(--color-muted)] hover:bg-[var(--color-surface)] hover:text-[var(--color-fg)]"
            title="Paste this note into the previously focused app"
          >
            <Send size={12} />
            Paste
          </button>
        )}
        <button
          onClick={onCancel}
          className="rounded px-3 py-1 text-[12px] text-[var(--color-muted)] hover:bg-[var(--color-surface)]"
        >
          {form.id === null ? "Cancel" : "Close"}
        </button>
        <button
          onClick={onSave}
          disabled={saving}
          className="rounded bg-[var(--color-accent)] px-3 py-1 text-[12px] text-[var(--color-accent-fg)] hover:opacity-90 disabled:opacity-50"
        >
          {saving ? "Saving…" : form.id === null ? "Create" : "Save"}
        </button>
      </div>
    </div>
  );
}
