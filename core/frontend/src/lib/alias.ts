/**
 * `alias` — guided shell-alias builder (v0.127.0). Pure string builders for the
 * AliasPanel: given an alias name + the command it should run, produce the
 * exact one-liner that creates the alias on each OS's default shell —
 * macOS → zsh (`~/.zshrc`), Linux → bash (`~/.bashrc`), Windows → PowerShell
 * (`$PROFILE`, as a `function` since `Set-Alias` can't carry arguments).
 *
 * Quoting is the load-bearing part (the adb lesson: single quotes have NO
 * backslash escaping in POSIX):
 *  - the alias VALUE is single-quoted with the close-reopen `'\''` pattern,
 *  - the display one-liner wraps it in double quotes for `printf`, escaping
 *    `\` `` ` `` `"` `$` — so a command containing any quote survives verbatim,
 *  - PowerShell single-quoted strings escape `'` by doubling it.
 */

export interface AliasSetup {
  /** Row key + label, e.g. "macos". */
  os: "macos" | "linux" | "windows";
  label: string;
  /** Where the alias lands (shown to the user). */
  target: string;
  /** The full one-liner to run in a terminal on that OS. */
  command: string;
}

/** Shell-safe alias names: no spaces, no `=`, nothing a shell would parse. */
export function validAliasName(name: string): boolean {
  return /^[A-Za-z_][A-Za-z0-9_-]*$/.test(name);
}

/** POSIX single-quote a string: close-reopen for embedded quotes (`'\''`). */
export function posixQuote(s: string): string {
  return `'${s.split("'").join("'\\''")}'`;
}

/** The rc-file line itself: `alias gs='git status'`. */
export function aliasLine(name: string, command: string): string {
  return `alias ${name}=${posixQuote(command)}`;
}

/** Escape for a POSIX double-quoted string (`printf '%s\n' "…"`). */
function dqEscape(s: string): string {
  return s.replace(/[\\`"$]/g, (c) => "\\" + c);
}

/** PowerShell profile line: a function, because `Set-Alias` can only point at
 *  a bare command — `function gs { git status $args }` forwards arguments. */
export function psFunction(name: string, command: string): string {
  return `function ${name} { ${command} $args }`;
}

/** PowerShell single-quoted string: `'` doubles. */
function psQuote(s: string): string {
  return `'${s.split("'").join("''")}'`;
}

/** The three per-OS one-liners for the panel. */
export function buildAliasSetups(name: string, command: string): AliasSetup[] {
  const line = aliasLine(name, command);
  const posix = (rc: string) => `printf '%s\\n' "${dqEscape(line)}" >> ~/${rc} && source ~/${rc}`;
  const ps = psFunction(name, command);
  return [
    { os: "macos", label: "macOS · zsh", target: "~/.zshrc", command: posix(".zshrc") },
    { os: "linux", label: "Linux · bash", target: "~/.bashrc", command: posix(".bashrc") },
    {
      os: "windows",
      label: "Windows · PowerShell",
      target: "$PROFILE",
      command: `if (!(Test-Path $PROFILE)) { New-Item -ItemType File -Force $PROFILE | Out-Null }; Add-Content $PROFILE ${psQuote(ps)}`,
    },
  ];
}

/** Search + sort for the management list: case-insensitive substring over
 *  name AND command, always alphabetical by name. Pure. */
export function filterAliases<T extends { name: string; command: string }>(
  list: readonly T[],
  term: string,
): T[] {
  const q = term.trim().toLowerCase();
  const hit = q
    ? list.filter((e) => e.name.toLowerCase().includes(q) || e.command.toLowerCase().includes(q))
    : [...list];
  return hit.sort((a, b) => a.name.localeCompare(b.name));
}
