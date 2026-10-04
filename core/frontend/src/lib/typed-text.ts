import type { ListEntry } from "./types";

/**
 * The typed text as its own row (2026-10-04). When the query names an app
 * (`BRIDGE` → "Code Bridge"), the list used to offer only "launch the app" —
 * the string itself could not be transformed, because the string-manipulation
 * bar only exists for clipboard rows. This row sits DIRECTLY BELOW the app hit:
 * launching stays the first choice, the text is one ↓ away, its preview opens
 * the transform bar and Enter pastes the text as typed.
 *
 * Only together with an app hit — that is the case where the typed text has no
 * row of its own; without an app the clips matching the text are the result.
 */
export function typedTextEntry(query: string, hasAppEntry: boolean): ListEntry | null {
  if (!hasAppEntry) return null;
  const text = query.trim();
  if (!text) return null;
  return { kind: "typed-text", data: { text } };
}
