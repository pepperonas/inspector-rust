/**
 * Pure kill-picker matching — filter the process snapshot by a name/exe
 * substring (or exact PID) and float an exact PID hit to the top.
 *
 * Multi-word patterns match when EVERY whitespace token appears in the
 * combined name+exe (case-insensitive), so `kill inspector rust` hits
 * `InspectorRust` / `inspector-rust` even without a literal space.
 */

export type KillProcess = {
  pid: number;
  name: string;
  memory_mb: number;
  exe: string;
};

/** Cap visible rows — refine the pattern beyond this. */
export const KILL_LIST_CAP = 50;

/**
 * Filter + order processes for the kill picker.
 * Empty `pattern` → full list (caller still applies {@link KILL_LIST_CAP}).
 */
export function filterKillProcesses<T extends KillProcess>(
  processes: readonly T[],
  pattern: string,
): T[] {
  const trimmed = pattern.trim();
  if (!trimmed) return [...processes];

  const pidQuery = /^\d+$/.test(trimmed) ? Number(trimmed) : null;
  const tokens = trimmed.toLowerCase().split(/\s+/).filter(Boolean);

  const filtered = processes.filter((p) => {
    if (pidQuery !== null && p.pid === pidQuery) return true;
    const hay = `${p.name} ${p.exe}`.toLowerCase();
    return tokens.every((t) => hay.includes(t));
  });

  if (pidQuery === null) return filtered;
  return filtered.sort(
    (a, b) => Number(b.pid === pidQuery) - Number(a.pid === pidQuery),
  );
}
