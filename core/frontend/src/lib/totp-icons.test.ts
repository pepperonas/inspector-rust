import { describe, expect, it } from "vitest";
import {
  buildIconIndex,
  chipNeedsDark,
  iconForIssuer,
  monogramHue,
  monogramInitial,
  normalizeKey,
  type IconData,
} from "./totp-icons";
import realData from "./totp-icons.json";
import fePkg from "../../package.json";

const FIX: IconData = {
  v: "test",
  icons: {
    github: ["GitHub", "181717", "M1"],
    google: ["Google", "4285F4", "M2"],
    dotenv: [".ENV", "ECD53F", "M3"],
    x: ["X Corp", "000000", "M4"],
    ex: ["X", "111111", "M5"],
  },
  alias: { Dotenv: "dotenv" },
};
const idx = buildIconIndex(FIX);

describe("totp-icons issuer matching", () => {
  it("matches title, slug and case-insensitively", () => {
    expect(iconForIssuer(idx, "GitHub")?.slug).toBe("github");
    expect(iconForIssuer(idx, "github")?.slug).toBe("github");
    expect(iconForIssuer(idx, "GITHUB")?.slug).toBe("github");
  });

  it("resolves domain-shaped issuers, label next to the TLD first", () => {
    expect(iconForIssuer(idx, "github.com")?.slug).toBe("github");
    // ⚠️ right-to-left: in "x.github.com" the brand is "github", not the
    // subdomain "x" — a left-to-right scan would return the wrong icon.
    expect(iconForIssuer(idx, "x.github.com")?.slug).toBe("github");
    expect(iconForIssuer(idx, "https://accounts.google.com/foo")?.slug).toBe("google");
  });

  it("falls back to the first word for suffixed issuers", () => {
    expect(iconForIssuer(idx, "Google (privat)")?.slug).toBe("google");
  });

  it("a miss is an honest null — never a guessed look-alike", () => {
    expect(iconForIssuer(idx, "Amazon Web Services")).toBeNull();
    expect(iconForIssuer(idx, "")).toBeNull();
    expect(iconForIssuer(idx, "   ")).toBeNull();
  });

  it("aliases and symbol-titles resolve via normalisation", () => {
    expect(iconForIssuer(idx, "Dotenv")?.slug).toBe("dotenv");
    // ".ENV" normalises to "env" — the leading dot must not break the key.
    expect(iconForIssuer(idx, ".env")?.slug).toBe("dotenv");
  });

  it("slug beats a colliding title in the lookup", () => {
    // Slug "x" (X Corp) and title "X" (slug ex) normalise to the same key —
    // the canonical slug must win, or lookups depend on object order.
    expect(iconForIssuer(idx, "x")?.slug).toBe("x");
  });

  it("normalizeKey strips to the slug alphabet", () => {
    expect(normalizeKey("Amazon Pay")).toBe("amazonpay");
    expect(normalizeKey(".ENV")).toBe("env");
    expect(normalizeKey("Å-Ö!")).toBe("");
  });
});

describe("totp-icons chip + monogram", () => {
  it("light brands flip to the dark chip, dark brands stay on the light one", () => {
    expect(chipNeedsDark("181717")).toBe(false); // GitHub near-black
    expect(chipNeedsDark("FFFFFF")).toBe(true);
    expect(chipNeedsDark("ECD53F")).toBe(true); // yellow is too light too
    expect(chipNeedsDark("4285F4")).toBe(false); // Google blue
  });

  it("monogram is deterministic and takes the first letter/digit", () => {
    expect(monogramHue("Hostinger")).toBe(monogramHue("Hostinger"));
    expect(monogramHue("Hostinger")).toBeGreaterThanOrEqual(0);
    expect(monogramHue("Hostinger")).toBeLessThan(360);
    expect(monogramInitial("Ölbank")).toBe("Ö");
    expect(monogramInitial("2fas Test")).toBe("2");
    expect(monogramInitial("---")).toBe("?");
  });
});

describe("totp-icons generated data (smoke)", () => {
  const real = buildIconIndex(realData as unknown as IconData);

  it("carries the full set and the pinned version", () => {
    expect(Object.keys((realData as unknown as IconData).icons).length).toBeGreaterThan(3000);
    // Drift pin: the shipped JSON must come from the pinned devDependency —
    // a bumped package without a re-run of gen-totp-icons.mjs goes red here.
    expect((realData as unknown as IconData).v).toBe(
      (fePkg as { devDependencies: Record<string, string> }).devDependencies["simple-icons"],
    );
  });

  it("resolves everyday 2FA issuers", () => {
    for (const issuer of ["GitHub", "Google", "PayPal", "Hetzner", "Discord"]) {
      const icon = iconForIssuer(real, issuer);
      expect(icon, issuer).not.toBeNull();
      expect(icon!.path.length).toBeGreaterThan(20);
      expect(icon!.hex).toMatch(/^[0-9A-F]{6}$/i);
    }
  });
});
