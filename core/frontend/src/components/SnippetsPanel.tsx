import { useMemo, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { confirmDialog } from "../lib/confirm";
import {
  ArrowDown,
  ArrowUp,
  Check,
  FolderCog,
  Plus,
  RotateCcw,
  Trash2,
  Upload,
  X,
  Zap,
} from "lucide-react";
import {
  createSnippetCategory,
  deleteSnippet,
  deleteSnippetCategory,
  importSnippetsFromFile,
  renameSnippetCategory,
  reorderSnippetCategories,
  restoreDefaultPrompts,
  setSuppressHide,
  upsertSnippet,
  type ImportResult,
  type SnippetCategory,
} from "../lib/ipc";
import type { Snippet } from "../lib/types";

interface Props {
  snippets: Snippet[];
  categories: SnippetCategory[];
  onRefresh: () => void;
}

interface FormState {
  id: number | null;
  abbreviation: string;
  title: string;
  body: string;
  categoryId: number | null;
}

/** null = "All", -1 = "Ungrouped", positive = a specific group id. */
type Filter = null | -1 | number;

const EMPTY_FORM: FormState = { id: null, abbreviation: "", title: "", body: "", categoryId: null };

export function SnippetsPanel({ snippets, categories, onRefresh }: Props) {
  const [form, setForm] = useState<FormState | null>(null);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [filter, setFilter] = useState<Filter>(null);
  const [managing, setManaging] = useState(false);
  // Inline "create group" input shown inside the edit form's group picker.
  const [newGroupName, setNewGroupName] = useState<string | null>(null);
  const [importStatus, setImportStatus] = useState<
    | { kind: "ok"; result: ImportResult }
    | { kind: "err"; message: string }
    | null
  >(null);
  const [importing, setImporting] = useState(false);
  const [confirmingRestore, setConfirmingRestore] = useState(false);

  // Map category id → name for showing a chip on each snippet row.
  const catName = useMemo(() => {
    const m = new Map<number, string>();
    for (const c of categories) m.set(c.id, c.name);
    return m;
  }, [categories]);

  const ungroupedCount = useMemo(
    () => snippets.filter((s) => s.category == null).length,
    [snippets],
  );

  const visible = useMemo(() => {
    if (filter === null) return snippets;
    if (filter === -1) return snippets.filter((s) => s.category == null);
    const name = catName.get(filter);
    return snippets.filter((s) => s.category === name);
  }, [snippets, filter, catName]);

  const openNew = () => {
    // Pre-fill the group from the current filter for a fast "add to this group".
    const preset = typeof filter === "number" && filter > 0 ? filter : null;
    setForm({ ...EMPTY_FORM, categoryId: preset });
    setNewGroupName(null);
    setError(null);
  };

  const openEdit = (s: Snippet) => {
    const cid = s.category
      ? categories.find((c) => c.name === s.category)?.id ?? null
      : null;
    setForm({ id: s.id, abbreviation: s.abbreviation, title: s.title, body: s.body, categoryId: cid });
    setNewGroupName(null);
    setError(null);
  };

  const cancel = () => {
    setForm(null);
    setNewGroupName(null);
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
      let categoryId = form.categoryId;
      // Pending "new group" typed but not yet created → create it now.
      if (newGroupName !== null && newGroupName.trim()) {
        categoryId = await createSnippetCategory(newGroupName.trim());
      }
      await upsertSnippet(form.id, form.abbreviation, form.title, form.body, categoryId);
      await onRefresh();
      setForm(null);
      setNewGroupName(null);
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
      !(await confirmDialog(
        "Re-import the bundled default AI-prompt templates (~25 prompts).\n\nExisting snippets with the same abbreviation will be overwritten with the latest version. Your other snippets stay untouched.\n\nContinue?",
        "Restore defaults?",
      ))
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
    await setSuppressHide(true).catch(() => {});
    try {
      const selected = await open({
        multiple: false,
        directory: false,
        filters: [{ name: "JSON", extensions: ["json"] }],
        title: "Select snippets JSON file",
      });
      if (!selected) return;
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
      {/* Left: group filter + snippet list */}
      <div className="flex w-2/5 flex-col border-r border-[var(--color-border)]">
        <div className="flex h-10 items-center justify-between border-b border-[var(--color-border)] px-2">
          {confirmingRestore ? (
            <div className="flex w-full items-center gap-1 text-[11px]">
              <span className="text-red-400">Restore defaults?</span>
              <button
                onClick={() => {
                  setConfirmingRestore(false);
                  void onRestoreDefaults();
                }}
                className="ml-auto rounded px-2 py-0.5 text-red-400 hover:bg-red-400/10"
              >
                Yes
              </button>
              <button
                onClick={() => setConfirmingRestore(false)}
                className="rounded px-2 py-0.5 text-[var(--color-muted)] hover:bg-[var(--color-surface)]"
              >
                Cancel
              </button>
            </div>
          ) : (
            <>
              <button
                onClick={openNew}
                title="New snippet"
                className="flex h-7 w-7 items-center justify-center rounded text-[var(--color-accent)] hover:bg-[var(--color-surface)]"
                aria-label="New snippet"
              >
                <Plus size={14} />
              </button>
              <button
                onClick={() => {
                  setManaging((m) => !m);
                  setForm(null);
                }}
                title="Manage groups — create, rename, reorder or delete"
                className={
                  "flex h-7 w-7 items-center justify-center rounded hover:bg-[var(--color-surface)] " +
                  (managing ? "text-[var(--color-accent)]" : "text-[var(--color-muted)] hover:text-[var(--color-accent)]")
                }
                aria-label="Manage groups"
              >
                <FolderCog size={14} />
              </button>
              <button
                onClick={() => void onPickFile()}
                disabled={importing}
                title={importing ? "Importing…" : "Import snippets from JSON file"}
                className="flex h-7 w-7 items-center justify-center rounded text-[var(--color-muted)] hover:bg-[var(--color-surface)] hover:text-[var(--color-accent)] disabled:opacity-50"
                aria-label="Import snippets"
              >
                <Upload size={14} />
              </button>
              <button
                onClick={() => setConfirmingRestore(true)}
                disabled={importing}
                title="Restore default snippets — re-imports the bundled AI-prompt templates. Existing snippets sharing an abbreviation will be overwritten; your other snippets are untouched."
                className="flex h-7 w-7 items-center justify-center rounded text-[var(--color-muted)] hover:bg-[var(--color-surface)] hover:text-[var(--color-accent)] disabled:opacity-50"
                aria-label="Restore default snippets"
              >
                <RotateCcw size={14} />
              </button>
            </>
          )}
        </div>

        {/* Group filter chips */}
        <div className="flex items-center gap-1 overflow-x-auto border-b border-[var(--color-border)] px-2 py-1.5">
          <GroupChip active={filter === null} label="All" count={snippets.length} onClick={() => setFilter(null)} />
          {categories.map((c) => (
            <GroupChip
              key={c.id}
              active={filter === c.id}
              label={c.name}
              count={c.count}
              onClick={() => setFilter(c.id)}
            />
          ))}
          {ungroupedCount > 0 && (
            <GroupChip
              active={filter === -1}
              label="Ungrouped"
              count={ungroupedCount}
              onClick={() => setFilter(-1)}
            />
          )}
        </div>

        {importStatus && (
          <div
            className={
              "border-b border-[var(--color-border)] px-3 py-1.5 text-[11px] " +
              (importStatus.kind === "ok" ? "text-[var(--color-muted)]" : "text-red-400")
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
          {visible.length === 0 && (
            <div className="flex h-full items-center justify-center text-[12px] text-[var(--color-muted)]">
              {snippets.length === 0 ? "No snippets yet" : "No snippets in this group"}
            </div>
          )}
          {visible.map((s) => {
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
                  className={"mt-0.5 shrink-0 " + (isActive ? "text-white/80" : "text-[var(--color-accent)]")}
                />
                <div className="min-w-0 flex-1">
                  <div className="truncate font-[var(--font-mono)] font-medium">{s.abbreviation}</div>
                  <div
                    className={
                      "truncate text-[11px] " + (isActive ? "text-white/70" : "text-[var(--color-muted)]")
                    }
                  >
                    {s.title || s.body.split("\n")[0]}
                  </div>
                </div>
                {/* Group badge — only when viewing "All" so it's not redundant. */}
                {s.category && filter === null && (
                  <span
                    className={
                      "mt-0.5 shrink-0 rounded-full px-1.5 py-0.5 text-[9px] " +
                      (isActive ? "bg-white/20 text-white/90" : "bg-[var(--color-surface)] text-[var(--color-muted)]")
                    }
                  >
                    {s.category}
                  </span>
                )}
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

      {/* Right: manage-groups OR edit form OR empty state */}
      <div className="flex w-3/5 flex-col p-4">
        {managing ? (
          <ManageGroups categories={categories} onRefresh={onRefresh} onClose={() => setManaging(false)} />
        ) : form === null ? (
          <div className="flex h-full flex-col items-center justify-center gap-2 text-[12px] text-[var(--color-muted)]">
            <Zap size={24} className="opacity-30" />
            <span>
              Select a snippet to edit or click&nbsp;<b>+ New Snippet</b>
            </span>
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

            <div className="flex gap-2">
              <label className="flex flex-1 flex-col gap-1">
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
                        if (e.key === "Escape") setNewGroupName(null);
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
                value={form.body}
                onChange={(e) => setForm({ ...form, body: e.target.value })}
                placeholder="Template text that gets pasted…"
                className="flex-1 resize-none rounded border border-[var(--color-border)] bg-[var(--color-surface)] px-2 py-1.5 font-[var(--font-mono)] text-[12px] leading-5 outline-none focus:border-[var(--color-accent)]"
                onKeyDown={(e) => {
                  if (e.key === "Escape") cancel();
                }}
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

// ── Group-filter chip ─────────────────────────────────────────────────────────

function GroupChip({
  active,
  label,
  count,
  onClick,
}: {
  active: boolean;
  label: string;
  count: number;
  onClick: () => void;
}) {
  return (
    <button
      onClick={onClick}
      className={
        "flex shrink-0 items-center gap-1 rounded-full px-2.5 py-0.5 text-[11px] transition-colors " +
        (active
          ? "bg-[var(--color-accent)] text-[var(--color-accent-fg)]"
          : "bg-[var(--color-surface)] text-[var(--color-muted)] hover:text-[var(--color-fg)]")
      }
      title={label}
    >
      <span className="max-w-[9rem] truncate">{label}</span>
      <span className={active ? "opacity-80" : "opacity-60"}>{count}</span>
    </button>
  );
}

// ── Manage-groups sub-view (create / rename / reorder / delete) ────────────────

function ManageGroups({
  categories,
  onRefresh,
  onClose,
}: {
  categories: SnippetCategory[];
  onRefresh: () => void;
  onClose: () => void;
}) {
  const [newName, setNewName] = useState("");
  const [editing, setEditing] = useState<{ id: number; name: string } | null>(null);
  const [confirmDel, setConfirmDel] = useState<number | null>(null);
  const [busy, setBusy] = useState(false);

  const create = async () => {
    const n = newName.trim();
    if (!n || busy) return;
    setBusy(true);
    try {
      await createSnippetCategory(n);
      setNewName("");
      await onRefresh();
    } finally {
      setBusy(false);
    }
  };

  const commitRename = async () => {
    if (!editing) return;
    const n = editing.name.trim();
    if (n) {
      setBusy(true);
      try {
        await renameSnippetCategory(editing.id, n);
        await onRefresh();
      } finally {
        setBusy(false);
      }
    }
    setEditing(null);
  };

  const move = async (idx: number, dir: -1 | 1) => {
    const to = idx + dir;
    if (to < 0 || to >= categories.length || busy) return;
    const ids = categories.map((c) => c.id);
    [ids[idx], ids[to]] = [ids[to], ids[idx]];
    setBusy(true);
    try {
      await reorderSnippetCategories(ids);
      await onRefresh();
    } finally {
      setBusy(false);
    }
  };

  const del = async (id: number) => {
    setBusy(true);
    try {
      await deleteSnippetCategory(id);
      await onRefresh();
    } finally {
      setBusy(false);
      setConfirmDel(null);
    }
  };

  return (
    <div className="flex h-full flex-col gap-3">
      <div className="flex items-center justify-between">
        <div className="text-[11px] font-semibold uppercase tracking-wide text-[var(--color-muted)]">
          Manage groups
        </div>
        <button
          onClick={onClose}
          className="rounded px-2 py-0.5 text-[11px] text-[var(--color-muted)] hover:bg-[var(--color-surface)]"
        >
          Done
        </button>
      </div>

      {/* Create */}
      <div className="flex items-center gap-2">
        <input
          value={newName}
          onChange={(e) => setNewName(e.target.value)}
          placeholder="New group name"
          className="flex-1 rounded border border-[var(--color-border)] bg-[var(--color-surface)] px-2 py-1 text-[13px] outline-none focus:border-[var(--color-accent)]"
          onKeyDown={(e) => {
            if (e.key === "Enter") void create();
          }}
        />
        <button
          onClick={() => void create()}
          disabled={busy || !newName.trim()}
          className="flex items-center gap-1 rounded bg-[var(--color-accent)] px-2.5 py-1 text-[12px] text-[var(--color-accent-fg)] hover:opacity-90 disabled:opacity-40"
        >
          <Plus size={13} /> Add
        </button>
      </div>

      {/* List */}
      <div className="flex-1 overflow-auto">
        {categories.length === 0 ? (
          <div className="flex h-full items-center justify-center text-[12px] text-[var(--color-muted)]">
            No groups yet — add one above.
          </div>
        ) : (
          <div className="flex flex-col">
            {categories.map((c, idx) => (
              <div
                key={c.id}
                className="flex items-center gap-1.5 border-b border-[var(--color-border)] py-1.5"
              >
                <div className="flex flex-col">
                  <button
                    onClick={() => void move(idx, -1)}
                    disabled={idx === 0 || busy}
                    className="text-[var(--color-muted)] hover:text-[var(--color-fg)] disabled:opacity-25"
                    title="Move up"
                  >
                    <ArrowUp size={12} />
                  </button>
                  <button
                    onClick={() => void move(idx, 1)}
                    disabled={idx === categories.length - 1 || busy}
                    className="text-[var(--color-muted)] hover:text-[var(--color-fg)] disabled:opacity-25"
                    title="Move down"
                  >
                    <ArrowDown size={12} />
                  </button>
                </div>

                {editing?.id === c.id ? (
                  <input
                    autoFocus
                    value={editing.name}
                    onChange={(e) => setEditing({ id: c.id, name: e.target.value })}
                    onBlur={() => void commitRename()}
                    onKeyDown={(e) => {
                      if (e.key === "Enter") void commitRename();
                      if (e.key === "Escape") setEditing(null);
                    }}
                    className="flex-1 rounded border border-[var(--color-accent)] bg-[var(--color-surface)] px-2 py-0.5 text-[13px] outline-none"
                  />
                ) : (
                  <button
                    onClick={() => setEditing({ id: c.id, name: c.name })}
                    className="flex-1 truncate text-left text-[13px] hover:text-[var(--color-accent)]"
                    title="Click to rename"
                  >
                    {c.name}
                  </button>
                )}

                <span className="shrink-0 text-[10px] text-[var(--color-muted)]">
                  {c.count} snippet{c.count === 1 ? "" : "s"}
                </span>

                {confirmDel === c.id ? (
                  <div className="flex shrink-0 items-center gap-0.5">
                    <button
                      onClick={() => void del(c.id)}
                      title="Delete group (snippets are kept, just ungrouped)"
                      className="rounded p-1 text-red-400 hover:bg-red-400/10"
                    >
                      <Check size={13} />
                    </button>
                    <button
                      onClick={() => setConfirmDel(null)}
                      title="Cancel"
                      className="rounded p-1 text-[var(--color-muted)] hover:bg-[var(--color-surface)]"
                    >
                      <X size={13} />
                    </button>
                  </div>
                ) : (
                  <button
                    onClick={() => setConfirmDel(c.id)}
                    title="Delete group — its snippets are kept and simply ungrouped"
                    className="shrink-0 rounded p-1 text-[var(--color-muted)] hover:bg-[var(--color-border)] hover:text-red-400"
                  >
                    <Trash2 size={12} />
                  </button>
                )}
              </div>
            ))}
          </div>
        )}
      </div>

      <p className="text-[10px] leading-snug text-[var(--color-muted)]">
        Deleting a group keeps its snippets — they just become <b>Ungrouped</b>. Click a name to rename;
        use the arrows to reorder.
      </p>
    </div>
  );
}
