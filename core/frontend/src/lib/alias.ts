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

/** Why a definition has to be a shell FUNCTION instead of an alias. */
export type FnReason = "changes-directory" | "takes-arguments";
export type Form = { kind: "alias" } | { kind: "function"; reason: FnReason };

/** Split a command at top-level `;` `&&` `||` `|`, ignoring quoted regions. */
function segments(command: string): string[] {
  const out: string[] = [];
  let cur = "";
  let quote: string | null = null;
  for (let i = 0; i < command.length; ) {
    const c = command[i];
    if (quote) {
      if (c === quote) quote = null;
      cur += c;
      i += 1;
    } else if (c === "'" || c === '"') {
      quote = c;
      cur += c;
      i += 1;
    } else if (c === ";" || c === "|" || c === "&") {
      const two = command[i + 1] === c;
      out.push(cur);
      cur = "";
      i += two ? 2 : 1;
    } else {
      cur += c;
      i += 1;
    }
  }
  out.push(cur);
  return out.map((s) => s.trim()).filter(Boolean);
}

/** Does the command reference `$1`…`$9`, `$@` or `$*`? */
function usesPositional(command: string): boolean {
  for (let i = 0; i < command.length; i++) {
    if (command[i] !== "$") continue;
    let j = i + 1;
    if (command[j] === "{") j += 1;
    const n = command[j];
    if (n && (/[0-9]/.test(n) || n === "@" || n === "*")) return true;
  }
  return false;
}

/**
 * Alias or function — mirrors `shell_alias::choose_form` in the Rust core;
 * a cross-language test pins the two against each other.
 *
 * ⚠️ **`cd` ALONE stays an alias.** `work='cd ~/projects'` exists to LEAVE you
 * there; a subshell would make it do nothing. Only a `cd` with something after
 * it wants one — you go there to run a thing and expect your shell to stay put.
 */
export function chooseForm(command: string): Form {
  if (usesPositional(command)) return { kind: "function", reason: "takes-arguments" };
  const segs = segments(command);
  const at = segs.findIndex((s) => s === "cd" || s.startsWith("cd "));
  return at >= 0 && at + 1 < segs.length
    ? { kind: "function", reason: "changes-directory" }
    : { kind: "alias" };
}

/** One-line POSIX function. `"$@"` is appended for the directory case because
 *  an alias forwards trailing arguments for free and a function does not. */
export function functionLine(name: string, command: string, reason: FnReason): string {
  return reason === "changes-directory"
    ? `${name}() { ( ${command} "$@" ); }`
    : `${name}() { ${command}; }`;
}

/** The rc line for this command — alias or function, as it needs. */
export function definitionLine(name: string, command: string): string {
  const f = chooseForm(command);
  return f.kind === "alias" ? aliasLine(name, command) : functionLine(name, command, f.reason);
}

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
export function psFunction(name: string, command: string, form?: Form): string {
  // ⚠️ A PowerShell function runs in the CALLER's scope, so a bare `cd` inside
  // one strands the user's prompt in the target directory — the very bug the
  // POSIX subshell avoids. `Push-Location` with no argument saves where you
  // are; `finally` puts you back even if the command fails. No parsing of the
  // command needed.
  if (form?.kind === "function" && form.reason === "changes-directory") {
    return `function ${name} { Push-Location; try { ${command} @args } finally { Pop-Location } }`;
  }
  return `function ${name} { ${command} $args }`;
}

/** PowerShell single-quoted string: `'` doubles. */
function psQuote(s: string): string {
  return `'${s.split("'").join("''")}'`;
}

/** The three per-OS one-liners for the panel. */
export function buildAliasSetups(name: string, command: string): AliasSetup[] {
  const line = definitionLine(name, command);
  const posix = (rc: string) => `printf '%s\\n' "${dqEscape(line)}" >> ~/${rc} && source ~/${rc}`;
  const ps = psFunction(name, command, chooseForm(command));
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
