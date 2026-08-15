import { describe, it, expect } from "vitest";
import { slugify, generateUuids, sha256Hex, formatJson, decodeJwt } from "./devtools";

describe("slugify", () => {
  it("lowercases and dashes spaces", () => {
    expect(slugify("Hello World")).toBe("hello-world");
  });
  it("collapses runs of separators and trims", () => {
    expect(slugify("  Foo   bar__baz  ")).toBe("foo-bar-baz");
    expect(slugify("a---b")).toBe("a-b");
  });
  it("strips diacritics", () => {
    expect(slugify("Café Über Señor")).toBe("cafe-uber-senor");
  });
  it("drops punctuation entirely", () => {
    expect(slugify("Hello, World! (2024)")).toBe("hello-world-2024");
  });
  it("returns empty for all-punctuation input", () => {
    expect(slugify("!!!")).toBe("");
    expect(slugify("")).toBe("");
  });
});

describe("generateUuids", () => {
  it("generates one valid v4 UUID by default", () => {
    const out = generateUuids(1);
    expect(out.split("\n")).toHaveLength(1);
    expect(out).toMatch(
      /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/,
    );
  });
  it("generates n newline-joined UUIDs", () => {
    expect(generateUuids(5).split("\n")).toHaveLength(5);
  });
  it("clamps to [1, 100] and floors fractionals", () => {
    expect(generateUuids(0).split("\n")).toHaveLength(1);
    expect(generateUuids(-3).split("\n")).toHaveLength(1);
    expect(generateUuids(1000).split("\n")).toHaveLength(100);
    expect(generateUuids(3.9).split("\n")).toHaveLength(3);
    expect(generateUuids(NaN).split("\n")).toHaveLength(1);
  });
  it("produces unique values", () => {
    const set = new Set(generateUuids(50).split("\n"));
    expect(set.size).toBe(50);
  });
});

describe("sha256Hex", () => {
  it("matches the known digest of the empty string", async () => {
    expect(await sha256Hex("")).toBe(
      "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
    );
  });
  it("matches the known digest of 'abc'", async () => {
    expect(await sha256Hex("abc")).toBe(
      "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad",
    );
  });
  it("is 64 lowercase hex chars", async () => {
    const h = await sha256Hex("inspector-rust");
    expect(h).toMatch(/^[0-9a-f]{64}$/);
  });
});

describe("formatJson", () => {
  it("pretty-prints with 2-space indent", () => {
    expect(formatJson('{"a":1,"b":[2,3]}')).toBe(
      '{\n  "a": 1,\n  "b": [\n    2,\n    3\n  ]\n}',
    );
  });
  it("throws on invalid JSON", () => {
    expect(() => formatJson("{not json")).toThrow();
  });
});

describe("decodeJwt", () => {
  // A standard HS256 JWT (jwt.io sample): {alg:HS256,typ:JWT} / {sub,name,iat}.
  const TOKEN =
    "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9." +
    "eyJzdWIiOiIxMjM0NTY3ODkwIiwibmFtZSI6IkpvaG4gRG9lIiwiaWF0IjoxNTE2MjM5MDIyfQ." +
    "SflKxwRJSMeKKF2QT4fwpMeJf36POk6yJV_adQssw5c";

  it("decodes non-ASCII claims as UTF-8, not latin1", () => {
    // REGRESSION (found 2026-08-15): plain `atob` returns latin1 bytes, so
    // every Umlaut/emoji/CJK claim mojibaked ("Jörg" → "JÃ¶rg") — and
    // `name`/`given_name` carry them constantly.
    const b64url = (o: unknown) =>
      btoa(String.fromCharCode(...new TextEncoder().encode(JSON.stringify(o))))
        .replace(/\+/g, "-")
        .replace(/\//g, "_")
        .replace(/=+$/, "");
    const payload = { name: "Jörg Müller", city: "東京", tag: "🦀" };
    const parsed = JSON.parse(decodeJwt(`${b64url({ alg: "HS256" })}.${b64url(payload)}.sig`));
    expect(parsed.payload).toEqual(payload);
  });

  it("decodes header + payload to pretty JSON", () => {
    const out = decodeJwt(TOKEN);
    const parsed = JSON.parse(out);
    expect(parsed.header).toEqual({ alg: "HS256", typ: "JWT" });
    expect(parsed.payload).toEqual({ sub: "1234567890", name: "John Doe", iat: 1516239022 });
    expect(parsed.signature).toBe("SflKxwRJSMeKKF2QT4fwpMeJf36POk6yJV_adQssw5c");
  });
  it("tolerates a token without a signature", () => {
    const parts = TOKEN.split(".");
    const out = decodeJwt(`${parts[0]}.${parts[1]}`);
    expect(JSON.parse(out).signature).toBeNull();
  });
  it("throws on a non-JWT string", () => {
    expect(() => decodeJwt("not-a-jwt")).toThrow();
  });
});

describe("decodeJwt — base64url alphabet + padding", () => {
  // The header/payload of a real token routinely contain `-` and `_`, the
  // base64url stand-ins for `+` and `/`. The existing tests only exercise a
  // signature containing `_` — and the signature is never decoded.
  const HEADER = "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9"; // {"alg":"HS256","typ":"JWT"}
  const PAYLOAD = "eyJzdWIiOiI_Pz4-Pz8ifQ"; // {"sub":"??>>??"} — has BOTH _ and -

  it("translates `-` and `_` back to `+` and `/` before decoding", () => {
    const parsed = JSON.parse(decodeJwt(`${HEADER}.${PAYLOAD}.sig`));
    expect(parsed.payload).toEqual({ sub: "??>>??" });
  });

  it("re-pads a segment whose length is not a multiple of four", () => {
    // PAYLOAD is 22 chars → needs two `=` before atob will take it.
    expect(PAYLOAD.length % 4).toBe(2);
    expect(() => decodeJwt(`${HEADER}.${PAYLOAD}`)).not.toThrow();
  });

  it("ignores segments beyond the signature", () => {
    // A 5-segment JWE-shaped token still reports header/payload/signature.
    const parsed = JSON.parse(decodeJwt(`${HEADER}.${PAYLOAD}.sig.iv.tag`));
    expect(parsed.signature).toBe("sig");
  });

  it("trims surrounding whitespace from a pasted token", () => {
    const parsed = JSON.parse(decodeJwt(`\n  ${HEADER}.${PAYLOAD}  \n`));
    expect(parsed.header).toEqual({ alg: "HS256", typ: "JWT" });
  });

  it("throws (rather than half-decoding) when a segment is not valid JSON", () => {
    expect(() => decodeJwt(`${HEADER}.bm90LWpzb24`)).toThrow(); // "not-json"
    expect(() => decodeJwt("")).toThrow();
    expect(() => decodeJwt(".")).toThrow();
  });
});

describe("slugify — URL safety is the whole point", () => {
  it("output is always URL-safe, however wild the input", () => {
    const inputs = [
      "Hello World",
      "Café Über Señor",
      "日本語のテキスト",
      "🦀 emoji 🎉 party",
      "  ---  ",
      "MiXeD_CaSe--and__separators",
      "Grüße aus Köln!",
      "a".repeat(500),
      "\t\n\r",
      "100% pure",
    ];
    for (const input of inputs) {
      const out = slugify(input);
      expect(out, input).toMatch(/^[a-z0-9]*(-[a-z0-9]+)*$/); // no leading/trailing/double dash
      expect(out).not.toMatch(/^-|-$|--/);
    }
  });

  it("is idempotent — slugging a slug changes nothing", () => {
    for (const input of ["Hello World", "Café Über", "a---b", "100% pure"]) {
      const once = slugify(input);
      expect(slugify(once)).toBe(once);
    }
  });

  it("drops scripts it cannot transliterate rather than emitting mojibake", () => {
    // NFKD has no ASCII decomposition for CJK/emoji — they must vanish, not
    // leak bytes into the URL.
    expect(slugify("日本語")).toBe("");
    expect(slugify("🦀")).toBe("");
    expect(slugify("Rust 🦀 Lang")).toBe("rust-lang");
  });

  it("keeps digits and collapses a mixed separator run", () => {
    expect(slugify("Version 2.0 — Release_Notes")).toBe("version-2-0-release-notes");
  });
});

describe("formatJson — non-object roots", () => {
  it("pretty-prints top-level scalars, arrays and null", () => {
    expect(formatJson("42")).toBe("42");
    expect(formatJson('"hi"')).toBe('"hi"');
    expect(formatJson("null")).toBe("null");
    expect(formatJson("true")).toBe("true");
    expect(formatJson("[1,2]")).toBe("[\n  1,\n  2\n]");
  });

  it("throws on empty / whitespace-only input", () => {
    expect(() => formatJson("")).toThrow();
    expect(() => formatJson("   ")).toThrow();
  });

  it("preserves Unicode verbatim (no \\uXXXX escaping)", () => {
    expect(formatJson('{"k":"Grüße 🦀"}')).toBe('{\n  "k": "Grüße 🦀"\n}');
  });

  it("is idempotent — re-formatting already-pretty JSON is a no-op", () => {
    const once = formatJson('{"a":1,"b":[2,3]}');
    expect(formatJson(once)).toBe(once);
  });
});
