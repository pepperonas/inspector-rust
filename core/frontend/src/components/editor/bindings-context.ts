import { createContext, useCallback, useContext } from "react";
import { DEFAULT_BINDINGS, formatBinding, type Binding, type Bindings } from "../../lib/editor-shortcuts";
import { useT } from "../../lib/i18n";

/** The editor's current key map, for tooltips deep in the inspector. */
export const BindingsContext = createContext<Bindings>(DEFAULT_BINDINGS);

export function useBindings(): Bindings {
  return useContext(BindingsContext);
}

/**
 * The one way to show a key: "⌘D", "Space" (localized), or "not assigned" for an action that
 * lost its key. Tooltips, lane hints and the shortcuts dialog all use it.
 */
export function useKeyLabel(): (b: Binding | null) => string {
  const t = useT();
  return useCallback((b: Binding | null) => (b ? formatBinding(b, t("rec4.keys.space")) : t("rec4.keys.unassigned")), [t]);
}

/** A tooltip with its key in brackets; an unassigned action shows the text alone. */
export function useKeyTitle(): (text: string, b: Binding | null) => string {
  const label = useKeyLabel();
  return useCallback((text: string, b: Binding | null) => (b ? `${text} (${label(b)})` : text), [label]);
}
