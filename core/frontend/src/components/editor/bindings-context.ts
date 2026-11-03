import { createContext, useContext } from "react";
import { DEFAULT_BINDINGS, formatBinding, type Bindings } from "../../lib/editor-shortcuts";
import { useT } from "../../lib/i18n";

/** The editor's current key map, for tooltips deep in the inspector. */
export const BindingsContext = createContext<Bindings>(DEFAULT_BINDINGS);

/** The key label of an action in the current map ("⌘D", "Space"). */
export function useActionKey(): (action: keyof Bindings) => string {
  const bindings = useContext(BindingsContext);
  const t = useT();
  return (action) => formatBinding(bindings[action], t("rec4.keys.space"));
}
