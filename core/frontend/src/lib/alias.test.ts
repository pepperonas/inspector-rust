import { describe, it, expect } from "vitest";
import {
  validAliasName,
  posixQuote,
  aliasLine,
  psFunction,
  buildAliasSetups,
  filterAliases,
  chooseForm,
  definitionLine,
} from "./alias";

describe("validAliasName", () => {
  it("accepts shell-safe names and rejects everything a shell would parse", () => {
    expect(validAliasName("gs")).toBe(true);
    expect(validAliasName("git-st")).toBe(true);
    expect(validAliasName("_x2")).toBe(true);
    expect(validAliasName("")).toBe(false);
    expect(validAliasName("2fast")).toBe(false); // must not start with a digit
    expect(validAliasName("a b")).toBe(false);
    expect(validAliasName("a=b")).toBe(false);
    expect(validAliasName("rm;")).toBe(false);
    expect(validAliasName("a$b")).toBe(false);
  });
});

describe("quoting", () => {
  it("posixQuote survives embedded single quotes (close-reopen, the adb lesson)", () => {
    expect(posixQuote("git status")).toBe("'git status'");
    // `echo 'hi'` → `'echo '\''hi'\'''` — each ' becomes '\''
    expect(posixQuote("echo 'hi'")).toBe("'echo '\\''hi'\\'''");
  });

  it("aliasLine is the exact rc-file line", () => {
    expect(aliasLine("gs", "git status")).toBe("alias gs='git status'");
  });

  it("psFunction forwards arguments", () => {
    expect(psFunction("gs", "git status")).toBe("function gs { git status $args }");
  });
});

describe("buildAliasSetups", () => {
  it("produces one row per OS with the right targets", () => {
    const rows = buildAliasSetups("gs", "git status");
    expect(rows.map((r) => r.os)).toEqual(["macos", "linux", "windows"]);
    expect(rows[0].target).toBe("~/.zshrc");
    expect(rows[1].target).toBe("~/.bashrc");
    expect(rows[2].target).toBe("$PROFILE");
    // The POSIX one-liners append AND source, and carry the alias line intact.
    expect(rows[0].command).toContain(`"alias gs='git status'"`);
    expect(rows[0].command).toContain(">> ~/.zshrc && source ~/.zshrc");
    expect(rows[1].command).toContain(">> ~/.bashrc && source ~/.bashrc");
  });

  it("a command with double quotes and dollars survives the printf wrapper", () => {
    const rows = buildAliasSetups("greet", 'echo "hi $USER"');
    // Inside the printf double-quoted string, `"` and `$` must be escaped.
    expect(rows[0].command).toContain('\\"hi \\$USER\\"');
  });

  it("a command with single quotes survives BOTH the posix and the ps path", () => {
    const rows = buildAliasSetups("say", "echo 'hi'");
    // The printf payload sits in POSIX double quotes — expand the double-quote
    // escapes (`\\` `` \` `` `\"` `\$` → bare char, anything else stays) and
    // the result must be EXACTLY the rc-file line. Pinning the semantics, not
    // the escaped literal: both `\'` and `\\'` are valid spellings inside
    // double quotes, so a literal comparison would over-constrain the builder.
    const m = rows[0].command.match(/^printf '%s\\n' "(.*)" >> /);
    expect(m).not.toBeNull();
    const expanded = (m as RegExpMatchArray)[1].replace(/\\([\\`"$])/g, "$1");
    expect(expanded).toBe(aliasLine("say", "echo 'hi'"));
    expect(expanded).toBe("alias say='echo '\\''hi'\\'''");
    // PowerShell doubles the embedded single quotes.
    expect(rows[2].command).toContain("'function say { echo ''hi'' $args }'");
  });

  it("the windows one-liner creates $PROFILE if missing", () => {
    const rows = buildAliasSetups("gs", "git status");
    expect(rows[2].command).toMatch(/Test-Path \$PROFILE.*New-Item.*Add-Content \$PROFILE/);
  });
});

describe("filterAliases", () => {
  const list = [
    { name: "zz", command: "top" },
    { name: "gs", command: "git status" },
    { name: "gl", command: "git log" },
  ];

  it("sorts alphabetically by name and filters name OR command", () => {
    expect(filterAliases(list, "").map((e) => e.name)).toEqual(["gl", "gs", "zz"]);
    expect(filterAliases(list, "git").map((e) => e.name)).toEqual(["gl", "gs"]);
    expect(filterAliases(list, "STATUS").map((e) => e.name)).toEqual(["gs"]); // case-insensitive, command hit
    expect(filterAliases(list, "zz").map((e) => e.name)).toEqual(["zz"]);
    expect(filterAliases(list, "nope")).toEqual([]);
  });

  it("does not mutate the input", () => {
    const before = list.map((e) => e.name);
    filterAliases(list, "");
    expect(list.map((e) => e.name)).toEqual(before);
  });
});

describe("alias vs. function", () => {
  it("keeps a plain command an alias", () => {
    expect(chooseForm("git status")).toEqual({ kind: "alias" });
    expect(definitionLine("gs", "git status")).toBe("alias gs='git status'");
  });

  it("keeps a bare cd an alias", () => {
    // ⚠️ The load-bearing case. `work='cd ~/projects'` exists to LEAVE you
    // there — a subshell would make it do nothing at all.
    expect(chooseForm("cd ~/projects")).toEqual({ kind: "alias" });
  });

  it("makes cd-then-run a subshell function", () => {
    expect(chooseForm("cd ~/x && ./y")).toEqual({ kind: "function", reason: "changes-directory" });
    expect(definitionLine("bb", "cd ~/x && ./y")).toBe('bb() { ( cd ~/x && ./y "$@" ); }');
  });

  it("still forwards arguments after the conversion", () => {
    // An alias forwards trailing arguments for free; a function does not.
    expect(definitionLine("bb", "cd ~/x && ./y")).toContain('"$@"');
  });

  it("makes a command that reads $1 a function, without adding a second set", () => {
    expect(chooseForm("echo $1")).toEqual({ kind: "function", reason: "takes-arguments" });
    const line = definitionLine("c", 'git commit -m "$1"');
    expect(line).not.toContain('"$@"');
  });

  it("ignores a separator inside quotes and a non-positional $", () => {
    expect(chooseForm("echo 'a;b'")).toEqual({ kind: "alias" });
    expect(chooseForm("echo $HOME")).toEqual({ kind: "alias" });
  });

  it("gives PowerShell the Push-/Pop-Location equivalent of a subshell", () => {
    // A PS function runs in the CALLER's scope, so a bare `cd` inside one
    // strands the prompt — exactly the bug the POSIX subshell avoids.
    const ps = psFunction("bb", "cd C:/x; ./y", chooseForm("cd C:/x; ./y"));
    expect(ps).toContain("Push-Location");
    expect(ps).toContain("finally { Pop-Location }");
    expect(psFunction("gs", "git status", chooseForm("git status"))).not.toContain("Push-Location");
  });

  it("puts the function — not an alias line — into every OS one-liner", () => {
    const setups = buildAliasSetups("bb", "cd ~/x && ./y");
    const posix = setups.filter((s) => s.os !== "windows");
    expect(posix.length).toBe(2);
    for (const s of posix) {
      expect(s.command).toContain("bb() {");
      expect(s.command).not.toContain("alias bb=");
    }
  });
});
