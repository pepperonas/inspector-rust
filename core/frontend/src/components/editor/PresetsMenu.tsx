import { useCallback, useEffect, useRef, useState } from "react";
import { Check, FolderOpen, Pencil, Plus, Star, Trash2, X } from "lucide-react";
import {
  recordPresetDelete,
  recordPresetRename,
  recordPresetSave,
  recordPresetSetDefault,
  recordPresetsList,
  recordPresetsReveal,
  type RecordPresetEntry,
} from "../../lib/ipc";
import { useT, type MessageKey } from "../../lib/i18n";

/** Longest preset name (Rust refuses longer ones, see `presets::MAX_NAME_CHARS`). */
export const PRESET_NAME_MAX = 80;

const reasonText = (t: (k: MessageKey, v?: Record<string, string | number>) => string, e: unknown) => {
  const s = typeof e === "string" ? e : e instanceof Error ? e.message : String(e);
  return s.startsWith("record.editor.") ? t(s as MessageKey) : s;
};

const rowBtnBase = "rounded-md p-1 hover:bg-[var(--color-bg)] disabled:opacity-40";
const rowBtn = rowBtnBase + " text-[var(--color-muted)] hover:text-[var(--color-fg)]";

/**
 * The editor's style presets: the look (background, frame, pointer, aspect) saved as files in
 * the recordings folder's `Presets/`. Picking one applies it (the editor records one undo
 * step); a star marks the one new projects start with. Deleting asks inline first.
 */
export function PresetsMenu({
  sessionId,
  onApply,
  onClose,
}: {
  sessionId: number;
  onApply: (entry: RecordPresetEntry) => void;
  onClose: () => void;
}) {
  const t = useT();
  const [presets, setPresets] = useState<RecordPresetEntry[] | null>(null);
  const [defaultFile, setDefaultFile] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  /** The "save current style" form (`null`: closed). */
  const [saveName, setSaveName] = useState<string | null>(null);
  const [renaming, setRenaming] = useState<{ file: string; name: string } | null>(null);
  /** The preset whose delete waits for its confirmation. */
  const [armed, setArmed] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const box = useRef<HTMLDivElement>(null);

  const reload = useCallback(async () => {
    try {
      const l = await recordPresetsList();
      setPresets(l.presets);
      setDefaultFile(l.defaultFile);
    } catch (e) {
      setPresets([]);
      setError(reasonText(t, e));
    }
  }, [t]);

  useEffect(() => {
    void reload();
  }, [reload]);

  /** Runs a preset change; its error is shown in the menu, the list is read again either way. */
  const run = useCallback(
    async (op: () => Promise<unknown>) => {
      setBusy(true);
      setError(null);
      try {
        await op();
        return true;
      } catch (e) {
        setError(reasonText(t, e));
        return false;
      } finally {
        setBusy(false);
        await reload();
      }
    },
    [reload, t],
  );

  // Esc steps back: an open form or armed delete first, then the menu itself.
  const stateRef = useRef({ saveName, renaming, armed });
  useEffect(() => {
    stateRef.current = { saveName, renaming, armed };
  }, [saveName, renaming, armed]);
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== "Escape") return;
      e.preventDefault();
      e.stopPropagation();
      const s = stateRef.current;
      if (s.armed) setArmed(null);
      else if (s.renaming) setRenaming(null);
      else if (s.saveName !== null) setSaveName(null);
      else onClose();
    };
    const onDown = (e: PointerEvent) => {
      const el = box.current;
      if (el && e.target instanceof Node && !el.contains(e.target) && !el.parentElement?.contains(e.target)) onClose();
    };
    window.addEventListener("keydown", onKey, true);
    window.addEventListener("pointerdown", onDown, true);
    return () => {
      window.removeEventListener("keydown", onKey, true);
      window.removeEventListener("pointerdown", onDown, true);
    };
  }, [onClose]);

  const save = async () => {
    const name = (saveName ?? "").trim();
    if (!name) return;
    if (await run(() => recordPresetSave(sessionId, name))) setSaveName(null);
  };
  const rename = async () => {
    if (!renaming) return;
    const { file, name } = renaming;
    if (await run(() => recordPresetRename(file, name.trim()))) setRenaming(null);
  };

  return (
    <div
      ref={box}
      role="dialog"
      aria-label={t("rec4.preset.title")}
      data-testid="ed-presets"
      className="pop-enter absolute right-0 top-full z-30 mt-1 w-80 rounded-lg border border-[var(--color-border)] bg-[var(--color-surface)] p-1.5 shadow-xl"
    >
      <p className="px-2 pb-1.5 pt-1 text-[11px] text-[var(--color-muted)]">{t("rec4.preset.hint")}</p>
      {presets === null ? (
        <p className="px-2 py-2 text-[12px] text-[var(--color-muted)]">{t("rec4.preset.loading")}</p>
      ) : presets.length === 0 ? (
        <p className="px-2 py-2 text-[12px] text-[var(--color-muted)]">{t("rec4.preset.none")}</p>
      ) : (
        <ul className="max-h-64 overflow-y-auto">
          {presets.map((p) => {
            const isDefault = p.file === defaultFile;
            if (armed === p.file) {
              return (
                <li key={p.file} className="flex items-center gap-1.5 rounded-md px-2 py-1.5 text-[12px]" data-testid="ed-preset-armed">
                  <span className="min-w-0 flex-1 truncate">{t("rec4.preset.delete_confirm", { name: p.name })}</span>
                  <button
                    className="rounded-md bg-red-600 px-2 py-0.5 text-[11px] font-medium text-white disabled:opacity-40"
                    disabled={busy}
                    onClick={() => void run(() => recordPresetDelete(p.file)).then(() => setArmed(null))}
                    data-testid="ed-preset-delete-confirm"
                  >
                    {t("rec4.preset.delete")}
                  </button>
                  <button className={rowBtn} onClick={() => setArmed(null)} aria-label={t("rec4.preset.cancel")}>
                    <X size={13} />
                  </button>
                </li>
              );
            }
            if (renaming?.file === p.file) {
              return (
                <li key={p.file} className="flex items-center gap-1.5 px-1 py-1">
                  <input
                    autoFocus
                    className="min-w-0 flex-1 rounded-md border border-[var(--color-border)] bg-[var(--color-bg)] px-2 py-1 text-[12px]"
                    value={renaming.name}
                    maxLength={PRESET_NAME_MAX}
                    aria-label={t("rec4.preset.rename")}
                    onChange={(e) => setRenaming({ file: p.file, name: e.target.value })}
                    onKeyDown={(e) => {
                      if (e.key === "Enter") {
                        e.preventDefault();
                        void rename();
                      }
                    }}
                    data-testid="ed-preset-rename-input"
                  />
                  <button className={rowBtn} disabled={busy || !renaming.name.trim()} onClick={() => void rename()} aria-label={t("rec4.preset.save_confirm")}>
                    <Check size={13} />
                  </button>
                </li>
              );
            }
            return (
              <li key={p.file} className="flex items-center gap-0.5">
                <button
                  className="flex min-w-0 flex-1 flex-col items-start rounded-md px-2 py-1.5 text-left text-[12px] hover:bg-[var(--color-bg)] disabled:cursor-default disabled:opacity-60 disabled:hover:bg-transparent"
                  disabled={!p.ok || busy}
                  onClick={() => onApply(p)}
                  title={p.ok ? t("rec4.preset.apply_hint") : undefined}
                  data-testid="ed-preset-apply"
                >
                  <span className="w-full truncate">{p.name}</span>
                  {!p.ok && p.error && (
                    <span className="w-full truncate text-[11px] text-red-500">{reasonText(t, p.error)}</span>
                  )}
                </button>
                <button
                  className={isDefault ? rowBtnBase + " text-[var(--color-accent)]" : rowBtn}
                  disabled={busy || (!p.ok && !isDefault)}
                  onClick={() => void run(() => recordPresetSetDefault(isDefault ? null : p.file))}
                  aria-pressed={isDefault}
                  aria-label={isDefault ? t("rec4.preset.default_on") : t("rec4.preset.default")}
                  title={isDefault ? t("rec4.preset.default_on") : t("rec4.preset.default")}
                  data-testid="ed-preset-default"
                >
                  <Star size={13} fill={isDefault ? "currentColor" : "none"} />
                </button>
                <button
                  className={rowBtn}
                  disabled={busy || !p.ok}
                  onClick={() => {
                    setArmed(null);
                    setRenaming({ file: p.file, name: p.name });
                  }}
                  aria-label={t("rec4.preset.rename")}
                  title={t("rec4.preset.rename")}
                >
                  <Pencil size={13} />
                </button>
                <button
                  className={rowBtn}
                  disabled={busy}
                  onClick={() => {
                    setRenaming(null);
                    setArmed(p.file);
                  }}
                  aria-label={t("rec4.preset.delete")}
                  title={t("rec4.preset.delete")}
                  data-testid="ed-preset-delete"
                >
                  <Trash2 size={13} />
                </button>
              </li>
            );
          })}
        </ul>
      )}
      {error && (
        <p className="px-2 py-1 text-[11px] text-red-500" role="alert" data-testid="ed-preset-error">
          {error}
        </p>
      )}
      <div className="mt-1 border-t border-[var(--color-border)] pt-1">
        {saveName === null ? (
          <button
            className="flex w-full items-center gap-2 rounded-md px-2 py-1.5 text-left text-[12px] hover:bg-[var(--color-bg)]"
            onClick={() => {
              setRenaming(null);
              setArmed(null);
              setSaveName("");
            }}
            data-testid="ed-preset-save-open"
          >
            <Plus size={13} /> {t("rec4.preset.save")}
          </button>
        ) : (
          <div className="flex items-center gap-1.5 px-1 py-1">
            <input
              autoFocus
              className="min-w-0 flex-1 rounded-md border border-[var(--color-border)] bg-[var(--color-bg)] px-2 py-1 text-[12px]"
              value={saveName}
              maxLength={PRESET_NAME_MAX}
              placeholder={t("rec4.preset.name_placeholder")}
              aria-label={t("rec4.preset.name_placeholder")}
              onChange={(e) => setSaveName(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Enter") {
                  e.preventDefault();
                  void save();
                }
              }}
              data-testid="ed-preset-name"
            />
            <button
              className="rounded-md bg-[var(--color-accent)] px-2 py-1 text-[11px] font-medium text-[var(--color-accent-fg)] disabled:opacity-40"
              disabled={busy || !saveName.trim()}
              onClick={() => void save()}
              data-testid="ed-preset-save"
            >
              {t("rec4.preset.save_confirm")}
            </button>
          </div>
        )}
        <button
          className="flex w-full items-center gap-2 rounded-md px-2 py-1.5 text-left text-[12px] hover:bg-[var(--color-bg)]"
          onClick={() => void run(() => recordPresetsReveal())}
          data-testid="ed-preset-reveal"
        >
          <FolderOpen size={13} /> {t("rec4.preset.reveal")}
        </button>
      </div>
    </div>
  );
}
