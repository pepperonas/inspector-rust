// @vitest-environment happy-dom
import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import type { RecordEditDoc, RecordEffectRegion } from "../../lib/ipc";
import { CAPTION_DEFAULTS } from "../../lib/editor";
import { EFFECTS } from "../../lib/effects";
import { setCatalog } from "../../lib/i18n";
import en from "../../locales/en.json";
import de from "../../locales/de.json";
import { EFFECT_TEXT, EffectsTab, PARAM_LABEL, type EffectsTabProps } from "./EffectsTab";

afterEach(cleanup);

const region = (effect: string, over: Partial<RecordEffectRegion> = {}): RecordEffectRegion => ({
  id: "fx1",
  clip: 0,
  startMs: 1_000,
  endMs: 3_000,
  effect,
  values: {},
  ...over,
});

const doc = (effects: RecordEffectRegion[]): RecordEditDoc =>
  ({
    version: 6,
    clips: [{ startMs: 0, endMs: 10_000 }],
    speed: [],
    crop: [null],
    output: { aspect: "auto" },
    micGainDb: 0,
    appearance: {} as RecordEditDoc["appearance"],
    cursor: {} as RecordEditDoc["cursor"],
    zooms: [],
    autoZoomDone: true,
    annotations: [],
    audioTracks: [],
    systemGainDb: 0,
    camera: {} as RecordEditDoc["camera"],
    layoutRegions: [],
    transcripts: [],
    captions: { ...CAPTION_DEFAULTS },
    effects,
  }) as RecordEditDoc;

function mount(d: RecordEditDoc, selected: string | null, over: Partial<EffectsTabProps> = {}) {
  const props: EffectsTabProps = {
    doc: d,
    selected,
    onSelect: vi.fn(),
    onAdd: vi.fn(),
    onCommit: vi.fn(),
    onLive: vi.fn(),
    onLiveEnd: vi.fn(),
    clipHasPointer: () => true,
    ...over,
  };
  render(<EffectsTab {...props} />);
  return props;
}

const lastDoc = (fn: unknown) => {
  const calls = (fn as ReturnType<typeof vi.fn>).mock.calls;
  return calls[calls.length - 1][0] as RecordEditDoc;
};

describe("EffectsTab", () => {
  it("every catalogue effect has a name and a description, every parameter a label, in both languages", () => {
    for (const e of EFFECTS) {
      const txt = EFFECT_TEXT[e.id];
      expect(txt, e.id).toBeTruthy();
      for (const cat of [en, de] as Record<string, string>[]) {
        expect(cat[txt.name], e.id).toBeTruthy();
        expect(cat[txt.desc], e.id).toBeTruthy();
        for (const p of e.params) expect(cat[PARAM_LABEL[p.name]], `${e.id}.${p.name}`).toBeTruthy();
      }
    }
  });

  it("the add menu lists the six effects with name and description; picking one adds it", () => {
    const props = mount(doc([]), null);
    expect(screen.queryByTestId("ed-effects-menu")).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "Add effect" }));
    const items = within(screen.getByTestId("ed-effects-menu")).getAllByRole("menuitem");
    expect(items).toHaveLength(6);
    expect(items[0].textContent).toContain("Vignette");
    expect(items[0].textContent).toContain("Darkens the edges of the picture.");
    fireEvent.click(screen.getByRole("menuitem", { name: /Click ripple/ }));
    expect(props.onAdd).toHaveBeenCalledWith("ripple");
    expect(screen.queryByTestId("ed-effects-menu")).toBeNull();
  });

  it("shows a hint without a selection", () => {
    mount(doc([region("vignette")]), null);
    expect(screen.getByTestId("ed-effects-none")).toBeTruthy();
  });

  it("a selected effect shows one control per parameter: slider with the range, colour field", () => {
    const props = mount(doc([region("vignette")]), "fx1");
    const strength = screen.getByRole("slider", { name: "Strength" }) as HTMLInputElement;
    expect([strength.min, strength.max, strength.value]).toEqual(["0", "1", "0.35"]);
    const radius = screen.getByRole("slider", { name: "Radius" }) as HTMLInputElement;
    expect([radius.min, radius.max]).toEqual(["0.2", "1.2"]);
    expect(screen.getAllByRole("slider")).toHaveLength(3);
    const color = screen.getByLabelText("Colour") as HTMLInputElement;
    expect(color.type).toBe("color");
    fireEvent.change(strength, { target: { value: "0.8" } });
    expect(lastDoc(props.onLive).effects?.[0].values).toEqual({ strength: 0.8 });
    fireEvent.pointerUp(strength);
    expect(props.onLiveEnd).toHaveBeenCalled();
    fireEvent.change(color, { target: { value: "#ff0000" } });
    expect(lastDoc(props.onLive).effects?.[0].values).toEqual({ color: "#ff0000" });
  });

  it("a toggle parameter is a switch and one undo step", () => {
    const props = mount(doc([region("grain")]), "fx1");
    const animated = screen.getByRole("checkbox", { name: "Animated" }) as HTMLInputElement;
    expect(animated.checked).toBe(true);
    fireEvent.click(animated);
    expect(lastDoc(props.onCommit).effects?.[0].values).toEqual({ animated: false });
    expect(props.onLive).not.toHaveBeenCalled();
  });

  it("delete asks once inline; Keep leaves it, Delete removes and deselects", () => {
    const props = mount(doc([region("scanlines")]), "fx1");
    fireEvent.click(screen.getByRole("button", { name: "Delete effect" }));
    expect(props.onCommit).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: "Keep" }));
    expect(screen.queryByTestId("ed-effect-confirm")).toBeNull();
    expect(props.onCommit).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: "Delete effect" }));
    fireEvent.click(within(screen.getByTestId("ed-effect-confirm")).getByRole("button", { name: "Delete" }));
    expect(lastDoc(props.onCommit).effects).toEqual([]);
    expect(props.onSelect).toHaveBeenCalledWith(null);
  });

  it("ripple and spotlight say so when the clip has no pointer data; others never", () => {
    for (const id of ["ripple", "spotlight"]) {
      mount(doc([region(id)]), "fx1", { clipHasPointer: () => false });
      expect(screen.getByTestId("ed-effect-no-pointer"), id).toBeTruthy();
      cleanup();
      mount(doc([region(id)]), "fx1", { clipHasPointer: () => true });
      expect(screen.queryByTestId("ed-effect-no-pointer"), id).toBeNull();
      cleanup();
    }
    mount(doc([region("vignette")]), "fx1", { clipHasPointer: () => false });
    expect(screen.queryByTestId("ed-effect-no-pointer")).toBeNull();
  });

  it("the controls carry German labels", () => {
    setCatalog("de", de);
    try {
      mount(doc([region("spotlight")]), "fx1");
      expect(screen.getByRole("slider", { name: "Abdunkeln" })).toBeTruthy();
    } finally {
      setCatalog("en", en);
    }
  });
});
