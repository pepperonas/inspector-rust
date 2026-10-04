// One-line preview of a copied result for the "copied" feedback (snackbar in
// the preview's transform bar, status toast for the clipboard commands).
// Deliberately free of imports: App.tsx uses it, and anything it pulls in
// lands in the start-up bundle.

/** Longest preview shown in the copy feedback. */
export const FEEDBACK_PREVIEW_MAX = 40;

/**
 * Pure: the one-line preview of a transformed result for the "copied"
 * feedback — whitespace collapsed (a multi-line result must not blow up a
 * one-line pill or toast), cut at {@link FEEDBACK_PREVIEW_MAX} characters with
 * an ellipsis, code points kept intact (no half emoji). Empty → "(leer)" so the
 * user sees that an empty string really was copied.
 */
export function feedbackPreview(result: string, max = FEEDBACK_PREVIEW_MAX): string {
  const flat = result.replace(/\s+/g, " ").trim();
  if (!flat) return "(leer)";
  const chars = [...flat];
  return chars.length <= max ? flat : `${chars.slice(0, max - 1).join("").trimEnd()}…`;
}

