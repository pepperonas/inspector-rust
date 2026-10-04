import { describe, expect, it } from "vitest";
import { typedTextEntry } from "./typed-text";
import { CUSTOM_COMMAND_KINDS } from "./types";

describe("typed-text row", () => {
  it("appears with an app hit and carries the trimmed text", () => {
    expect(typedTextEntry("  BRIDGE ", true)).toEqual({ kind: "typed-text", data: { text: "BRIDGE" } });
  });
  it("does not appear without an app hit or without text", () => {
    expect(typedTextEntry("BRIDGE", false)).toBeNull();
    expect(typedTextEntry("   ", true)).toBeNull();
  });
  it("is not styled as a custom command (it pastes text, it runs nothing)", () => {
    expect(CUSTOM_COMMAND_KINDS.has("typed-text" as never)).toBe(false);
  });
});

describe("typed-text ranking (source pin)", () => {
  it("sits directly below the app hit — launching stays the first choice", async () => {
    // Same disk-read pattern as motion-stage.test.ts.
    const { readFileSync } = (await import("node:" + "fs")) as unknown as {
      readFileSync(path: string, encoding: "utf8"): string;
    };
    const cwd = (globalThis as unknown as { process: { cwd(): string } }).process.cwd();
    const code = readFileSync(cwd + "/src/App.tsx", "utf8").replace(/\/\/.*$/gm, "");
    const app = code.indexOf("...(appEntry ? [appEntry] : []),");
    const typed = code.indexOf("...(typedTextRow ? [typedTextRow] : []),");
    expect(app).toBeGreaterThan(-1);
    expect(typed).toBeGreaterThan(app);
    // Nothing in between: the text is exactly one ↓ below the app.
    expect(code.slice(app, typed).replace(/\s+/g, "")).toBe("...(appEntry?[appEntry]:[]),");
  });
});
