import { describe, expect, it } from "vitest";
import { CAPTION_DEFAULTS } from "./editor";
import type { RecordEditDoc, RecordTranscript, RecordTranscriptWord } from "./ipc";
import {
  CUT_BREATH_MS,
  type ClipWord,
  breathe,
  carryOverEdits,
  clipWords,
  cutWords,
  gaps,
  setWordText,
} from "./transcript";

const w = (id: string, startMs: number, endMs: number, text = id): RecordTranscriptWord => ({ id, startMs, endMs, text });
const cw = (id: string, startMs: number, endMs: number): ClipWord => ({
  id: `0:${id}`,
  wordId: id,
  startMs,
  endMs,
  text: id,
  edited: false,
});

// Words of the own take: a 0–300, b 400–700, (pause) c 1500–1800, d 1850–2100, e 3000–3300.
const WORDS = [w("a", 0, 300), w("b", 400, 700), w("c", 1500, 1800), w("d", 1850, 2100), w("e", 3000, 3300)];

const doc = (over: Partial<RecordEditDoc> = {}): RecordEditDoc =>
  ({
    version: 5,
    clips: [{ startMs: 0, endMs: 10_000 }],
    speed: [],
    crop: [null],
    zooms: [],
    annotations: [],
    audioTracks: [],
    layoutRegions: [],
    transcripts: [{ source: null, lang: "de", words: WORDS }],
    captions: CAPTION_DEFAULTS,
    ...over,
  }) as RecordEditDoc;

describe("breathe", () => {
  it("breathe_keeps_70ms_next_to_kept_words", () => {
    // Cut c..d (1500–2100) between kept b (ends 700) and e (starts 3000): long pauses both sides.
    const [s, e] = breathe(1500, 2100, [cw("b", 400, 700), cw("e", 3000, 3300)]);
    expect(s).toBe(700 + CUT_BREATH_MS);
    expect(e).toBe(3000 - CUT_BREATH_MS);
  });

  it("breathe_stops_at_the_gap_midpoint", () => {
    // Pause of 100 ms before, 60 ms after: half of each is the most that stays.
    const [s, e] = breathe(800, 1000, [cw("x", 500, 700), cw("y", 1060, 1200)]);
    expect(s).toBe(750);
    expect(e).toBe(1030);
  });

  it("breathe_leaves_edges_inside_words", () => {
    // Both edges inside kept words, and no kept word on the far side of the next case.
    expect(breathe(550, 1100, [cw("x", 500, 700), cw("y", 1060, 1200)])).toEqual([550, 1100]);
    expect(breathe(800, 1000, [])).toEqual([800, 1000]);
  });
});

describe("cutWords", () => {
  it("cut_words_selection_spans_first_to_last", () => {
    // Selecting b and d also cuts c; breath against a (ends 300) and e (starts 3000).
    const out = cutWords(doc(), 0, ["b", "0:d"]);
    expect(out.clips).toEqual([
      { startMs: 0, endMs: 300 + 50 },
      { startMs: 3000 - CUT_BREATH_MS, endMs: 10_000 },
    ]);
    expect(cutWords(doc(), 0, ["nope"])).toEqual(doc());
  });

  it("cut_words_merges_cuts_under_two_frames", () => {
    // Clip starts 40 ms before word c: the cut snaps onto the clip edge (no 40-ms sliver).
    const d = doc({ clips: [{ startMs: 1460, endMs: 10_000 }] });
    const out = cutWords(d, 0, ["c"]);
    expect(out.clips).toEqual([{ startMs: 1850 - 25, endMs: 10_000 }]);
  });
});

describe("clipWords / gaps", () => {
  it("clip_words_only_inside_the_clip_window", () => {
    const d = doc({ clips: [{ startMs: 350, endMs: 2000 }], crop: [null] });
    // d (1850–2100) has its middle at 1975 → inside; a (middle 150) outside.
    expect(clipWords(d, d.transcripts, 0).map((x) => x.id)).toEqual(["0:b", "0:c", "0:d"]);
    const other = doc({ clips: [{ startMs: 0, endMs: 10_000, source: "take-2" }] });
    expect(clipWords(other, other.transcripts, 0)).toEqual([]);
  });

  it("gaps_list_the_cut_words_between_neighbours", () => {
    const cut = cutWords(doc(), 0, ["c", "d"]);
    expect(cut.clips.length).toBe(2);
    expect(gaps(cut, cut.transcripts, 0)).toEqual([
      {
        afterClip: 0,
        words: [
          { id: "0:c", wordId: "c", startMs: 1500, endMs: 1800, text: "c", edited: false },
          { id: "0:d", wordId: "d", startMs: 1850, endMs: 2100, text: "d", edited: false },
        ],
      },
    ]);
    expect(gaps(cut, cut.transcripts, 1)).toEqual([]);
  });
});

describe("word text", () => {
  it("set_word_text_keeps_and_clears_original", () => {
    const d1 = setWordText(doc(), null, "b", "  bee ");
    expect(d1.transcripts[0].words[1]).toEqual({ id: "b", startMs: 400, endMs: 700, text: "bee", originalText: "b" });
    const d2 = setWordText(d1, null, "b", "bea");
    expect(d2.transcripts[0].words[1].originalText).toBe("b");
    const d3 = setWordText(d2, null, "b", "b");
    expect(d3.transcripts[0].words[1]).toEqual({ id: "b", startMs: 400, endMs: 700, text: "b" });
    expect(clipWords(d1, d1.transcripts, 0)[1].edited).toBe(true);
    const same = doc();
    expect(setWordText(same, null, "b", "b")).toBe(same);
    expect(setWordText(same, "take-2", "b", "x")).toBe(same);
  });

  it("carry_over_keeps_user_corrections_on_overlap", () => {
    const old: RecordTranscript = {
      source: null,
      lang: "de",
      words: [{ ...w("o1", 400, 700, "Rust"), originalText: "rost" }, { ...w("o2", 5000, 5300, "Tauri"), originalText: "tory" }],
    };
    const next: RecordTranscript = {
      source: null,
      lang: "de",
      words: [w("n1", 420, 690, "rost"), w("n2", 6000, 6300, "tory"), w("n3", 900, 1000, "rost")],
    };
    const out = carryOverEdits(old, next);
    expect(out.words[0]).toEqual({ id: "n1", startMs: 420, endMs: 690, text: "Rust", originalText: "rost" });
    // No overlap (n2) or the old correction already used: kept as recognized.
    expect(out.words[1]).toEqual(w("n2", 6000, 6300, "tory"));
    expect(out.words[2]).toEqual(w("n3", 900, 1000, "rost"));
  });
});
