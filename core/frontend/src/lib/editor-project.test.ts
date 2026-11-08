import { describe, expect, it } from "vitest";
import type { EditorDoc } from "./editor-geometry";
import { parseProject, serializeProject } from "./editor-project";

const doc: EditorDoc = {
  annotations: [
    { id: "r", type: "rect", x: 1, y: 2, w: 3, h: 4, color: "#f00", width: 4 },
    { id: "i", type: "image", x: 0, y: 0, w: 10, h: 10, src: "data:image/png;base64,AAAA" },
  ],
  crop: { x: 0, y: 0, w: 50, h: 40 },
  background: null,
  orient: { rot: 90, flip: true },
};
const bg = { kind: "solid" as const, color: "#fff", padding: 20, radius: 8, shadow: true };

describe("editor project", () => {
  it("round-trips document and background", () => {
    const p = parseProject(serializeProject(doc, bg));
    expect(p).toEqual({ doc, background: bg });
  });
  it("rejects broken or foreign files", () => {
    expect(parseProject("nope")).toBeNull();
    expect(parseProject("{}")).toBeNull();
    expect(parseProject(JSON.stringify({ version: 99, doc, background: null }))).toBeNull();
    expect(parseProject(JSON.stringify({ version: 1, doc: { annotations: "x" } }))).toBeNull();
  });
  it("drops elements it can't draw safely, keeps the rest", () => {
    const evil = { ...doc, annotations: [...doc.annotations, { id: "x", type: "image", x: 0, y: 0, w: 1, h: 1, src: "https://evil/a.png" }, { id: "y", type: "unknown" }, null] };
    const p = parseProject(JSON.stringify({ version: 1, doc: evil, background: null }));
    expect(p?.doc.annotations.map((a) => a.id)).toEqual(["r", "i"]);
  });
  it("a missing crop, orient or background reads as none", () => {
    const p = parseProject(JSON.stringify({ version: 1, doc: { annotations: [] } }));
    expect(p).toEqual({ doc: { annotations: [], crop: null, background: null }, background: null });
  });
});
