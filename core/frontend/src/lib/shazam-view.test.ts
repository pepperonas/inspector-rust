import { describe, expect, it } from "vitest";
import { SHAZAM_VIEWS, shazamView } from "./shazam-view";

describe("shazamView", () => {
  it("bare shazam listens", () => {
    expect(shazamView("")).toBe("listen");
    expect(shazamView(undefined)).toBe("listen");
  });
  it("history words open the list", () => {
    expect(shazamView("history")).toBe("history");
    expect(shazamView("list")).toBe("history");
  });
  it("lyrics words capture lyrics", () => {
    expect(shazamView("lyrics")).toBe("lyrics");
    expect(shazamView("Songtext")).toBe("lyrics");
    expect(shazamView("lyric")).toBe("lyrics");
  });
  it("the lyrics keyword wins without an argument", () => {
    expect(shazamView("", "lyrics")).toBe("lyrics");
    expect(shazamView("", "shazam")).toBe("listen");
  });
  it("offers each view once as a sub-row", () => {
    expect(SHAZAM_VIEWS.map((v) => v.view).sort()).toEqual(["history", "listen", "lyrics"]);
  });
});
