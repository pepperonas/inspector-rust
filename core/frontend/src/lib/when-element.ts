/**
 * Call `found` with the element `id` — now if it exists, else as soon as it
 * appears under `root`, for at most `timeoutMs`. Returns a cancel function
 * (which also runs the cleanup `found` returned).
 *
 * For deep links into views whose parts render only after their state has
 * loaded: looking the target up once, at mount, finds nothing.
 */
export function whenElement(
  id: string,
  root: Node,
  timeoutMs: number,
  found: (el: HTMLElement) => void | (() => void),
): () => void {
  let cleanup: void | (() => void);
  const now = document.getElementById(id);
  if (now) {
    cleanup = found(now);
    return () => cleanup?.();
  }
  const mo = new MutationObserver(() => {
    const el = document.getElementById(id);
    if (!el) return;
    mo.disconnect();
    clearTimeout(timer);
    cleanup = found(el);
  });
  mo.observe(root, { childList: true, subtree: true });
  const timer = setTimeout(() => mo.disconnect(), timeoutMs);
  return () => {
    mo.disconnect();
    clearTimeout(timer);
    cleanup?.();
  };
}
