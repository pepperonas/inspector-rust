import { describe, expect, it } from "vitest";
import { formatBilingualForCopy } from "./shazam-lyrics";

describe("formatBilingualForCopy", () => {
  it("joins original-only when source is German", () => {
    expect(
      formatBilingualForCopy(
        [
          { orig: "Hallo", trans: "Hallo" },
          { orig: "Welt", trans: "Welt" },
        ],
        "de",
      ),
    ).toBe("Hallo\nWelt");
  });

  it("pairs orig + translation with blank lines between", () => {
    expect(
      formatBilingualForCopy(
        [
          { orig: "Hello", trans: "Hallo" },
          { orig: "World", trans: "Welt" },
        ],
        "en",
      ),
    ).toBe("Hello\nHallo\n\nWorld\nWelt");
  });

  it("drops a redundant identical translation", () => {
    expect(
      formatBilingualForCopy([{ orig: "Yeah", trans: "Yeah" }], "en"),
    ).toBe("Yeah");
  });

  it("handles empty segments", () => {
    expect(formatBilingualForCopy([], "en")).toBe("");
  });
});
