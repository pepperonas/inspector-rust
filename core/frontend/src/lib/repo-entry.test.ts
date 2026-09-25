import { describe, it, expect } from "vitest";
import { repoUrlEntry } from "./repo-url";
import { CUSTOM_COMMAND_KINDS } from "./types";

describe("repo-url list entry", () => {
  it("appears for a bare repo URL and carries owner/repo", () => {
    const e = repoUrlEntry("https://github.com/o/r", false);
    expect(e).toMatchObject({ kind: "repo-url", data: { owner: "o", repo: "r" } });
  });
  it("does not appear while an explicit command is typed (repo <url> has its own row)", () => {
    expect(repoUrlEntry("https://github.com/o/r", true)).toBeNull();
  });
  it("does not appear for issue links or text", () => {
    expect(repoUrlEntry("https://github.com/o/r/issues/1", false)).toBeNull();
    expect(repoUrlEntry("hallo", false)).toBeNull();
  });
  it("is a custom command row (red accent, outranks apps)", () => {
    expect(CUSTOM_COMMAND_KINDS.has("repo-url")).toBe(true);
  });
});

describe("repo-url ranking (source pin)", () => {
  it("is spliced before the app-launcher hit — custom commands outrank apps", async () => {
    // Same disk-read pattern as motion-stage.test.ts: import.meta.url is a
    // server-style `/src/…` URL under vitest, so resolve from the cwd.
    const { readFileSync } = (await import("node:" + "fs")) as unknown as {
      readFileSync(path: string, encoding: "utf8"): string;
    };
    const cwd = (globalThis as unknown as { process: { cwd(): string } }).process.cwd();
    const src = readFileSync(cwd + "/src/App.tsx", "utf8");
    const code = src.replace(/\/\/.*$/gm, "");
    const row = code.indexOf("...(repoUrlRow ?");
    expect(row).toBeGreaterThan(-1);
    expect(row).toBeLessThan(code.indexOf("...(appEntry ?"));
  });
});
