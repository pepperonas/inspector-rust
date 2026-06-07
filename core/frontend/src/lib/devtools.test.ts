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
