// @vitest-environment happy-dom
import { afterEach, describe, expect, it, vi } from "vitest";
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import type { RecordAnnotation, RecordAnnotationBody, RecordAnnotationKind, RecordEditDoc } from "../../lib/ipc";
import { TEXT_FONTS, defaultAnnotationBody } from "../../lib/editor";
import { BindingsContext } from "./bindings-context";
import { DEFAULT_BINDINGS, type Bindings } from "../../lib/editor-shortcuts";
import { AnnotationTab, annotationFontCss, registerAnnotationFonts, type AnnotationTabProps } from "./AnnotationTab";
import { CAPTION_DEFAULTS } from "../../lib/editor";

afterEach(cleanup);

const ann = (kind: RecordAnnotationKind, id = "a1", body?: Partial<RecordAnnotationBody>): RecordAnnotation => ({
  id,
  clip: 0,
  startMs: 0,
  endMs: 2_000,
  x: 50,
  y: 50,
  w: 30,
  h: 20,
  zIndex: 1,
  body: { ...defaultAnnotationBody(kind), ...body } as RecordAnnotationBody,
});

const doc = (annotations: RecordAnnotation[]): RecordEditDoc =>
  ({
    version: 3,
    clips: [{ startMs: 0, endMs: 10_000 }],
    speed: [],
    crop: [null],
    output: { aspect: "auto" },
    micGainDb: 0,
    appearance: {} as RecordEditDoc["appearance"],
    cursor: {} as RecordEditDoc["cursor"],
    zooms: [],
    autoZoomDone: true,
    annotations,
    audioTracks: [],
    systemGainDb: 0,
    camera: {
      preset: "no-webcam",
      sizePct: 40,
      mask: "square",
      roundness: null,
      mirror: false,
      reactiveZoom: true,
      anchor: "bottom-right",
      crop: null,
      background: { mode: "none", blur: 0.5, custom: { kind: "wallpaper", id: "01" } },
    },
    layoutRegions: [],
    transcripts: [],
    captions: { ...CAPTION_DEFAULTS },
  }) as RecordEditDoc;

function mount(d: RecordEditDoc, selected: string | null, over: Partial<AnnotationTabProps> = {}, bindings: Bindings = DEFAULT_BINDINGS) {
  const props: AnnotationTabProps = {
    doc: d,
    selected,
    onSelect: vi.fn(),
    onAdd: vi.fn(),
    onCommit: vi.fn(),
    onLive: vi.fn(),
    onLiveEnd: vi.fn(),
    onImport: vi.fn(async () => null),
    projectAssetUrl: (f) => `asset://proj/${f}`,
    ...over,
  };
  render(
    <BindingsContext.Provider value={bindings}>
      <AnnotationTab {...props} />
    </BindingsContext.Provider>,
  );
  return props;
}

const body = (d: RecordEditDoc) => d.annotations[0].body;

describe("AnnotationTab", () => {
  it("the add hint names the bound key, and leaves it out when unassigned", () => {
    mount(doc([]), null);
    expect(document.body.textContent).toContain("lasts 2 s. A adds a text.");
    cleanup();
    mount(doc([]), null, {}, { ...DEFAULT_BINDINGS, addAnnotation: { key: "q" } });
    expect(document.body.textContent).toContain("Q adds a text.");
    cleanup();
    mount(doc([]), null, {}, { ...DEFAULT_BINDINGS, addAnnotation: null });
    expect(document.body.textContent).toContain("lasts 2 s.");
    expect(document.body.textContent).not.toContain("adds a text");
  });

  it("offers all four kinds to add, and a hint when nothing is selected", () => {
    const p = mount(doc([]), null);
    for (const k of ["text", "image", "arrow", "blur"] as const) fireEvent.click(screen.getByTestId(`ed-ann-add-${k}`));
    expect((p.onAdd as ReturnType<typeof vi.fn>).mock.calls.map((c) => c[0])).toEqual(["text", "image", "arrow", "blur"]);
    expect(screen.getByTestId("ed-ann-none")).toBeTruthy();
  });

  it("text: typing moves live and the field's blur is one undo step", () => {
    const p = mount(doc([ann("text")]), "a1");
    const field = screen.getByTestId("ed-ann-content");
    fireEvent.change(field, { target: { value: "Hallo" } });
    expect(body((p.onLive as ReturnType<typeof vi.fn>).mock.calls[0][0])).toMatchObject({ kind: "text", content: "Hallo" });
    expect(p.onCommit).not.toHaveBeenCalled();
    fireEvent.blur(field);
    expect(p.onLiveEnd).toHaveBeenCalledTimes(1);
  });

  it("text: every font is offered in its own face; a pick is one step", () => {
    const p = mount(doc([ann("text")]), "a1");
    const radios = screen.getAllByRole("radio");
    expect(radios.map((r) => r.textContent)).toEqual([...TEXT_FONTS]);
    expect(radios.map((r) => r.style.fontFamily)).toEqual(TEXT_FONTS.map(annotationFontCss));
    expect(radios[0].getAttribute("aria-checked")).toBe("true");
    fireEvent.click(radios[3]);
    expect(body((p.onCommit as ReturnType<typeof vi.fn>).mock.calls[0][0])).toMatchObject({ fontFamily: "Caveat" });
  });

  it("text: size slider runs 8–200 and moves live; style chips toggle", () => {
    const p = mount(doc([ann("text")]), "a1");
    const size = screen.getByLabelText("Size") as HTMLInputElement;
    expect([size.min, size.max, size.value]).toEqual(["8", "200", "32"]);
    fireEvent.change(size, { target: { value: "64" } });
    expect(body((p.onLive as ReturnType<typeof vi.fn>).mock.calls[0][0])).toMatchObject({ fontSize: 64 });
    fireEvent.click(screen.getByText("Italic"));
    expect(body((p.onCommit as ReturnType<typeof vi.fn>).mock.calls[0][0])).toMatchObject({ italic: true, bold: true });
    fireEvent.click(screen.getByText("Typewriter"));
    expect(body((p.onCommit as ReturnType<typeof vi.fn>).mock.calls[1][0])).toMatchObject({ animation: "typewriter" });
  });

  it("arrow: eight directions as a grid around an empty centre", () => {
    const p = mount(doc([ann("arrow")]), "a1");
    const dirs = screen.getAllByRole("radio");
    expect(dirs).toHaveLength(8);
    expect(dirs.find((d) => d.getAttribute("aria-checked") === "true")?.getAttribute("aria-label")).toBe("Right");
    fireEvent.click(screen.getByLabelText("Down left"));
    expect(body((p.onCommit as ReturnType<typeof vi.fn>).mock.calls[0][0])).toMatchObject({ direction: "down-left" });
  });

  it("blur: style picks which slider shows; tint only for the mosaic", () => {
    const mosaic = mount(doc([ann("blur")]), "a1");
    expect(screen.getByLabelText("Block size")).toBeTruthy();
    expect(screen.queryByLabelText("Strength")).toBeNull();
    fireEvent.click(screen.getByText("Free"));
    expect(body((mosaic.onCommit as ReturnType<typeof vi.fn>).mock.calls[0][0])).toMatchObject({ shape: "freehand" });
    cleanup();
    mount(doc([ann("blur", "a1", { style: "blur" } as Partial<RecordAnnotationBody>)]), "a1");
    const strength = screen.getByLabelText("Strength") as HTMLInputElement;
    expect([strength.min, strength.max]).toEqual(["2", "40"]);
    expect((screen.getByText("Black") as HTMLButtonElement).disabled).toBe(true);
  });

  it("image: a picked file keeps the annotation's id and builds on the newest document", async () => {
    const rendered = doc([ann("image")]);
    // While the file dialog was open another edit landed (a second annotation).
    const latest = doc([ann("image"), ann("arrow", "a2")]);
    const p = mount(rendered, "a1", { onImport: vi.fn(async () => "1234.png"), getDoc: () => latest });
    await act(async () => {
      fireEvent.click(screen.getByTestId("ed-ann-pick-image"));
    });
    const next = (p.onCommit as ReturnType<typeof vi.fn>).mock.calls[0][0] as RecordEditDoc;
    expect(next.annotations.map((a) => a.id)).toEqual(["a1", "a2"]);
    expect(next.annotations[0].body).toEqual({ kind: "image", file: "1234.png" });
    expect(p.onSelect).not.toHaveBeenCalled();
  });

  it("the picker's fallbacks match each face's kind (Caveat is handwriting)", () => {
    expect(annotationFontCss("Caveat")).toBe('"IR Caveat", cursive');
    expect(annotationFontCss("Lora")).toBe('"IR Lora", serif');
    expect(annotationFontCss("IBM Plex Mono")).toBe('"IR IBM Plex Mono", monospace');
    expect(annotationFontCss("Oswald")).toBe('"IR Oswald", sans-serif');
  });

  it("the fonts are registered once per window, however often the editor remounts", () => {
    const added: string[] = [];
    class FakeFace {
      constructor(public family: string, public src: string) {}
      load() {
        return Promise.resolve(this);
      }
    }
    vi.stubGlobal("FontFace", FakeFace);
    const fonts = (document as unknown as { fonts?: unknown }).fonts;
    Object.defineProperty(document, "fonts", { configurable: true, value: { add: (f: FakeFace) => added.push(f.src) } });
    try {
      const list = [{ family: "Inter", regular: "/r/Inter-Regular.ttf", bold: "/r/Inter-Bold.ttf" }];
      registerAnnotationFonts(list, (p) => `asset://${p}`);
      registerAnnotationFonts(list, (p) => `asset://${p}`);
      expect(added).toEqual(['url("asset:///r/Inter-Regular.ttf")', 'url("asset:///r/Inter-Bold.ttf")']);
    } finally {
      Object.defineProperty(document, "fonts", { configurable: true, value: fonts });
      vi.unstubAllGlobals();
    }
  });

  it("delete removes the annotation and clears the selection", () => {
    const p = mount(doc([ann("arrow"), ann("text", "a2")]), "a1");
    fireEvent.click(screen.getByText("Delete annotation"));
    expect(((p.onCommit as ReturnType<typeof vi.fn>).mock.calls[0][0] as RecordEditDoc).annotations.map((a) => a.id)).toEqual(["a2"]);
    expect(p.onSelect).toHaveBeenCalledWith(null);
  });
});
