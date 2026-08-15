import { describe, it, expect } from "vitest";
import { tryEvaluate, formatResult } from "./calc";

describe("tryEvaluate — basic arithmetic", () => {
  it("adds", () => {
    expect(tryEvaluate("1+2")?.display).toBe("3");
  });

  it("subtracts", () => {
    expect(tryEvaluate("10 - 4")?.display).toBe("6");
  });

  it("multiplies and divides", () => {
    expect(tryEvaluate("6 * 7")?.display).toBe("42");
    expect(tryEvaluate("10/4")?.display).toBe("2.5");
  });

  it("respects operator precedence", () => {
    expect(tryEvaluate("2 + 3 * 4")?.display).toBe("14");
    expect(tryEvaluate("(2 + 3) * 4")?.display).toBe("20");
  });

  it("handles unary minus", () => {
    expect(tryEvaluate("-5 + 3")?.display).toBe("-2");
    expect(tryEvaluate("3 * -2")?.display).toBe("-6");
  });

  it("supports power (right-associative)", () => {
    expect(tryEvaluate("2^10")?.display).toBe("1024");
    expect(tryEvaluate("2^3^2")?.display).toBe("512"); // 2^(3^2) = 2^9
  });

  it("handles modulo", () => {
    expect(tryEvaluate("10 % 3")?.display).toBe("1");
  });
});

describe("tryEvaluate — numbers", () => {
  it("parses decimals", () => {
    expect(tryEvaluate("0.1 + 0.2")?.value).toBeCloseTo(0.3, 10);
  });

  it("parses leading-dot decimals", () => {
    expect(tryEvaluate(".5 + .5")?.display).toBe("1");
  });

  it("parses scientific notation", () => {
    expect(tryEvaluate("1e3 + 1")?.display).toBe("1001");
    expect(tryEvaluate("1.5e-2 * 100")?.value).toBeCloseTo(1.5, 10);
  });

  it("ignores underscore digit grouping", () => {
    expect(tryEvaluate("1_000 + 1")?.display).toBe("1001");
  });
});

describe("tryEvaluate — functions and constants", () => {
  it("evaluates sqrt", () => {
    expect(tryEvaluate("sqrt(16)")?.display).toBe("4");
  });

  it("evaluates trig in radians", () => {
    expect(tryEvaluate("sin(0)")?.display).toBe("0");
    expect(tryEvaluate("cos(pi)")?.display).toBe("-1");
  });

  it("evaluates pi and tau constants", () => {
    expect(tryEvaluate("pi")?.value).toBeCloseTo(Math.PI, 10);
    expect(tryEvaluate("tau / 2")?.value).toBeCloseTo(Math.PI, 10);
  });

  it("evaluates min/max with multiple args", () => {
    expect(tryEvaluate("min(3, 1, 2)")?.display).toBe("1");
    expect(tryEvaluate("max(3, 1, 2)")?.display).toBe("3");
  });

  it("evaluates log10 and ln", () => {
    expect(tryEvaluate("log(1000)")?.value).toBeCloseTo(3, 10);
    expect(tryEvaluate("ln(e)")?.value).toBeCloseTo(1, 10);
  });

  it("evaluates abs and floor/ceil", () => {
    expect(tryEvaluate("abs(-7)")?.display).toBe("7");
    expect(tryEvaluate("floor(2.9)")?.display).toBe("2");
    expect(tryEvaluate("ceil(2.1)")?.display).toBe("3");
  });
});

describe("tryEvaluate — gating", () => {
  it("returns null for plain numbers without operators", () => {
    expect(tryEvaluate("42")).toBeNull();
    expect(tryEvaluate("3.14")).toBeNull();
  });

  it("returns null for non-math input", () => {
    expect(tryEvaluate("hello")).toBeNull();
    expect(tryEvaluate("foo bar")).toBeNull();
  });

  it("returns null for empty input", () => {
    expect(tryEvaluate("")).toBeNull();
    expect(tryEvaluate("   ")).toBeNull();
  });

  it("returns null for non-finite results", () => {
    expect(tryEvaluate("1/0")).toBeNull();
  });

  it("forces calc mode with leading =", () => {
    expect(tryEvaluate("=42")?.display).toBe("42");
    expect(tryEvaluate("=pi")?.value).toBeCloseTo(Math.PI, 10);
  });

  it("returns null for malformed input", () => {
    expect(tryEvaluate("1 +")).toBeNull();
    expect(tryEvaluate("(1 + 2")).toBeNull();
    expect(tryEvaluate("foo(1)")).toBeNull();
  });

  it("recognises constants without an operator", () => {
    expect(tryEvaluate("pi")?.value).toBeCloseTo(Math.PI, 10);
  });
});

describe("formatResult", () => {
  it("formats integers without a decimal point", () => {
    expect(formatResult(42)).toBe("42");
    expect(formatResult(-7)).toBe("-7");
  });

  it("trims trailing zeros from decimals", () => {
    expect(formatResult(2.5)).toBe("2.5");
    expect(formatResult(0.1 + 0.2)).toBe("0.3");
  });

  it("preserves scientific notation for tiny/huge values", () => {
    expect(formatResult(1e-12)).toMatch(/e/);
  });
});

describe("tryEvaluate — parse/tokenize error paths", () => {
  it("returns null for two values with no operator (leftover token)", () => {
    expect(tryEvaluate("2 3")).toBeNull();
  });

  it("returns null for an unclosed function call", () => {
    expect(tryEvaluate("sqrt(4")).toBeNull();
  });

  it("returns null for an unknown function", () => {
    expect(tryEvaluate("foo(2)")).toBeNull();
  });

  it("returns null for an unknown identifier", () => {
    expect(tryEvaluate("2 + foo")).toBeNull();
  });

  it("returns null for a malformed number", () => {
    expect(tryEvaluate("1..2 + 1")).toBeNull();
  });

  it("returns null for an unexpected character", () => {
    expect(tryEvaluate("2 + §")).toBeNull();
  });

  it("returns null for empty parens", () => {
    expect(tryEvaluate("()")).toBeNull();
  });
});

describe("tryEvaluate — forced `=` prefix and unary plus", () => {
  it("a bare `=` yields nothing", () => {
    expect(tryEvaluate("=")).toBeNull();
    expect(tryEvaluate("=   ")).toBeNull();
  });

  it("`= expr` evaluates the expression", () => {
    expect(tryEvaluate("= 2+2")?.display).toBe("4");
  });

  it("unary plus is accepted and is a no-op", () => {
    expect(tryEvaluate("+5 - 2")?.display).toBe("3");
  });

  it("scientific-notation literals work in expressions", () => {
    expect(tryEvaluate("1e3 + 1")?.display).toBe("1001");
    expect(tryEvaluate("1e+2 * 2")?.display).toBe("200");
    expect(tryEvaluate("1e-2 * 100")?.display).toBe("1");
  });

  it("the unicode π constant evaluates like pi", () => {
    expect(tryEvaluate("2 * π")?.value).toBeCloseTo(2 * Math.PI, 10);
  });
});

describe("formatResult — non-finite values", () => {
  it("passes through Infinity and NaN as strings", () => {
    expect(formatResult(Infinity)).toBe("Infinity");
    expect(formatResult(-Infinity)).toBe("-Infinity");
    expect(formatResult(NaN)).toBe("NaN");
  });
});

describe("tryEvaluate — remaining function table entries", () => {
  it("min / max / pow take multiple args", () => {
    expect(tryEvaluate("min(2, 3)")?.display).toBe("2");
    expect(tryEvaluate("max(2, 3)")?.display).toBe("3");
    expect(tryEvaluate("pow(2, 8)")?.display).toBe("256");
  });

  it("mod is a floored modulo (sign follows the divisor)", () => {
    expect(tryEvaluate("mod(10, 3)")?.display).toBe("1");
    expect(tryEvaluate("mod(-1, 3)")?.display).toBe("2");
  });

  it("hyperbolic functions evaluate", () => {
    expect(tryEvaluate("cosh(0)")?.display).toBe("1");
    expect(tryEvaluate("tanh(0)")?.display).toBe("0");
    expect(tryEvaluate("sinh(0)")?.display).toBe("0");
  });
});

describe("tryEvaluate — modulo is FLOORED, not JS remainder", () => {
  // `%` here is `a - b*floor(a/b)`, so the sign follows the DIVISOR. JS's own
  // `%` keeps the dividend's sign, and a refactor to the native operator would
  // silently change every negative result.
  it("a negative dividend wraps into the positive range", () => {
    expect(tryEvaluate("-1 % 3")?.display).toBe("2"); // JS `%` would say -1
    expect(tryEvaluate("-7 % 3")?.display).toBe("2");
  });

  it("a negative divisor pulls the result negative", () => {
    expect(tryEvaluate("10 % -3")?.display).toBe("-2"); // JS `%` would say 1
  });

  it("the `%` operator and the mod() function agree", () => {
    for (const [a, b] of [
      [-1, 3],
      [10, 3],
      [10, -3],
      [-10, -3],
    ]) {
      expect(tryEvaluate(`${a} % ${b}`)?.value).toBe(tryEvaluate(`mod(${a}, ${b})`)?.value);
    }
  });
});

describe("tryEvaluate — the rest of the function table", () => {
  it("maps every documented function to the right operation", () => {
    const cases: Array<[string, number]> = [
      ["cbrt(27)", 3],
      ["sign(-3)", -1],
      ["sign(0)", 0],
      ["round(2.5)", 3],
      ["round(-2.5)", -2], // JS rounds half UP, i.e. towards +∞
      ["log2(8)", 3],
      ["exp(0)", 1],
      ["asin(1)", Math.PI / 2],
      ["acos(1)", 0],
      ["atan(1)", Math.PI / 4],
      ["atan2(1, 1)", Math.PI / 4],
      ["tan(0)", 0],
    ];
    for (const [expr, want] of cases) {
      expect(tryEvaluate(expr)?.value, expr).toBeCloseTo(want, 10);
    }
  });

  it("a function whose result is not a number yields no row", () => {
    // NaN must never reach the list as a pasteable "result".
    expect(tryEvaluate("sqrt(-1)")).toBeNull();
    expect(tryEvaluate("log(-1)")).toBeNull();
    expect(tryEvaluate("mod(1, 0)")).toBeNull();
  });

  it("a function called with no arguments yields no row (never crashes)", () => {
    // Math.min() is Infinity, Math.abs() is NaN — all non-finite → filtered.
    expect(tryEvaluate("min()")).toBeNull();
    expect(tryEvaluate("max()")).toBeNull();
    expect(tryEvaluate("abs()")).toBeNull();
  });
});

describe("tryEvaluate — grammar edge cases a user can actually type", () => {
  it("skips tabs and newlines as whitespace", () => {
    expect(tryEvaluate("1\t+\n2")?.display).toBe("3");
  });

  it("unary minus binds TIGHTER than the power operator", () => {
    // `-2^2` === `(-2)^2` === 4 (Excel's reading), NOT -(2^2) = -4 (Python/bc).
    // The written grammar is `power := unary ('^' power)?`, so this is the
    // grammar's own choice — pinned so a rewrite has to make it consciously.
    expect(tryEvaluate("-2^2")?.display).toBe("4");
    expect(tryEvaluate("0 - 2^2")?.display).toBe("-4"); // explicit binary minus
  });

  it("has no implicit multiplication", () => {
    expect(tryEvaluate("2(3+4)")).toBeNull();
    expect(tryEvaluate("2pi")).toBeNull();
  });

  it("handles deep nesting and stray whitespace inside parens", () => {
    expect(tryEvaluate("( ( 1 + 2 ) * ( 3 + 4 ) )")?.display).toBe("21");
  });

  it("a trailing comma / empty argument slot is rejected", () => {
    expect(tryEvaluate("max(1,)")).toBeNull();
    expect(tryEvaluate("max(,1)")).toBeNull();
  });

  it("0^0 is 1 (IEEE pow), and division by a computed zero yields no row", () => {
    expect(tryEvaluate("0^0")?.display).toBe("1");
    expect(tryEvaluate("1/(2-2)")).toBeNull();
  });
});

describe("tryEvaluate — the echoed expression", () => {
  it("keeps the trimmed input verbatim, INCLUDING a forcing `=`", () => {
    // `expression` is what the row shows + what provenance the paste carries.
    expect(tryEvaluate("  2 + 2  ")?.expression).toBe("2 + 2");
    expect(tryEvaluate("= 2+2")?.expression).toBe("= 2+2");
  });
});

describe("formatResult — integer / precision boundary", () => {
  it("prints large integers plainly below 1e16", () => {
    expect(formatResult(1e15)).toBe("1000000000000000");
    expect(formatResult(-1e15)).toBe("-1000000000000000");
  });

  it("switches to exponent form at 1e16 (beyond exact integer display)", () => {
    expect(formatResult(1e16)).toMatch(/e\+?16/);
  });

  it("keeps 12 significant digits without trailing zero dust", () => {
    expect(formatResult(1 / 3)).toBe("0.333333333333");
    expect(formatResult(2.5)).toBe("2.5");
    expect(formatResult(-0.25)).toBe("-0.25");
  });
});
