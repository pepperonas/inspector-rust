// `gestures` argument parsing — kept tiny and separate so App.tsx can use it
// without pulling the panel's helpers into the start-up chunk.

/** `gestures`, `gestures on`, `gestures off`. Anything else is unknown, never
 *  a silent toggle — a typo must not switch gestures off. */
export type GesturesArg = "panel" | "on" | "off" | "unknown";

export function parseGesturesArg(arg: string): GesturesArg {
  const a = arg.trim().toLowerCase();
  if (a === "") return "panel";
  if (a === "on" || a === "an" || a === "1") return "on";
  if (a === "off" || a === "aus" || a === "0") return "off";
  return "unknown";
}
