import { describe, it, expect } from "vitest";
import { TRANSFORMS, applyTransform } from "./text-transform";

describe("TRANSFORMS catalogue", () => {
  it("has 11 transforms", () => {
    expect(TRANSFORMS.length).toBe(11);
  });

  it("the first nine carry a unique digit 1–9, the rest none", () => {
    const digits = TRANSFORMS.map((t) => t.digit).filter((d): d is number => d != null);
    expect(digits).toEqual([1, 2, 3, 4, 5, 6, 7, 8, 9]);
    // The last two (decode pair) are click-only.
    expect(TRANSFORMS.slice(9).every((t) => t.digit === undefined)).toBe(true);
  });

  it("every transform kind is unique with a non-empty label", () => {
    const kinds = new Set(TRANSFORMS.map((t) => t.kind));
    expect(kinds.size).toBe(TRANSFORMS.length);
    expect(TRANSFORMS.every((t) => t.label.length > 0)).toBe(true);
  });
});

describe("applyTransform — remove-vowels", () => {
  it("strips English vowels", () => {
    expect(applyTransform("remove-vowels", "hello world")).toBe("hll wrld");
  });
  it("strips uppercase vowels + German umlauts", () => {
    expect(applyTransform("remove-vowels", "HÄLLO Über")).toBe("HLL br");
  });
  it("handles an empty string", () => {
    expect(applyTransform("remove-vowels", "")).toBe("");
  });
});

describe("applyTransform — case", () => {
  it("upper / lower", () => {
    expect(applyTransform("upper", "Hello World")).toBe("HELLO WORLD");
    expect(applyTransform("lower", "Hello World")).toBe("hello world");
  });
  it("title case capitalises each word, lowercases the rest", () => {
    expect(applyTransform("title", "hELLO wORLD")).toBe("Hello World");
    expect(applyTransform("title", "the quick brown fox")).toBe(
      "The Quick Brown Fox",
    );
  });
});

describe("applyTransform — camel / snake / kebab", () => {
  it("camelCase from space-separated", () => {
    expect(applyTransform("camel", "hello world foo")).toBe("helloWorldFoo");
  });
  it("camelCase from snake_case", () => {
    expect(applyTransform("camel", "hello_world_foo")).toBe("helloWorldFoo");
  });
  it("snake_case from camelCase (breaks the boundary)", () => {
    expect(applyTransform("snake", "helloWorldFoo")).toBe("hello_world_foo");
  });
  it("snake_case from space-separated", () => {
    expect(applyTransform("snake", "Hello World")).toBe("hello_world");
  });
  it("kebab-case from camelCase", () => {
    expect(applyTransform("kebab", "helloWorldFoo")).toBe("hello-world-foo");
  });
  it("kebab-case from snake_case", () => {
    expect(applyTransform("kebab", "hello_world")).toBe("hello-world");
  });
  it("collapses runs of separators / whitespace", () => {
    expect(applyTransform("snake", "  hello   world  ")).toBe("hello_world");
    expect(applyTransform("kebab", "hello___world")).toBe("hello-world");
  });
});

describe("applyTransform — base64", () => {
  it("encodes ASCII", () => {
    expect(applyTransform("base64-encode", "hello")).toBe("aGVsbG8=");
  });
  it("encode → decode round-trips", () => {
    const original = "hello world";
    const enc = applyTransform("base64-encode", original);
    expect(applyTransform("base64-decode", enc)).toBe(original);
  });
  it("round-trips Unicode (umlauts, emoji, CJK)", () => {
    const original = "Grüße 🦀 世界";
    const enc = applyTransform("base64-encode", original);
    expect(applyTransform("base64-decode", enc)).toBe(original);
  });
  it("invalid base64 input decodes to itself (no-op, no throw)", () => {
    expect(applyTransform("base64-decode", "!!!not base64!!!")).toBe(
      "!!!not base64!!!",
    );
  });
});

describe("applyTransform — URL encode / decode", () => {
  it("encodes special characters", () => {
    expect(applyTransform("url-encode", "a b&c=d")).toBe("a%20b%26c%3Dd");
  });
  it("encode → decode round-trips", () => {
    const original = "path/to file?q=1&x=ü";
    const enc = applyTransform("url-encode", original);
    expect(applyTransform("url-decode", enc)).toBe(original);
  });
  it("malformed percent-sequence decodes to itself (no-op, no throw)", () => {
    expect(applyTransform("url-decode", "%zz%")).toBe("%zz%");
  });
});

describe("applyTransform — never throws", () => {
  it("every transform tolerates empty input", () => {
    for (const t of TRANSFORMS) {
      expect(() => applyTransform(t.kind, "")).not.toThrow();
    }
  });
  it("every transform tolerates a long Unicode string", () => {
    const wild = "Grüße 🦀 世界 ".repeat(50);
    for (const t of TRANSFORMS) {
      expect(() => applyTransform(t.kind, wild)).not.toThrow();
    }
  });
});
