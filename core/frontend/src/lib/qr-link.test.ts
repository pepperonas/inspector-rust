import { describe, it, expect } from "vitest";
import { linkForQr, qrLinkEntry } from "./qr-link";

describe("linkForQr", () => {
  it("accepts a pasted http(s) or www link", () => {
    expect(linkForQr("https://celox.io/projekte?x=1#a")).toBe("https://celox.io/projekte?x=1#a");
    expect(linkForQr("  http://example.org  ")).toBe("http://example.org");
    expect(linkForQr("www.youtube.com/watch?v=abc")).toBe("www.youtube.com/watch?v=abc");
    expect(linkForQr("http://localhost:1420/x")).toBe("http://localhost:1420/x");
  });
  it("rejects plain searches, bare domains and text with a link inside", () => {
    expect(linkForQr("")).toBeNull();
    expect(linkForQr("example.com")).toBeNull();
    expect(linkForQr("https")).toBeNull();
    expect(linkForQr("see https://x.io please")).toBeNull();
    expect(linkForQr("https://nodot")).toBeNull();
  });
  it("rejects a link too long to encode", () => {
    expect(linkForQr(`https://x.io/${"a".repeat(2400)}`)).toBeNull();
  });
});

describe("qrLinkEntry", () => {
  it("builds the SAME command row `qr <link>` would (kind + arg)", () => {
    const e = qrLinkEntry("https://celox.io", false);
    expect(e?.kind).toBe("command");
    if (e?.kind !== "command") throw new Error("unreachable");
    expect(e.data.commandKind).toBe("qr");
    expect(e.data.arg).toBe("https://celox.io");
  });
  it("yields nothing when the query already is a command (no duplicate row)", () => {
    expect(qrLinkEntry("https://celox.io", true)).toBeNull();
  });
});
