import { useEffect, useMemo, useRef, useState } from "react";
import {
  Check,
  Copy,
  Download,
  GripVertical,
  KeyRound,
  Pencil,
  Plus,
  Trash2,
  Upload,
  X,
} from "lucide-react";
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
import {
  setSuppressHide,
  totpAdd,
  totpCurrentCodesAll,
  totpDelete,
  totpDeleteAll,
  totpExport,
  totpImport,
  totpImportFile,
  totpList,
  totpRemoveDuplicates,
  totpSetOrder,
  totpUpdate,
} from "../lib/ipc";
import type { TotpCode, TotpEntry } from "../lib/totp";

/**
 * Full-overlay TOTP / 2FA management. Replaces the popup body, owns
 * keyboard input (Esc to exit). Three tabs:
 *
 *   - **List**: every entry with live code + countdown ring + copy
 *     + delete buttons.
 *   - **Add**: manual form (Issuer / Account / Secret, plus the
 *     advanced trio digits/period/algorithm hidden behind a toggle).
 *   - **Import / Export**: paste-and-go for any supported format,
 *     plus a copy-all-as-otpauth-URIs button.
 *
 * Live codes are fetched every 1 s via `totpCurrentCodesAll`. The
 * countdown ring on each row interpolates locally between fetches
 * so it looks smooth instead of ticking once a second.
 */

interface Props {
  onExit: () => void;
}

type Tab = "list" | "add" | "import";

function importSummary(
  r: { added: number; skipped?: number; failed?: number },
  from?: string,
): string {
  const src = from ? ` from ${from}` : "";
  const skip = r.skipped ? `, ${r.skipped} duplicate(s) skipped` : "";
  const fail = r.failed ? `, ${r.failed} invalid entr${r.failed === 1 ? "y" : "ies"} dropped` : "";
  return `${r.added} entr${r.added === 1 ? "y" : "ies"} imported${src}${skip}${fail}.`;
}

export function TotpOverlay({ onExit }: Props) {
  const [tab, setTab] = useState<Tab>("list");
  const [entries, setEntries] = useState<TotpEntry[]>([]);
  const [codes, setCodes] = useState<Map<number, TotpCode>>(new Map());
  // Bumped on every 1s tick; used to drive the smooth countdown ring.
  const [tickNow, setTickNow] = useState(() => Date.now());
  const [busy, setBusy] = useState(false);
  const [toast, setToast] = useState<{ kind: "ok" | "err"; message: string } | null>(null);

  // Add/Edit-form state. `editingId != null` → the Add tab is an Edit form.
  const [editingId, setEditingId] = useState<number | null>(null);
  const [addIssuer, setAddIssuer] = useState("");
  const [addAccount, setAddAccount] = useState("");
  const [addSecret, setAddSecret] = useState("");
  const [addAdvanced, setAddAdvanced] = useState(false);
  const [addDigits, setAddDigits] = useState(6);
  const [addPeriod, setAddPeriod] = useState(30);
  const [addAlgorithm, setAddAlgorithm] = useState("SHA1");

  // Import-tab state
  const [importText, setImportText] = useState("");
  const [dragOver, setDragOver] = useState(false);

  // Esc to exit — owned by this overlay.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.preventDefault();
        e.stopPropagation();
        onExit();
      }
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [onExit]);

  // Keep the popup alive while the overlay is open — so dragging an import file
  // in from Finder doesn't dismiss it on focus-loss (only Esc closes it) — and
  // wire OS drag-and-drop: dropping a file anywhere imports it.
  useEffect(() => {
    void setSuppressHide(true).catch(() => undefined);
    let cancelled = false;
    let unlisten: (() => void) | undefined;
    getCurrentWebviewWindow()
      .onDragDropEvent((event) => {
        const p = event.payload;
        if (p.type === "enter" || p.type === "over") {
          setDragOver(true);
        } else if (p.type === "leave") {
          setDragOver(false);
        } else if (p.type === "drop") {
          setDragOver(false);
          const path = p.paths[0];
          if (!path) return;
          setBusy(true);
          totpImportFile(path)
            .then(async (result) => {
              if (result.error) {
                setToast({ kind: "err", message: result.error });
                return;
              }
              setToast({
                kind: "ok",
                message: importSummary(result, path.split("/").pop()),
              });
              const [list, currentCodes] = await Promise.all([
                totpList(),
                totpCurrentCodesAll(),
              ]);
              setEntries(list);
              setCodes(new Map(currentCodes.map((c) => [c.id, c])));
              setTab("list");
            })
            .catch((e) => setToast({ kind: "err", message: String(e) }))
            .finally(() => setBusy(false));
        }
      })
      .then((u) => {
        if (cancelled) u();
        else unlisten = u;
      })
      .catch(() => undefined);
    return () => {
      cancelled = true;
      unlisten?.();
      void setSuppressHide(false).catch(() => undefined);
    };
  }, []);

  // Initial fetch + 1 s polling of live codes.
  useEffect(() => {
    let cancelled = false;
    const refresh = async () => {
      try {
        const [list, currentCodes] = await Promise.all([
          totpList(),
          totpCurrentCodesAll(),
        ]);
        if (cancelled) return;
        setEntries(list);
        setCodes(new Map(currentCodes.map((c) => [c.id, c])));
      } catch (e) {
        if (!cancelled) {
          console.error("totp refresh failed", e);
        }
      }
    };
    void refresh();
    const interval = setInterval(refresh, 1000);
    // The 100 ms tick only drives the countdown rings, which render on the
    // List tab — don't burn 10 re-renders/s while Add or Import is showing.
    const tick =
      tab === "list" ? setInterval(() => setTickNow(Date.now()), 100) : undefined;
    return () => {
      cancelled = true;
      clearInterval(interval);
      if (tick !== undefined) clearInterval(tick);
    };
  }, [tab]);

  const doAdd = async () => {
    // On edit, a blank secret keeps the current one; on add it's required.
    if (!addIssuer.trim() || (editingId === null && !addSecret.trim())) {
      setToast({ kind: "err", message: "Issuer and Secret are required." });
      return;
    }
    setBusy(true);
    setToast(null);
    try {
      if (editingId !== null) {
        await totpUpdate({
          id: editingId,
          issuer: addIssuer.trim(),
          account: addAccount.trim(),
          secret: addSecret.trim(), // blank → keep current
          digits: addDigits,
          period: addPeriod,
          algorithm: addAlgorithm,
        });
      } else {
        await totpAdd({
          issuer: addIssuer.trim(),
          account: addAccount.trim(),
          secret: addSecret.trim(),
          digits: addDigits,
          period: addPeriod,
          algorithm: addAlgorithm,
        });
      }
      const wasEdit = editingId !== null;
      resetAddForm();
      setToast({ kind: "ok", message: wasEdit ? "Entry updated." : "Entry added." });
      await refreshList();
      setTab("list");
    } catch (e) {
      setToast({ kind: "err", message: String(e) });
    } finally {
      setBusy(false);
    }
  };

  const refreshList = async () => {
    const [list, currentCodes] = await Promise.all([totpList(), totpCurrentCodesAll()]);
    setEntries(list);
    setCodes(new Map(currentCodes.map((c) => [c.id, c])));
  };

  // Actual delete — confirmation is handled inline in the list (window.confirm
  // is unreliable in the Tauri webview, which is why delete "didn't work").
  const doDelete = async (id: number, label: string) => {
    setBusy(true);
    try {
      await totpDelete(id);
      setEntries((es) => es.filter((e) => e.id !== id));
      setCodes((c) => {
        const next = new Map(c);
        next.delete(id);
        return next;
      });
      setToast({ kind: "ok", message: `"${label}" deleted.` });
    } catch (e) {
      setToast({ kind: "err", message: String(e) });
    } finally {
      setBusy(false);
    }
  };

  const resetAddForm = () => {
    setEditingId(null);
    setAddIssuer("");
    setAddAccount("");
    setAddSecret("");
    setAddAdvanced(false);
    setAddDigits(6);
    setAddPeriod(30);
    setAddAlgorithm("SHA1");
  };

  // Open the Add tab pre-filled as an Edit form. The secret is never exposed by
  // the list, so it's left blank (a placeholder says "leave blank to keep").
  const startEdit = (e: TotpEntry) => {
    setEditingId(e.id);
    setAddIssuer(e.issuer);
    setAddAccount(e.account);
    setAddSecret("");
    setAddDigits(e.digits);
    setAddPeriod(e.period);
    setAddAlgorithm(e.algorithm);
    setAddAdvanced(e.digits !== 6 || e.period !== 30 || e.algorithm !== "SHA1");
    setToast(null);
    setTab("add");
  };

  // Persist a manual drag-reorder (called by the list on drop).
  const doReorder = async (ordered: TotpEntry[]) => {
    setEntries(ordered);
    try {
      await totpSetOrder(ordered.map((e) => e.id));
    } catch (e) {
      setToast({ kind: "err", message: String(e) });
    }
  };

  const doRemoveDuplicates = async () => {
    setBusy(true);
    try {
      const removed = await totpRemoveDuplicates();
      await refreshList();
      setToast({
        kind: "ok",
        message: removed > 0 ? `${removed} duplicate(s) removed.` : "No duplicates found.",
      });
    } catch (e) {
      setToast({ kind: "err", message: String(e) });
    } finally {
      setBusy(false);
    }
  };

  const doDeleteAll = async () => {
    setBusy(true);
    try {
      const removed = await totpDeleteAll();
      await refreshList();
      setToast({ kind: "ok", message: `${removed} entr${removed === 1 ? "y" : "ies"} deleted.` });
    } catch (e) {
      setToast({ kind: "err", message: String(e) });
    } finally {
      setBusy(false);
    }
  };

  const doCopy = async (id: number, label: string) => {
    const code = codes.get(id)?.code;
    if (!code) return;
    try {
      const { writeText } = await import("@tauri-apps/plugin-clipboard-manager");
      await writeText(code);
      setToast({ kind: "ok", message: `${label} → clipboard` });
    } catch (e) {
      setToast({ kind: "err", message: String(e) });
    }
  };

  const doImport = async () => {
    if (!importText.trim()) {
      setToast({ kind: "err", message: "Nothing to import." });
      return;
    }
    setBusy(true);
    try {
      const result = await totpImport(importText);
      if (result.error) {
        setToast({ kind: "err", message: result.error });
      } else {
        setToast({ kind: "ok", message: importSummary(result) });
        setImportText("");
        await refreshList();
        setTab("list");
      }
    } catch (e) {
      setToast({ kind: "err", message: String(e) });
    } finally {
      setBusy(false);
    }
  };

  const doExport = async () => {
    try {
      const uris = await totpExport();
      if (!uris.trim()) {
        setToast({ kind: "err", message: "No entries to export." });
        return;
      }
      const { writeText } = await import("@tauri-apps/plugin-clipboard-manager");
      await writeText(uris);
      setToast({
        kind: "ok",
        message: `${entries.length} entries exported as otpauth URIs to clipboard. Caution: plaintext!`,
      });
    } catch (e) {
      setToast({ kind: "err", message: String(e) });
    }
  };

  return (
    <div className="relative flex h-full w-full flex-col bg-[var(--color-bg)] text-[var(--color-fg)]">
      {/* Drag-and-drop import hint — the drop is handled window-wide. */}
      {dragOver && (
        <div className="pointer-events-none absolute inset-0 z-50 flex flex-col items-center justify-center gap-2 border-2 border-dashed border-[var(--color-accent)] bg-[var(--color-bg)]/85 backdrop-blur-sm">
          <Upload size={28} className="text-[var(--color-accent)]" />
          <span className="text-[13px] font-semibold">Drop to import</span>
          <span className="text-[11px] text-[var(--color-muted)]">
            otpauth URIs · Aegis / 2FAS / OTPManager JSON · migration QR
          </span>
        </div>
      )}
      {/* Top bar */}
      <div className="flex items-center justify-between border-b border-[var(--color-border)] px-4 py-2 text-[12px]">
        <div className="flex items-center gap-2">
          <KeyRound size={14} className="text-[var(--color-accent)]" />
          <span className="font-semibold">2FA · TOTP</span>
          <span className="text-[var(--color-muted)]">·</span>
          <TabButton active={tab === "list"} onClick={() => setTab("list")}>
            List {entries.length > 0 ? `(${entries.length})` : ""}
          </TabButton>
          <TabButton active={tab === "add"} onClick={() => setTab("add")}>
            {editingId !== null ? "Edit" : "Add"}
          </TabButton>
          <TabButton active={tab === "import"} onClick={() => setTab("import")}>
            Import / Export
          </TabButton>
        </div>
        <button
          onClick={onExit}
          className="flex items-center gap-1 rounded border border-[var(--color-border)] px-2 py-0.5 text-[10px] hover:border-[var(--color-accent)] hover:text-[var(--color-accent)]"
        >
          <X size={11} />
          Esc
        </button>
      </div>

      {/* Body */}
      <div className="flex-1 overflow-auto p-4">
        {tab === "list" && (
          <ListTab
            entries={entries}
            codes={codes}
            tickNow={tickNow}
            onCopy={doCopy}
            onDelete={doDelete}
            onEdit={startEdit}
            onReorder={doReorder}
            onRemoveDuplicates={doRemoveDuplicates}
            onDeleteAll={doDeleteAll}
            busy={busy}
          />
        )}
        {tab === "add" && (
          <AddTab
            issuer={addIssuer}
            setIssuer={setAddIssuer}
            account={addAccount}
            setAccount={setAddAccount}
            secret={addSecret}
            setSecret={setAddSecret}
            advanced={addAdvanced}
            setAdvanced={setAddAdvanced}
            digits={addDigits}
            setDigits={setAddDigits}
            period={addPeriod}
            setPeriod={setAddPeriod}
            algorithm={addAlgorithm}
            setAlgorithm={setAddAlgorithm}
            onSubmit={doAdd}
            editing={editingId !== null}
            onCancel={() => {
              resetAddForm();
              setTab("list");
            }}
            busy={busy}
          />
        )}
        {tab === "import" && (
          <ImportExportTab
            importText={importText}
            setImportText={setImportText}
            onImport={doImport}
            onExport={doExport}
            entryCount={entries.length}
            busy={busy}
          />
        )}
      </div>

      {/* Toast */}
      {toast && (
        <div
          className={
            "mx-4 mb-3 rounded border px-3 py-2 text-[12px] " +
            (toast.kind === "ok"
              ? "border-emerald-500/40 bg-emerald-500/10"
              : "border-rose-500/40 bg-rose-500/10")
          }
        >
          {toast.message}
        </div>
      )}
    </div>
  );
}

function TabButton({
  active,
  onClick,
  children,
}: {
  active: boolean;
  onClick: () => void;
  children: React.ReactNode;
}) {
  return (
    <button
      onClick={onClick}
      className={
        "rounded px-2 py-0.5 text-[12px] " +
        (active
          ? "bg-[var(--color-accent)] text-[var(--color-accent-fg)]"
          : "text-[var(--color-muted)] hover:bg-[var(--color-surface)]")
      }
    >
      {children}
    </button>
  );
}

// ── List tab ─────────────────────────────────────────────────────────

function ListTab({
  entries,
  codes,
  tickNow,
  onCopy,
  onDelete,
  onEdit,
  onReorder,
  onRemoveDuplicates,
  onDeleteAll,
  busy,
}: {
  entries: TotpEntry[];
  codes: Map<number, TotpCode>;
  tickNow: number;
  onCopy: (id: number, label: string) => void;
  onDelete: (id: number, label: string) => void;
  onEdit: (e: TotpEntry) => void;
  onReorder: (ordered: TotpEntry[]) => void;
  onRemoveDuplicates: () => void;
  onDeleteAll: () => void;
  busy: boolean;
}) {
  // Local order for smooth pointer-drag reordering. Pointer events (not HTML5
  // DnD) because Tauri's OS drag-drop — which we use for file import — swallows
  // HTML5 element drags.
  const [order, setOrder] = useState<TotpEntry[]>(entries);
  const orderRef = useRef(order);
  const draggingRef = useRef<number | null>(null);
  const [draggingId, setDraggingId] = useState<number | null>(null);
  const [confirmId, setConfirmId] = useState<number | null>(null);
  const [confirmClear, setConfirmClear] = useState(false);
  const rowRefs = useRef<Map<number, HTMLDivElement>>(new Map());

  // Removes the live drag's window listeners; set while a drag is active.
  // Needed so unmount (Esc closes the overlay mid-drag) detaches them — they'd
  // otherwise stay on `window` forever and the next unrelated pointerup would
  // fire onReorder with stale state against the unmounted component.
  const dragCleanupRef = useRef<(() => void) | null>(null);

  useEffect(() => {
    orderRef.current = order;
  }, [order]);
  useEffect(() => {
    if (draggingRef.current === null) setOrder(entries); // don't clobber a live drag
  }, [entries]);
  useEffect(
    () => () => {
      dragCleanupRef.current?.();
    },
    [],
  );

  const beginDrag = (ev: React.PointerEvent, id: number) => {
    ev.preventDefault();
    draggingRef.current = id;
    setDraggingId(id);
    const move = (e: PointerEvent) => {
      const cur = orderRef.current;
      const dragIdx = cur.findIndex((x) => x.id === draggingRef.current);
      if (dragIdx < 0) return;
      let target = cur.length - 1;
      for (let i = 0; i < cur.length; i++) {
        const el = rowRefs.current.get(cur[i].id);
        if (!el) continue;
        const r = el.getBoundingClientRect();
        if (e.clientY < r.top + r.height / 2) {
          target = i;
          break;
        }
      }
      if (target !== dragIdx) {
        const next = [...cur];
        const [m] = next.splice(dragIdx, 1);
        next.splice(target, 0, m);
        orderRef.current = next;
        setOrder(next);
      }
    };
    const detach = () => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", up);
      dragCleanupRef.current = null;
    };
    const up = () => {
      detach();
      const wasDragging = draggingRef.current !== null;
      draggingRef.current = null;
      setDraggingId(null);
      if (wasDragging) onReorder(orderRef.current);
    };
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", up);
    // Unmount mid-drag → just detach (no onReorder — the drop never happened).
    dragCleanupRef.current = () => {
      detach();
      draggingRef.current = null;
    };
  };

  if (entries.length === 0) {
    return (
      <div className="flex h-full flex-col items-center justify-center gap-3 text-center text-[var(--color-muted)]">
        <KeyRound size={32} className="opacity-50" />
        <div className="text-[14px]">No 2FA entries yet.</div>
        <div className="text-[12px]">
          Switch to <b>Add</b> or <b>Import</b> to set up your
          authenticator accounts.
        </div>
      </div>
    );
  }

  const iconBtn =
    "rounded border border-[var(--color-border)] p-1.5 text-[var(--color-muted)] disabled:opacity-40";

  return (
    <div className="flex flex-col gap-2">
      {/* Toolbar: reorder hint + cleanup */}
      <div className="mb-1 flex items-center gap-2 text-[11px]">
        <span className="mr-auto text-[var(--color-muted)]">Drag the ⠿ handle to reorder.</span>
        <button
          onClick={onRemoveDuplicates}
          disabled={busy}
          className="rounded border border-[var(--color-border)] px-2 py-1 hover:border-[var(--color-accent)] hover:text-[var(--color-accent)] disabled:opacity-40"
        >
          Remove duplicates
        </button>
        {confirmClear ? (
          <span className="flex items-center gap-1">
            <span className="text-rose-500">Delete all?</span>
            <button
              onClick={() => {
                setConfirmClear(false);
                onDeleteAll();
              }}
              className="rounded border border-rose-500/60 px-2 py-1 text-rose-500 hover:bg-rose-500/10"
            >
              Yes
            </button>
            <button
              onClick={() => setConfirmClear(false)}
              className="rounded border border-[var(--color-border)] px-2 py-1"
            >
              No
            </button>
          </span>
        ) : (
          <button
            onClick={() => setConfirmClear(true)}
            disabled={busy}
            className="rounded border border-[var(--color-border)] px-2 py-1 hover:border-rose-500/60 hover:text-rose-500 disabled:opacity-40"
          >
            Clear all
          </button>
        )}
      </div>

      {order.map((e) => {
        const c = codes.get(e.id);
        const label = `${e.issuer || "?"}${e.account ? " · " + e.account : ""}`;
        const confirming = confirmId === e.id;
        return (
          <div
            key={e.id}
            ref={(el) => {
              if (el) rowRefs.current.set(e.id, el);
              else rowRefs.current.delete(e.id);
            }}
            className={
              "flex items-center gap-2 rounded-lg border p-3 " +
              (draggingId === e.id
                ? "border-[var(--color-accent)] bg-[var(--color-accent)]/10 shadow-lg"
                : "border-[var(--color-border)] bg-[var(--color-surface)]")
            }
          >
            <button
              onPointerDown={(ev) => beginDrag(ev, e.id)}
              title="Drag to reorder"
              className="cursor-grab touch-none text-[var(--color-muted)] hover:text-[var(--color-fg)] active:cursor-grabbing"
            >
              <GripVertical size={16} />
            </button>
            <CountdownRing
              secondsRemaining={c?.seconds_remaining ?? 0}
              period={e.period}
              tickNow={tickNow}
            />
            <div className="flex flex-1 flex-col overflow-hidden">
              <div className="truncate text-[13px] font-semibold">
                {e.issuer || "(no issuer)"}
              </div>
              {e.account && (
                <div className="truncate text-[11px] text-[var(--color-muted)]">{e.account}</div>
              )}
            </div>
            <button
              onClick={() => onCopy(e.id, label)}
              disabled={busy || !c}
              className="rounded border border-[var(--color-border)] px-3 py-1.5 font-[var(--font-mono)] text-[20px] font-semibold tracking-[0.15em] tabular-nums hover:border-[var(--color-accent)] hover:text-[var(--color-accent)] disabled:opacity-40"
              title="Copy code to clipboard"
            >
              {c?.code ?? "…"}
            </button>
            {confirming ? (
              <>
                <button
                  onClick={() => {
                    setConfirmId(null);
                    onDelete(e.id, label);
                  }}
                  title="Confirm delete"
                  className="rounded border border-rose-500/60 p-1.5 text-rose-500 hover:bg-rose-500/10"
                >
                  <Check size={14} />
                </button>
                <button
                  onClick={() => setConfirmId(null)}
                  title="Cancel"
                  className={iconBtn + " hover:text-[var(--color-fg)]"}
                >
                  <X size={14} />
                </button>
              </>
            ) : (
              <>
                <button
                  onClick={() => onCopy(e.id, label)}
                  disabled={busy || !c}
                  title="Copy"
                  className={iconBtn + " hover:border-[var(--color-accent)] hover:text-[var(--color-accent)]"}
                >
                  <Copy size={14} />
                </button>
                <button
                  onClick={() => onEdit(e)}
                  disabled={busy}
                  title="Edit"
                  className={iconBtn + " hover:border-[var(--color-accent)] hover:text-[var(--color-accent)]"}
                >
                  <Pencil size={14} />
                </button>
                <button
                  onClick={() => setConfirmId(e.id)}
                  disabled={busy}
                  title="Delete"
                  className={iconBtn + " hover:border-rose-500/60 hover:text-rose-500"}
                >
                  <Trash2 size={14} />
                </button>
              </>
            )}
          </div>
        );
      })}
    </div>
  );
}

/** SVG-based circular countdown indicator. Smoothly interpolates
 *  between server-side `seconds_remaining` values (which only update
 *  once per 1 s IPC poll) using the local `tickNow` clock. */
function CountdownRing({
  secondsRemaining,
  period,
  tickNow,
}: {
  secondsRemaining: number;
  period: number;
  tickNow: number;
}) {
  // We track a "reference" wall-clock of when we last got a fresh
  // server value, plus the value at that moment. Local interpolation
  // = server_value - elapsed_since_reference.
  // (useState would be overkill — we just compute from props each render.)
  const drift = (tickNow % 1000) / 1000;
  const localRemaining = Math.max(0, secondsRemaining - drift);
  const fraction = localRemaining / period;
  const r = 14;
  const c = 2 * Math.PI * r;
  const offset = c * (1 - fraction);
  // Color: green when fresh, amber when ≤5s, red when ≤2s.
  const color =
    localRemaining <= 2
      ? "stroke-rose-500"
      : localRemaining <= 5
        ? "stroke-amber-500"
        : "stroke-emerald-500";
  return (
    <div className="relative h-10 w-10 shrink-0">
      <svg viewBox="0 0 32 32" className="absolute inset-0 -rotate-90">
        <circle
          cx="16"
          cy="16"
          r={r}
          fill="none"
          className="stroke-[var(--color-border)]"
          strokeWidth="2.5"
        />
        <circle
          cx="16"
          cy="16"
          r={r}
          fill="none"
          className={color + " transition-[stroke-dashoffset] duration-100"}
          strokeWidth="2.5"
          strokeDasharray={c}
          strokeDashoffset={offset}
          strokeLinecap="round"
        />
      </svg>
      <div className="absolute inset-0 flex items-center justify-center font-[var(--font-mono)] text-[10px] font-semibold tabular-nums text-[var(--color-muted)]">
        {Math.ceil(localRemaining)}
      </div>
    </div>
  );
}

// ── Add tab ──────────────────────────────────────────────────────────

function AddTab(props: {
  issuer: string;
  setIssuer: (s: string) => void;
  account: string;
  setAccount: (s: string) => void;
  secret: string;
  setSecret: (s: string) => void;
  advanced: boolean;
  setAdvanced: (b: boolean) => void;
  digits: number;
  setDigits: (n: number) => void;
  period: number;
  setPeriod: (n: number) => void;
  algorithm: string;
  setAlgorithm: (s: string) => void;
  onSubmit: () => void;
  editing: boolean;
  onCancel: () => void;
  busy: boolean;
}) {
  return (
    <div className="mx-auto flex max-w-md flex-col gap-3">
      {props.editing && (
        <div className="flex items-center justify-between text-[12px]">
          <span className="font-semibold text-[var(--color-accent)]">Editing entry</span>
          <button
            onClick={props.onCancel}
            className="rounded border border-[var(--color-border)] px-2 py-0.5 text-[11px] hover:text-[var(--color-fg)]"
          >
            Cancel
          </button>
        </div>
      )}
      <label className="flex flex-col gap-1 text-[12px]">
        <span className="font-semibold">Issuer / Service</span>
        <input
          type="text"
          value={props.issuer}
          onChange={(e) => props.setIssuer(e.target.value)}
          placeholder="Amazon, GitHub, …"
          className="rounded border border-[var(--color-border)] bg-[var(--color-surface)] px-2 py-1.5 text-[13px] focus:border-[var(--color-accent)] focus:outline-none"
          autoFocus
        />
      </label>
      <label className="flex flex-col gap-1 text-[12px]">
        <span className="font-semibold">Account (optional)</span>
        <input
          type="text"
          value={props.account}
          onChange={(e) => props.setAccount(e.target.value)}
          placeholder="user@example.com"
          className="rounded border border-[var(--color-border)] bg-[var(--color-surface)] px-2 py-1.5 text-[13px] focus:border-[var(--color-accent)] focus:outline-none"
        />
      </label>
      <label className="flex flex-col gap-1 text-[12px]">
        <span className="font-semibold">Secret (Base32){props.editing ? " — optional" : ""}</span>
        <textarea
          value={props.secret}
          onChange={(e) => props.setSecret(e.target.value)}
          placeholder={props.editing ? "Leave blank to keep the current secret" : "JBSW Y3DP EHPK 3PXP"}
          rows={2}
          className="rounded border border-[var(--color-border)] bg-[var(--color-surface)] px-2 py-1.5 font-[var(--font-mono)] text-[13px] focus:border-[var(--color-accent)] focus:outline-none"
        />
        <span className="text-[10px] text-[var(--color-muted)]">
          {props.editing
            ? "Enter a new secret to replace it, or leave blank to keep the current one."
            : "Spaces and hyphens are stripped automatically; padding (=) is optional."}
        </span>
      </label>

      <button
        type="button"
        onClick={() => props.setAdvanced(!props.advanced)}
        className="self-start text-[11px] text-[var(--color-muted)] hover:text-[var(--color-accent)]"
      >
        {props.advanced ? "▾ Hide advanced" : "▸ Advanced (Digits / Period / Algorithm)"}
      </button>
      {props.advanced && (
        <div className="grid grid-cols-3 gap-3">
          <label className="flex flex-col gap-1 text-[12px]">
            <span className="font-semibold">Digits</span>
            <select
              value={props.digits}
              onChange={(e) => props.setDigits(Number(e.target.value))}
              className="rounded border border-[var(--color-border)] bg-[var(--color-surface)] px-2 py-1.5 text-[13px]"
            >
              <option value={6}>6</option>
              <option value={7}>7</option>
              <option value={8}>8</option>
            </select>
          </label>
          <label className="flex flex-col gap-1 text-[12px]">
            <span className="font-semibold">Period (s)</span>
            <select
              value={props.period}
              onChange={(e) => props.setPeriod(Number(e.target.value))}
              className="rounded border border-[var(--color-border)] bg-[var(--color-surface)] px-2 py-1.5 text-[13px]"
            >
              <option value={15}>15</option>
              <option value={30}>30</option>
              <option value={60}>60</option>
            </select>
          </label>
          <label className="flex flex-col gap-1 text-[12px]">
            <span className="font-semibold">Algorithm</span>
            <select
              value={props.algorithm}
              onChange={(e) => props.setAlgorithm(e.target.value)}
              className="rounded border border-[var(--color-border)] bg-[var(--color-surface)] px-2 py-1.5 text-[13px]"
            >
              <option value="SHA1">SHA1</option>
              <option value="SHA256">SHA256</option>
              <option value="SHA512">SHA512</option>
            </select>
          </label>
        </div>
      )}

      <button
        onClick={props.onSubmit}
        disabled={props.busy || !props.issuer.trim() || (!props.editing && !props.secret.trim())}
        className="mt-2 flex items-center justify-center gap-2 rounded bg-[var(--color-accent)] px-4 py-2 text-[13px] font-semibold text-[var(--color-accent-fg)] hover:opacity-90 disabled:opacity-40"
      >
        {props.editing ? <Check size={14} /> : <Plus size={14} />}
        {props.editing ? "Save changes" : "Add"}
      </button>
    </div>
  );
}

// ── Import / Export tab ─────────────────────────────────────────────

function ImportExportTab({
  importText,
  setImportText,
  onImport,
  onExport,
  entryCount,
  busy,
}: {
  importText: string;
  setImportText: (s: string) => void;
  onImport: () => void;
  onExport: () => void;
  entryCount: number;
  busy: boolean;
}) {
  const supportedFormats = useMemo(
    () => [
      "otpauth://totp/… (single QR code URI)",
      "otpauth-migration://offline?data=… (Google Authenticator bulk export)",
      "Aegis JSON (Android Aegis Authenticator, unencrypted)",
      "2FAS JSON (2FAS Auth)",
      "OTPManager JSON (macOS OTP Manager app export)",
      "Plain text: one otpauth:// URI per line",
    ],
    [],
  );

  return (
    <div className="mx-auto flex max-w-2xl flex-col gap-5">
      <section>
        <h3 className="mb-2 flex items-center gap-2 text-[13px] font-semibold">
          <Upload size={14} className="text-[var(--color-accent)]" />
          Import
        </h3>
        <p className="mb-2 text-[11px] text-[var(--color-muted)]">
          Paste below, or <strong>drag a file anywhere onto this window</strong> to import it
          (otpauth URIs · Aegis / 2FAS / OTPManager JSON · Google-Auth migration).
        </p>
        <textarea
          value={importText}
          onChange={(e) => setImportText(e.target.value)}
          placeholder="otpauth://totp/...  or paste JSON export here"
          rows={8}
          className="w-full rounded border border-[var(--color-border)] bg-[var(--color-surface)] px-3 py-2 font-[var(--font-mono)] text-[12px] focus:border-[var(--color-accent)] focus:outline-none"
        />
        <div className="mt-2 flex items-center gap-3">
          <button
            onClick={onImport}
            disabled={busy || !importText.trim()}
            className="flex items-center gap-2 rounded bg-[var(--color-accent)] px-3 py-1.5 text-[12px] font-semibold text-[var(--color-accent-fg)] hover:opacity-90 disabled:opacity-40"
          >
            <Upload size={12} />
            Import
          </button>
          <span className="text-[11px] text-[var(--color-muted)]">
            Format is detected automatically.
          </span>
        </div>
        <details className="mt-3 text-[11px] text-[var(--color-muted)]">
          <summary className="cursor-pointer hover:text-[var(--color-fg)]">
            Supported formats
          </summary>
          <ul className="mt-2 list-disc space-y-1 pl-5">
            {supportedFormats.map((f, i) => (
              <li key={i}>{f}</li>
            ))}
          </ul>
        </details>
      </section>

      <section>
        <h3 className="mb-2 flex items-center gap-2 text-[13px] font-semibold">
          <Download size={14} className="text-[var(--color-accent)]" />
          Export
        </h3>
        <p className="mb-2 text-[12px] text-[var(--color-muted)]">
          Exports all {entryCount} entries as a list of{" "}
          <code className="rounded bg-[var(--color-surface)] px-1 font-[var(--font-mono)] text-[11px]">
            otpauth://
          </code>
          {" "}URIs to the clipboard.
        </p>
        <div className="mb-3 rounded border border-amber-500/40 bg-amber-500/10 px-3 py-2 text-[11px]">
          <b>Caution:</b> Plaintext. The secrets are the long-term root
          of your 2FA — store the export encrypted (e.g. password
          manager, encrypted archive) and clear the clipboard afterwards.
        </div>
        <button
          onClick={onExport}
          disabled={busy || entryCount === 0}
          className="flex items-center gap-2 rounded border border-[var(--color-border)] px-3 py-1.5 text-[12px] hover:border-[var(--color-accent)] hover:text-[var(--color-accent)] disabled:opacity-40"
        >
          <Download size={12} />
          Export to clipboard
        </button>
      </section>
    </div>
  );
}
