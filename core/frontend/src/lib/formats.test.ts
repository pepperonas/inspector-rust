import { beforeAll, describe, expect, it } from "vitest";
import type { RecordAspect } from "./ipc";
import { ADDABLE_FORMATS, addable, batchOrder, batchStep, formatLabel, isQuiet, type Batch } from "./formats";

let RUST = "";
beforeAll(async () => {
  const { readFileSync } = (await import("node:" + "fs")) as unknown as {
    readFileSync(path: string, encoding: "utf8"): string;
  };
  const cwd = (globalThis as unknown as { process: { cwd(): string } }).process.cwd();
  RUST = readFileSync(`${cwd}/../rust-lib/src/record/formats.rs`, "utf8") + readFileSync(`${cwd}/../rust-lib/src/record/editor.rs`, "utf8");
});

describe("formats", () => {
  it("offers the formats Rust accepts for adding", () => {
    const m = RUST.match(/pub const ADDABLE: \[Aspect; (\d+)\] = \[([^\]]+)\]/);
    expect(m).toBeTruthy();
    const names = m![2].split(",").map((s) => s.trim().replace("Aspect::", "")).filter(Boolean);
    expect(names.length).toBe(ADDABLE_FORMATS.length);
    // Each Rust variant's serde name is the TS value, in the same order.
    const serde = names.map((n) => RUST.match(new RegExp(`#\\[serde\\(rename = "([^"]+)"\\)\\]\\s*${n},`))?.[1]);
    expect(serde).toEqual([...ADDABLE_FORMATS]);
  });

  it("labels and offers", () => {
    expect(formatLabel("9:16", "Auto", "Original")).toBe("9:16");
    expect(formatLabel("auto", "Auto", "Original")).toBe("Auto");
    expect(formatLabel("source", "Auto", "Original")).toBe("Original");
    expect(addable(["auto", "9:16"])).toEqual(["16:9", "1:1", "4:3", "3:4", "21:9"]);
  });
});

describe("batch export", () => {
  const formats: RecordAspect[] = ["16:9", "9:16", "1:1"];

  it("exports the ticked formats in the project's order, or the active one", () => {
    expect(batchOrder(formats, new Set<RecordAspect>(["1:1", "16:9"]), "9:16")).toEqual(["16:9", "1:1"]);
    expect(batchOrder(formats, new Set(), "9:16")).toEqual(["9:16"]);
  });

  it("goes on after a failed format and keeps every outcome", () => {
    let b: Batch = { queue: ["16:9", "9:16", "1:1"], results: [] };
    expect(isQuiet(b)).toBe(true);
    b = batchStep(b, { path: "/a.mp4" });
    b = batchStep(b, { error: "boom", cancelled: false });
    expect(b.queue).toEqual(["1:1"]);
    expect(isQuiet(b)).toBe(false);
    b = batchStep(b, { path: "/c.mp4" });
    expect(b).toEqual({
      queue: [],
      results: [
        { aspect: "16:9", path: "/a.mp4" },
        { aspect: "9:16", error: "boom" },
        { aspect: "1:1", path: "/c.mp4" },
      ],
    });
    expect(batchStep(b, { path: "/late.mp4" })).toBe(b);
  });

  it("stops everything on cancel", () => {
    const b = batchStep({ queue: ["16:9", "9:16"], results: [] }, { error: "cancelled", cancelled: true });
    expect(b.queue).toEqual([]);
    expect(b.results).toHaveLength(1);
  });
});

describe("aspectSwitchTarget", () => {
  it("switches instead of overwriting a format the project keeps", async () => {
    const { aspectSwitchTarget } = await import("./formats");
    const cur = { output: { aspect: "16:9" as RecordAspect }, variants: [{ aspect: "9:16" as RecordAspect }] };
    expect(aspectSwitchTarget(cur, { output: { aspect: "9:16" } })).toBe("9:16");
    expect(aspectSwitchTarget(cur, { output: { aspect: "1:1" } })).toBeNull();
    expect(aspectSwitchTarget(cur, { output: { aspect: "16:9" } })).toBeNull();
    expect(aspectSwitchTarget({ output: { aspect: "16:9" } }, { output: { aspect: "9:16" } })).toBeNull();
  });
});

describe("makeBatchRunner", () => {
  it("runs one export after the other and reports every outcome", async () => {
    const { makeBatchRunner } = await import("./formats");
    const pos: (string | null)[] = [];
    let done: unknown = null;
    const started: [RecordAspect, boolean][] = [];
    const r = makeBatchRunner({ pos: (p) => pos.push(p ? `${p.format} ${p.n}/${p.total}` : null), done: (x) => (done = x) });
    expect(r.active()).toBe(false);
    r.start(["16:9", "9:16", "1:1"], async (f, quiet) => {
      started.push([f, quiet]);
      if (f === "9:16") throw new Error("busy");
    });
    expect(started).toEqual([["16:9", true]]);
    r.outcome({ path: "/a.mp4" });
    // The second could not even start: recorded, and the third is up without an event.
    await Promise.resolve();
    await Promise.resolve();
    expect(started).toEqual([["16:9", true], ["9:16", true], ["1:1", false]]);
    r.outcome({ path: "/c.mp4" });
    expect(r.active()).toBe(false);
    expect(pos).toEqual(["16:9 1/3", "9:16 2/3", "1:1 3/3", null]);
    expect(done).toEqual([
      { aspect: "16:9", path: "/a.mp4" },
      { aspect: "9:16", error: "Error: busy" },
      { aspect: "1:1", path: "/c.mp4" },
    ]);
    // A stray event after the batch changes nothing.
    r.outcome({ path: "/late.mp4" });
    expect(r.active()).toBe(false);
  });
});
