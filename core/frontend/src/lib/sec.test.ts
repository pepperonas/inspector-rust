import { describe, it, expect } from "vitest";
import {
  posixQuote,
  buildCommand,
  parseSecCommand,
  requiredKeys,
  visiblePresets,
  matchPresets,
  findTool,
  presetFlagHelp,
  type SecCatalog,
  type SecDefaults,
  type ToolSpec,
  type Segment,
} from "./sec";

// A small catalogue mirroring the real Rust registry shape for the tools/
// presets exercised here (segments identical to sec/registry.rs).
const lit = (text: string): Segment => ({ kind: "lit", text });
const field = (key: string): Segment => ({ kind: "field", key });
const flag = (f: string, key: string): Segment => ({ kind: "flag", flag: f, key });
const joined = (prefix: string, key: string): Segment => ({ kind: "joined", prefix, key });

const NMAP: ToolSpec = {
  name: "nmap",
  aliases: ["scan"],
  binary: "nmap",
  fields: [
    { key: "target", label: "Target", placeholder: "host", help: "", required: true },
    { key: "ports", label: "Ports", placeholder: "", help: "", required: false },
    { key: "timing", label: "Timing", placeholder: "", help: "", required: false },
    { key: "output", label: "Output", placeholder: "", help: "", required: false },
  ],
  flag_help: [
    ["-sV", "Service/version detection"],
    ["-sC", "Default NSE scripts"],
    ["-p-", "All 65535 TCP ports"],
    ["-p", "Ports"],
    ["-T", "Timing"],
    ["-oA", "Output base"],
  ],
  notes: [],
  presets: [
    {
      name: "service", aliases: ["vers"], purpose: "Version + default scripts",
      segments: [lit("nmap"), lit("-sV"), lit("-sC"), flag("-p", "ports"), joined("-T", "timing"), flag("-oA", "output"), field("target")],
      fields: ["target", "ports", "timing", "output"], sharp: false, tags: [], category: "scan",
    },
    {
      name: "full-tcp", aliases: ["full"], purpose: "All TCP ports",
      segments: [lit("nmap"), lit("-p-"), flag("-p", "ports"), joined("-T", "timing"), flag("-oA", "output"), field("target")],
      fields: ["target", "ports", "timing", "output"], sharp: true, tags: ["long-running"], category: "scan",
    },
  ],
};

const FEROX: ToolSpec = {
  name: "feroxbuster",
  aliases: ["ferox"],
  binary: "feroxbuster",
  fields: [
    { key: "url", label: "URL", placeholder: "", help: "", required: true },
    { key: "wordlist", label: "Wordlist", placeholder: "", help: "", required: true },
  ],
  flag_help: [["-u", "URL"], ["-w", "Wordlist"]],
  notes: [],
  presets: [
    {
      name: "dir", aliases: ["d"], purpose: "Content discovery",
      segments: [lit("feroxbuster"), flag("-u", "url"), flag("-w", "wordlist")],
      fields: ["url", "wordlist"], sharp: false, tags: [], category: "content",
    },
  ],
};

const JOHN: ToolSpec = {
  name: "john",
  aliases: ["jtr"],
  binary: "john",
  fields: [
    { key: "hashfile", label: "Hash file", placeholder: "", help: "", required: true },
    { key: "wordlist", label: "Wordlist", placeholder: "", help: "", required: false },
    { key: "format", label: "Format", placeholder: "", help: "", required: false },
    { key: "mask", label: "Mask", placeholder: "", help: "", required: true },
    { key: "archive", label: "File", placeholder: "", help: "", required: true },
  ],
  flag_help: [["--wordlist=", "Dictionary"], ["--incremental", "Brute force"], ["--mask=", "Mask"], ["--format=", "Format"]],
  notes: [],
  presets: [
    {
      name: "wordlist", aliases: ["dict"], purpose: "Dictionary attack",
      segments: [lit("john"), joined("--wordlist=", "wordlist"), joined("--format=", "format"), field("hashfile")],
      fields: ["hashfile", "wordlist", "format"], sharp: false, tags: [], category: "crack",
    },
    {
      name: "incremental", aliases: ["brute"], purpose: "Brute force",
      segments: [lit("john"), lit("--incremental"), joined("--format=", "format"), field("hashfile")],
      fields: ["hashfile", "format"], sharp: true, tags: ["long-running"], category: "crack",
    },
    {
      name: "mask", aliases: [], purpose: "Mask (Jumbo)",
      segments: [lit("john"), joined("--mask=", "mask"), field("hashfile")],
      fields: ["hashfile", "mask"], sharp: false, tags: ["jumbo-only"], category: "crack",
    },
    {
      name: "zip2john", aliases: ["zip"], purpose: "Extract ZIP hash",
      segments: [lit("zip2john"), field("archive")],
      fields: ["archive"], sharp: false, tags: ["jumbo-only", "prepare"], category: "prepare",
    },
  ],
};

const CAT: SecCatalog = { tools: [NMAP, FEROX, JOHN], common_wordlists: [], john_formats: [{ name: "nt", jumbo: true }, { name: "bcrypt", jumbo: false }] };

const DEF: SecDefaults = {
  wordlist: "",
  output_dir: "",
  timing: "",
  threads: 0,
  rate: 0,
  john_line: "jumbo",
  terminal: "iterm",
  auto_enter: false,
  scope_note: "",
  save_history: true,
};

// ── The test that matters: shell quoting can never break a command ───────────

describe("posixQuote — injection hardening", () => {
  it("leaves safe values unquoted", () => {
    expect(posixQuote("10.0.0.5")).toBe("10.0.0.5");
    expect(posixQuote("http://host:8080/path")).toBe("http://host:8080/path");
    expect(posixQuote("php,html,txt")).toBe("php,html,txt");
    expect(posixQuote("/usr/share/wl.txt")).toBe("/usr/share/wl.txt");
  });

  it("quotes URLs whose query string contains shell globs (? &)", () => {
    // `?` and `&` are shell metacharacters — quoting keeps zsh from erroring.
    expect(posixQuote("http://h/a?id=1")).toBe("'http://h/a?id=1'");
    expect(posixQuote("http://h/a?id=1&x=2")).toBe("'http://h/a?id=1&x=2'");
  });

  it("wraps and neutralises shell metacharacters", () => {
    expect(posixQuote("a; rm -rf /")).toBe("'a; rm -rf /'");
    expect(posixQuote("10.0.0.1 && curl evil")).toBe("'10.0.0.1 && curl evil'");
    expect(posixQuote("$(id)")).toBe("'$(id)'");
    expect(posixQuote("`whoami`")).toBe("'`whoami`'");
    expect(posixQuote('a"b')).toBe("'a\"b'");
    expect(posixQuote("a b")).toBe("'a b'");
    expect(posixQuote("line1\nline2")).toBe("'line1\nline2'");
  });

  it("escapes single quotes the POSIX way ('\\'')", () => {
    expect(posixQuote("O'Brien")).toBe("'O'\\''Brien'");
    expect(posixQuote("'")).toBe("''\\'''");
  });

  it("empty → ''", () => {
    expect(posixQuote("")).toBe("''");
  });

  it("a malicious target stays exactly ONE shell token", () => {
    // The whole point: the injection is contained in a single quoted argument.
    for (const evil of ["a; rm -rf /", "x && y", "$(id)", "a b c", "'; drop"]) {
      const r = parseSecCommand("nmap", `service ${evil}`, CAT, DEF);
      expect(r.kind).toBe("built");
      if (r.kind === "built") {
        // The dangerous chars are inside quotes; the command's own structure
        // (nmap -sV -sC …) is intact and the value is the final quoted token.
        expect(r.command.startsWith("nmap -sV -sC ")).toBe(true);
        const quoted = posixQuote(evil.split(/\s+/)[0]); // only first token becomes target
        expect(r.command).toContain(quoted);
      }
    }
  });
});

// ── Command builder — reference commands ─────────────────────────────────────

describe("buildCommand — reference commands", () => {
  it("nmap service 10.0.0.5 → nmap -sV -sC 10.0.0.5 (DoD)", () => {
    const p = NMAP.presets[0];
    const b = buildCommand(p, { target: "10.0.0.5" }, DEF, requiredKeys(NMAP));
    expect(b.command).toBe("nmap -sV -sC 10.0.0.5");
    expect(b.missing).toEqual([]);
  });

  it("a target with special chars is quoted (DoD)", () => {
    const b = buildCommand(NMAP.presets[0], { target: "a b; c" }, DEF, requiredKeys(NMAP));
    expect(b.command).toBe("nmap -sV -sC 'a b; c'");
  });

  it("optional fields appear only when set; default timing does not inject", () => {
    // No -T with the default (empty) timing.
    const bare = buildCommand(NMAP.presets[0], { target: "h" }, DEF, requiredKeys(NMAP));
    expect(bare.command).toBe("nmap -sV -sC h");
    // Setting timing/ports adds them.
    const full = buildCommand(
      NMAP.presets[0],
      { target: "h", ports: "1-1000", timing: "4", output: "scan" },
      DEF,
      requiredKeys(NMAP),
    );
    expect(full.command).toBe("nmap -sV -sC -p 1-1000 -T4 -oA scan h");
  });

  it("required flag with a missing value shows a ‹key› placeholder", () => {
    const b = buildCommand(FEROX.presets[0], { wordlist: "/wl" }, DEF, requiredKeys(FEROX));
    expect(b.command).toBe("feroxbuster -u ‹url› -w /wl");
    expect(b.missing).toContain("url");
  });

  it("john wordlist uses the Settings wordlist (DoD)", () => {
    const withDefault: SecDefaults = { ...DEF, wordlist: "/usr/share/rockyou.txt" };
    const b = buildCommand(JOHN.presets[0], { hashfile: "hashes.txt" }, withDefault, requiredKeys(JOHN));
    expect(b.command).toBe("john --wordlist=/usr/share/rockyou.txt hashes.txt");
  });
});

// ── Parser ───────────────────────────────────────────────────────────────────

describe("parseSecCommand", () => {
  it("bare `sec` → tool overview", () => {
    expect(parseSecCommand("sec", "", CAT, DEF)).toEqual({ kind: "tool-overview" });
  });

  it("`nmap` / `sec nmap` → preset list", () => {
    const a = parseSecCommand("nmap", "", CAT, DEF);
    const b = parseSecCommand("sec", "nmap", CAT, DEF);
    expect(a.kind).toBe("preset-list");
    expect(b.kind).toBe("preset-list");
    if (a.kind === "preset-list") expect(a.tool.name).toBe("nmap");
  });

  it("tool alias resolves (ferox → feroxbuster)", () => {
    const r = parseSecCommand("ferox", "dir http://h /wl", CAT, DEF);
    expect(r.kind).toBe("built");
    if (r.kind === "built") {
      expect(r.tool.name).toBe("feroxbuster");
      expect(r.command).toBe("feroxbuster -u http://h -w /wl");
    }
  });

  it("argument order is irrelevant", () => {
    const a = parseSecCommand("nmap", "service 10.0.0.5 --ports 80", CAT, DEF);
    const b = parseSecCommand("nmap", "service --ports 80 10.0.0.5", CAT, DEF);
    expect(a.kind === "built" && a.command).toBe("nmap -sV -sC -p 80 10.0.0.5");
    expect(b.kind === "built" && b.command).toBe("nmap -sV -sC -p 80 10.0.0.5");
  });

  it("--key=value sets an optional field", () => {
    const r = parseSecCommand("nmap", "service 10.0.0.5 --timing=5", CAT, DEF);
    expect(r.kind === "built" && r.command).toBe("nmap -sV -sC -T5 10.0.0.5");
  });

  it("a bare --flag (no value following) sets its field to 'true'", () => {
    // Trailing bare flag …
    const a = parseSecCommand("nmap", "service 10.0.0.5 --ports", CAT, DEF);
    expect(a.kind === "built" && a.command).toBe("nmap -sV -sC -p true 10.0.0.5");
    // … and a bare flag directly followed by ANOTHER flag must not eat it.
    const b = parseSecCommand("nmap", "service 10.0.0.5 --ports --timing=4", CAT, DEF);
    expect(b.kind === "built" && b.command).toBe("nmap -sV -sC -p true -T4 10.0.0.5");
  });

  it("surplus positional tokens are dropped, never crash the builder", () => {
    // Positional slots are the preset's REQUIRED fields only (service: just
    // `target`); optional fields are flag-only. Extra bare tokens therefore
    // have no slot — they must vanish silently, not corrupt the command.
    const r = parseSecCommand("nmap", "service a b c d", CAT, DEF);
    expect(r.kind).toBe("built");
    if (r.kind === "built") {
      expect(r.command).toBe("nmap -sV -sC a");
    }
  });

  it("missing target → placeholder, no crash", () => {
    const r = parseSecCommand("nmap", "service", CAT, DEF);
    expect(r.kind).toBe("built");
    if (r.kind === "built") {
      expect(r.command).toBe("nmap -sV -sC ‹target›");
      expect(r.missing).toContain("target");
    }
  });

  it("sharp preset flags the confirmation state", () => {
    const dump = parseSecCommand("nmap", "full-tcp 10.0.0.5", CAT, DEF);
    expect(dump.kind === "built" && dump.sharp).toBe(true);
    const inc = parseSecCommand("john", "incremental hashes.txt", CAT, DEF);
    expect(inc.kind === "built" && inc.sharp).toBe(true);
  });

  it("preview cheat-sheet only lists flags the preset uses", () => {
    const r = parseSecCommand("nmap", "service 10.0.0.5", CAT, DEF);
    if (r.kind === "built") {
      const flags = r.flagHelp.map(([f]) => f);
      expect(flags).toContain("-sV");
      expect(flags).toContain("-sC");
      expect(flags).not.toContain("-p-"); // full-tcp's flag, not service's
    }
  });

  it("does not fire on prose after a bare tool keyword ('nmap output parsen')", () => {
    expect(parseSecCommand("nmap", "output parsen", CAT, DEF).kind).toBe("not-command");
    expect(parseSecCommand("nmap", "10.0.0.5", CAT, DEF).kind).toBe("not-command");
    // But a partial preset IS clear intent.
    expect(parseSecCommand("nmap", "serv", CAT, DEF).kind).toBe("preset-list");
    // `sec …` is always an explicit command.
    expect(parseSecCommand("sec", "nmap output parsen", CAT, DEF).kind).toBe("preset-list");
  });

  it("unknown tool → suggestion", () => {
    expect(parseSecCommand("sec", "bogus", CAT, DEF).kind).toBe("suggestion");
  });

  it("`john prepare` shows the *2john helpers", () => {
    const r = parseSecCommand("john", "prepare", CAT, DEF);
    expect(r.kind).toBe("preset-list");
    if (r.kind === "preset-list") {
      const vis = visiblePresets(r.tool, DEF, true).map((p) => p.name);
      expect(vis).toContain("zip2john");
      expect(vis).not.toContain("wordlist"); // crack presets excluded from prepare
    }
  });
});

describe("John Core/Jumbo gating + fuzzy", () => {
  it("Core hides Jumbo-only presets (mask, *2john)", () => {
    const jumbo = visiblePresets(JOHN, { ...DEF, john_line: "jumbo" }, false).map((p) => p.name);
    const core = visiblePresets(JOHN, { ...DEF, john_line: "core" }, false).map((p) => p.name);
    expect(jumbo).toContain("mask");
    expect(core).not.toContain("mask");
    expect(core).toContain("wordlist");
  });

  it("fuzzy finds presets over alias/description", () => {
    expect(matchPresets("vers", NMAP.presets).map((p) => p.name)).toContain("service");
    expect(matchPresets("brute", JOHN.presets).map((p) => p.name)).toContain("incremental");
  });
});

describe("findTool", () => {
  it("resolves by exact name, case-insensitively", () => {
    expect(findTool("nmap", CAT)?.name).toBe("nmap");
    expect(findTool("NMAP", CAT)?.name).toBe("nmap");
    expect(findTool("  John  ".trim(), CAT)?.name).toBe("john");
  });

  it("resolves by alias when the name misses", () => {
    expect(findTool("scan", CAT)?.name).toBe("nmap");
    expect(findTool("ferox", CAT)?.name).toBe("feroxbuster");
    expect(findTool("jtr", CAT)?.name).toBe("john");
  });

  it("prefers an exact name over another tool's alias", () => {
    // A tool literally named after another's alias must still win by name.
    const shadow: SecCatalog = {
      ...CAT,
      tools: [{ ...FEROX, name: "scan", aliases: [] }, NMAP],
    };
    // "scan" is now FEROX's name AND NMAP's alias → the name wins.
    expect(findTool("scan", shadow)?.name).toBe("scan");
  });

  it("returns undefined for an unknown tool", () => {
    expect(findTool("hydra", CAT)).toBeUndefined();
  });
});

describe("presetFlagHelp", () => {
  it("returns only the flags the preset actually emits, in catalogue order", () => {
    const service = NMAP.presets.find((p) => p.name === "service")!;
    const help = presetFlagHelp(NMAP, service);
    const flags = help.map(([f]) => f);
    // -sV/-sC (bare-flag literals) + -p/-T/-oA (flag/joined) are used…
    expect(flags).toEqual(["-sV", "-sC", "-p", "-T", "-oA"]);
    // …and -p- (a different preset's literal) is not.
    expect(flags).not.toContain("-p-");
  });

  it("covers each segment kind that contributes a flag", () => {
    const full = NMAP.presets.find((p) => p.name === "full-tcp")!;
    const flags = presetFlagHelp(NMAP, full).map(([f]) => f);
    expect(flags).toContain("-p-"); // bare-flag literal
    expect(flags).toContain("-p"); // flag segment
    expect(flags).toContain("-T"); // joined segment
  });

  it("is empty when no used flag has help", () => {
    const dir = FEROX.presets[0];
    // FEROX only has -u/-w help, both used → non-empty; strip the help table.
    expect(presetFlagHelp({ ...FEROX, flag_help: [] }, dir)).toEqual([]);
  });
});
