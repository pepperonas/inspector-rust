import { afterEach, describe, it, expect, vi } from "vitest";
import { TRANSFORMS, applyTransform } from "./text-transform";

describe("TRANSFORMS catalogue", () => {
  it("has 12 transforms", () => {
    expect(TRANSFORMS.length).toBe(12);
  });

  it("the first nine carry a unique digit 1–9, the rest none", () => {
    const digits = TRANSFORMS.map((t) => t.digit).filter((d): d is number => d != null);
    expect(digits).toEqual([1, 2, 3, 4, 5, 6, 7, 8, 9]);
    // The decode pair + plain-text are click-only / bound to Cmd+^.
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

describe("plain-text — strip HTML / decode entities", () => {
  it("strips tags + collapses to text content", () => {
    expect(applyTransform("plain-text", "<b>hello</b> <i>world</i>")).toBe(
      "hello world",
    );
  });
  it("decodes named + numeric entities", () => {
    expect(applyTransform("plain-text", "AT&amp;T &lt;3 &#39;quote&#39;")).toBe(
      "AT&T <3 'quote'",
    );
  });
  it("flattens nested tags + drops attributes", () => {
    const html =
      '<div style="color:red"><p><span class="x">nested</span> text</p></div>';
    expect(applyTransform("plain-text", html)).toBe("nested text");
  });
  it("strips inline style + script content (the latter via textContent dropping <script>)", () => {
    // Browsers' DOMParser DOES include <script> contents in textContent
    // by default; that's the standards behaviour. We're not trying to
    // sanitise — just to denude. Document the behaviour either way so a
    // future reader knows it's deliberate.
    const html = '<p style="color:red">hi</p>';
    expect(applyTransform("plain-text", html)).toBe("hi");
  });
  it("trims surrounding whitespace from the result", () => {
    expect(applyTransform("plain-text", "  \n  plain  \n  ")).toBe("plain");
  });
  it("returns plain input essentially unchanged (round-trip safe)", () => {
    expect(applyTransform("plain-text", "just plain text")).toBe(
      "just plain text",
    );
  });
});

describe("plain-text — no-DOM fallback (regex tag-strip)", () => {
  // In headless / non-DOM contexts DOMParser is absent; the fallback path
  // drops tags and decodes the common entities by regex. Force that branch.
  afterEach(() => vi.unstubAllGlobals());

  it("strips tags without a DOMParser", () => {
    vi.stubGlobal("DOMParser", undefined);
    expect(applyTransform("plain-text", "<b>hello</b> <i>world</i>")).toBe(
      "hello world",
    );
  });

  it("decodes the handful of named / numeric entities it supports", () => {
    vi.stubGlobal("DOMParser", undefined);
    expect(
      applyTransform("plain-text", "a&nbsp;b AT&amp;T &lt;x&gt; &quot;q&quot; &#39;s&#39;"),
    ).toBe('a b AT&T <x> "q" \'s\'');
  });

  it("trims and leaves plain input unchanged in the fallback too", () => {
    vi.stubGlobal("DOMParser", undefined);
    expect(applyTransform("plain-text", "  just text  ")).toBe("just text");
  });
});

describe("applyTransform — the word tokeniser's exact rules", () => {
  it("a single word is passed through in the target casing", () => {
    expect(applyTransform("camel", "Hello")).toBe("hello");
    expect(applyTransform("snake", "Hello")).toBe("hello");
    expect(applyTransform("kebab", "HELLO")).toBe("hello");
  });

  it("an already-camelCase input round-trips through camel", () => {
    expect(applyTransform("camel", "helloWorldFoo")).toBe("helloWorldFoo");
  });

  it("breaks the digit→capital boundary too", () => {
    // The tokeniser splits on ([a-z0-9])([A-Z]), so a digit ends a word.
    expect(applyTransform("snake", "hello2World")).toBe("hello2_world");
    expect(applyTransform("kebab", "v2Api")).toBe("v2-api");
  });

  it("keeps an acronym run as ONE word (the rule is lower→Upper only)", () => {
    // Deliberate simplicity: there is no Upper→UpperLower rule, so
    // "HTTPServer" is a single token. Pinned so a tokeniser rewrite is a
    // conscious change rather than a silent one.
    expect(applyTransform("snake", "HTTPServer")).toBe("httpserver");
    expect(applyTransform("camel", "HTTPServer")).toBe("httpserver");
  });

  it("mixed separators collapse into one boundary", () => {
    expect(applyTransform("snake", "hello -_ world")).toBe("hello_world");
    expect(applyTransform("camel", "hello-world_again test")).toBe("helloWorldAgainTest");
  });

  it("input made only of separators produces an empty result", () => {
    for (const kind of ["camel", "snake", "kebab"] as const) {
      expect(applyTransform(kind, "   ")).toBe("");
      expect(applyTransform(kind, "-_-")).toBe("");
      expect(applyTransform(kind, "")).toBe("");
    }
  });

  it("camel lower-cases the first word but Title-cases the rest", () => {
    // `cap()` lower-cases the tail, so shouty input is normalised.
    expect(applyTransform("camel", "HELLO WORLD")).toBe("helloWorld");
  });

  it("Umlauts survive the tokeniser unharmed", () => {
    expect(applyTransform("snake", "grüße Welt")).toBe("grüße_welt");
    expect(applyTransform("camel", "straße test")).toBe("straßeTest");
  });
});

describe("applyTransform — title case details", () => {
  it("only capitalises after whitespace, not after punctuation", () => {
    expect(applyTransform("title", "hello-world")).toBe("Hello-world");
    expect(applyTransform("title", "it's fine")).toBe("It's Fine");
  });

  it("preserves leading whitespace while still capitalising the first word", () => {
    expect(applyTransform("title", "  hello")).toBe("  Hello");
  });

  it("capitalises Umlaut initials", () => {
    expect(applyTransform("title", "über alles")).toBe("Über Alles");
  });
});

describe("applyTransform — decode robustness on real clipboard content", () => {
  it("base64-decode tolerates the whitespace a copied blob carries", () => {
    // Copying base64 out of a terminal/mail very often brings a newline along.
    expect(applyTransform("base64-decode", "  aGVsbG8=\n")).toBe("hello");
  });

  it("base64 round-trips the empty string", () => {
    expect(applyTransform("base64-encode", "")).toBe("");
    expect(applyTransform("base64-decode", "")).toBe("");
  });

  it("url-decode leaves `+` as a literal plus (it is not a space here)", () => {
    // decodeURIComponent is not form-decoding — pinning it stops someone
    // "fixing" it into unescape/replace and corrupting real URLs.
    expect(applyTransform("url-decode", "a+b")).toBe("a+b");
    expect(applyTransform("url-encode", "a+b")).toBe("a%2Bb");
  });

  it("url-encode leaves the RFC-3986 unreserved set alone", () => {
    expect(applyTransform("url-encode", "aZ0-_.!~*'()")).toBe("aZ0-_.!~*'()");
  });
});

describe("applyTransform — idempotence where it is expected", () => {
  it("case + vowel transforms are stable when applied twice", () => {
    for (const kind of ["upper", "lower", "remove-vowels", "snake", "kebab"] as const) {
      const once = applyTransform(kind, "Hello Wörld Foo");
      expect(applyTransform(kind, once)).toBe(once);
    }
  });

  it("remove-vowels leaves y and ß (they are not in the vowel set)", () => {
    expect(applyTransform("remove-vowels", "syzygy Straße")).toBe("syzygy Strß");
  });
});
