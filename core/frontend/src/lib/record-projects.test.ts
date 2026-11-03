import { describe, expect, it } from "vitest";
import { filterProjects, foldText, lastChange } from "./record-projects";
import type { RecordProjectItem } from "./ipc";

const item = (title: string, createdAt: number, updatedAt?: number): RecordProjectItem => ({
  dir: `/p/${title}`, title, createdAt, updatedAt, durationMs: 1, status: "ready", failure: null, active: false,
});

describe("record project list rules", () => {
  it("last change is the edit, else the creation", () => {
    expect(lastChange(item("a", 10, 50))).toBe(50);
    expect(lastChange(item("a", 10, 0))).toBe(10);
    expect(lastChange(item("a", 10))).toBe(10);
  });

  it("folds case, accents and ß", () => {
    expect(foldText("Übersicht STRAßE Café")).toBe("ubersicht strasse cafe");
  });

  it("every term must match; empty query keeps all in order", () => {
    const list = [item("Release notes", Date.parse("2026-09-28T15:40:00")), item("Onboarding", Date.parse("2026-10-02T09:00:00"))];
    expect(filterProjects(list, "  ", "en-US")).toBe(list);
    expect(filterProjects(list, "release NOTES", "en-US").map((p) => p.title)).toEqual(["Release notes"]);
    expect(filterProjects(list, "release onboarding", "en-US")).toEqual([]);
  });

  it("matches the ISO day and the localized date of creation and last edit", () => {
    const p = item("x", Date.parse("2026-03-14T10:00:00"), Date.parse("2026-05-02T10:00:00"));
    expect(filterProjects([p], "2026-03-14", "de-DE")).toHaveLength(1);
    expect(filterProjects([p], "2026-05-02", "de-DE")).toHaveLength(1);
    expect(filterProjects([p], "märz", "de-DE")).toHaveLength(1);
    expect(filterProjects([p], "marz", "de-DE"), "accent-insensitive month").toHaveLength(1);
    expect(filterProjects([p], "May", "en-US")).toHaveLength(1);
    expect(filterProjects([p], "2025", "en-US")).toHaveLength(0);
  });
});
