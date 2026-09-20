import { describe, it, expect } from "vitest";
import {
  OVERALL,
  toneGlyph,
  mailboxTone,
  mailboxLabel,
  catchAllLabel,
  dnsTone,
  mxSummary,
  summaryLine,
  type DnsResult,
  type MailCheckResult,
} from "./mailcheck";

const dns = (over: Partial<DnsResult>): DnsResult => ({
  resolves: true,
  mx: [{ preference: 10, host: "mx1.example.com" }],
  implicit_mx: false,
  note: null,
  ...over,
});

describe("overall label + tone", () => {
  it("maps every variant to a label and tone", () => {
    expect(OVERALL.deliverable).toEqual({ label: "DELIVERABLE", tone: "ok" });
    expect(OVERALL.deliverable_likely.label).toBe("DELIVERABLE LIKELY");
    expect(OVERALL.risky.tone).toBe("warn");
    expect(OVERALL.undeliverable.tone).toBe("fail");
    expect(OVERALL.invalid.tone).toBe("fail");
  });
});

describe("glyphs (never color-only)", () => {
  it("gives each tone a distinct glyph", () => {
    expect(toneGlyph("ok")).toBe("✓");
    expect(toneGlyph("fail")).toBe("✕");
    expect(toneGlyph("unknown")).toBe("?");
    expect(toneGlyph("warn")).toBe("!");
    expect(toneGlyph("skipped")).toBe("–");
  });
});

describe("mailbox", () => {
  it("tone + label per status", () => {
    expect(mailboxTone("likely_valid")).toBe("ok");
    expect(mailboxTone("invalid")).toBe("fail");
    expect(mailboxTone("unlikely")).toBe("fail");
    expect(mailboxTone("unknown")).toBe("unknown");
    expect(mailboxTone("not_probed")).toBe("skipped");
    expect(mailboxLabel("unknown")).toBe("Inconclusive");
    expect(mailboxLabel("likely_valid")).toBe("Likely valid");
    expect(mailboxLabel("not_probed")).toBe("Not probed");
  });
});

describe("catch-all", () => {
  it("labels", () => {
    expect(catchAllLabel("yes")).toBe("Yes");
    expect(catchAllLabel("no")).toBe("No");
    expect(catchAllLabel("unknown")).toBe("Unknown");
  });
});

describe("dns row", () => {
  it("tone: resolved=ok, implicit=warn, none=fail", () => {
    expect(dnsTone(dns({}))).toBe("ok");
    expect(dnsTone(dns({ implicit_mx: true }))).toBe("warn");
    expect(dnsTone(dns({ resolves: false, mx: [] }))).toBe("fail");
  });
  it("summary text", () => {
    expect(mxSummary(dns({}))).toBe("1 MX record");
    expect(
      mxSummary(dns({ mx: [{ preference: 10, host: "a" }, { preference: 20, host: "b" }] })),
    ).toBe("2 MX records");
    expect(mxSummary(dns({ implicit_mx: true }))).toBe("No MX — implicit (A record)");
    expect(mxSummary(dns({ resolves: false, mx: [] }))).toBe("No MX / address record");
  });
});

describe("summaryLine", () => {
  it("is honest — never says the mailbox exists", () => {
    const r: MailCheckResult = {
      email: "hello@example.com",
      syntax_valid: true,
      domain: "example.com",
      disposable: false,
      dns: dns({}),
      smtp: {
        attempted: true,
        connected: false,
        tried_host: "mx1.example.com",
        banner: null,
        rcpt_code: null,
        mailbox: "unknown",
        catch_all: "unknown",
        note: "…",
      },
      overall: "deliverable_likely",
    };
    const line = summaryLine(r);
    expect(line).toContain("hello@example.com — DELIVERABLE LIKELY");
    expect(line).toContain("mx=1");
    expect(line).toContain("mailbox=Inconclusive");
    expect(line).not.toContain("exists");
  });
});
