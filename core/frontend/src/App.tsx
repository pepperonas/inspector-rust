import { useEffect, useMemo, useRef, useState } from "react";
import { getVersion } from "@tauri-apps/api/app";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { Footer } from "./components/Footer";
import { HistoryList } from "./components/HistoryList";
import { NotesPanel } from "./components/NotesPanel";
import { PreviewPanel } from "./components/PreviewPanel";
import { SearchBar } from "./components/SearchBar";
import { SettingsPanel } from "./components/SettingsPanel";
import { SnippetsPanel } from "./components/SnippetsPanel";
import { useClipboardHistory } from "./hooks/useClipboardHistory";
import { useFuzzySearch } from "./hooks/useFuzzySearch";
import { useKeyboardNav } from "./hooks/useKeyboardNav";
import { useNotes } from "./hooks/useNotes";
import { useSnippets } from "./hooks/useSnippets";
import { tryEvaluate } from "./lib/calc";
import {
  clearHistory,
  deleteEntry,
  findSnippets,
  hidePopup,
  pasteEntry,
  pasteSnippet,
  pasteText,
  saveClipAsNote,
} from "./lib/ipc";
import type { ListEntry, Snippet } from "./lib/types";

type Tab = "history" | "snippets" | "notes" | "settings";

function App() {
  const { entries, refresh: refreshHistory } = useClipboardHistory();
  const { snippets, refresh: refreshSnippets } = useSnippets();
  const { notes, categories: noteCategories, refresh: refreshNotes } = useNotes();
  const [query, setQuery] = useState("");
  const [selected, setSelected] = useState(0);
  const [activeTab, setActiveTab] = useState<Tab>("history");
  const [matchingSnippets, setMatchingSnippets] = useState<Snippet[]>([]);
  const [version, setVersion] = useState<string | undefined>(undefined);
  const searchRef = useRef<HTMLInputElement>(null);

  // Pulled once from tauri.conf.json via the core:app permission set.
  // Failure (e.g. browser dev preview without Tauri context) is silent —
  // the footer just hides the version chip.
  useEffect(() => {
    getVersion().then(setVersion).catch(() => undefined);
  }, []);

  const filteredClips = useFuzzySearch(entries, query);

  // Inline calculator: when the query parses as a math expression with at
  // least one operator/function/constant, surface the result as the top
  // list item (Alfred-style).
  const calcResult = useMemo(() => tryEvaluate(query), [query]);

  // Combine: calc result first, then snippet matches, then history clips.
  const combined: ListEntry[] = [
    ...(calcResult ? [{ kind: "calc", data: calcResult } as ListEntry] : []),
    ...matchingSnippets.map((s): ListEntry => ({ kind: "snippet", data: s })),
    ...filteredClips.map((c): ListEntry => ({ kind: "clip", data: c })),
  ];

  // Find matching snippets whenever query changes.
  useEffect(() => {
    if (!query.trim()) {
      setMatchingSnippets([]);
      return;
    }
    findSnippets(query)
      .then(setMatchingSnippets)
      .catch(() => setMatchingSnippets([]));
  }, [query]);

  useEffect(() => {
    setSelected(0);
  }, [query, entries.length]);

  // Handle window-shown (hotkey): reset to history tab.
  useEffect(() => {
    let unlisten: UnlistenFn | undefined;
    (async () => {
      unlisten = await listen("window-shown", () => {
        setActiveTab("history");
        setQuery("");
        setSelected(0);
        requestAnimationFrame(() => {
          searchRef.current?.focus();
          searchRef.current?.select();
        });
      });
    })();
    return () => unlisten?.();
  }, []);

  // Handle tray "Manage Snippets": switch to snippets tab.
  useEffect(() => {
    let unlisten: UnlistenFn | undefined;
    (async () => {
      unlisten = await listen("open-snippets-tab", () => {
        setActiveTab("snippets");
        void refreshSnippets();
      });
    })();
    return () => unlisten?.();
  }, [refreshSnippets]);

  // Handle tray "Manage Notes": switch to notes tab.
  useEffect(() => {
    let unlisten: UnlistenFn | undefined;
    (async () => {
      unlisten = await listen("open-notes-tab", () => {
        setActiveTab("notes");
        void refreshNotes();
      });
    })();
    return () => unlisten?.();
  }, [refreshNotes]);

  const activate = async (i: number) => {
    const target = combined[i];
    if (!target) return;
    try {
      if (target.kind === "snippet") {
        await pasteSnippet(target.data.id);
      } else if (target.kind === "calc") {
        await pasteText(target.data.display);
      } else {
        await pasteEntry(target.data.id);
      }
    } catch (e) {
      console.error("paste failed", e);
    }
  };

  const onSaveAsNote = async (i: number) => {
    const target = combined[i];
    if (!target || target.kind !== "clip") return;
    try {
      await saveClipAsNote(target.data.id, "", "");
      await refreshNotes();
    } catch (e) {
      console.error("save as note failed", e);
    }
  };

  const onDeleteClip = async (i: number) => {
    const target = combined[i];
    if (!target || target.kind !== "clip") return;
    try {
      await deleteEntry(target.data.id);
      await refreshHistory();
    } catch (e) {
      console.error("delete entry failed", e);
    }
  };

  const onClearAllHistory = async () => {
    try {
      await clearHistory();
      await refreshHistory();
    } catch (e) {
      console.error("clear history failed", e);
    }
  };

  useKeyboardNav({
    length: combined.length,
    selected,
    setSelected,
    onEnter: () => void activate(selected),
    onEscape: () => {
      void hidePopup();
    },
  });

  const current = combined[selected] ?? null;

  return (
    <div className="flex h-screen w-screen p-2">
      <div className="app-shell fade-in flex h-full w-full flex-col">

        {/* Header — fixed height, tab buttons anchored top-right */}
        <div className="relative shrink-0">
          {activeTab === "history" ? (
            <SearchBar
              ref={searchRef}
              value={query}
              onChange={setQuery}
              calcMode={calcResult !== null}
            />
          ) : (
            <div className="flex h-14 items-center border-b border-[var(--color-border)] pl-4 pr-[260px]">
              <span className="text-[15px] font-semibold">
                {activeTab === "snippets"
                  ? "Snippets"
                  : activeTab === "notes"
                    ? "Notes"
                    : "Settings"}
              </span>
            </div>
          )}
          <div className="absolute right-3 top-1/2 flex -translate-y-1/2 gap-1">
            <TabButton active={activeTab === "history"} onClick={() => setActiveTab("history")}>
              History
            </TabButton>
            <TabButton active={activeTab === "snippets"} onClick={() => {
              setActiveTab("snippets");
              void refreshSnippets();
            }}>
              Snippets
            </TabButton>
            <TabButton active={activeTab === "notes"} onClick={() => {
              setActiveTab("notes");
              void refreshNotes();
            }}>
              Notes
            </TabButton>
            <TabButton active={activeTab === "settings"} onClick={() => setActiveTab("settings")}>
              Settings
            </TabButton>
          </div>
        </div>

        {/* Content */}
        {activeTab === "history" ? (
          <div className="flex min-h-0 flex-1">
            <div className="w-2/5 border-r border-[var(--color-border)]">
              <HistoryList
                entries={combined}
                selectedIndex={selected}
                onSelect={setSelected}
                onActivate={activate}
                onSaveAsNote={onSaveAsNote}
                onDeleteClip={onDeleteClip}
                onClearAll={onClearAllHistory}
              />
            </div>
            <div className="w-3/5 min-w-0">
              <PreviewPanel entry={current} />
            </div>
          </div>
        ) : activeTab === "snippets" ? (
          <SnippetsPanel snippets={snippets} onRefresh={refreshSnippets} />
        ) : activeTab === "notes" ? (
          <NotesPanel notes={notes} categories={noteCategories} onRefresh={refreshNotes} />
        ) : (
          <SettingsPanel
            onBackupImported={async () => {
              // After a Backup → Import, refresh every list that might
              // have new rows. History reloads itself via the
              // `clipboard-changed` event the watcher emits, but Notes
              // and Snippets need an explicit nudge.
              await Promise.all([refreshHistory(), refreshSnippets(), refreshNotes()]);
            }}
          />
        )}

        <Footer
          index={selected}
          total={
            activeTab === "history"
              ? combined.length
              : activeTab === "snippets"
                ? snippets.length
                : activeTab === "notes"
                  ? notes.length
                  : 0
          }
          version={version}
        />
      </div>
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
        "rounded px-2 py-1 text-[11px] font-medium transition-colors whitespace-nowrap " +
        (active
          ? "bg-[var(--color-accent)] text-[var(--color-accent-fg)]"
          : "text-[var(--color-muted)] hover:bg-[var(--color-surface)] hover:text-[var(--color-fg)]")
      }
    >
      {children}
    </button>
  );
}

export default App;
