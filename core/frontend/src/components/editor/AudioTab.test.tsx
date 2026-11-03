// @vitest-environment happy-dom
import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import type { RecordAudioTrack, RecordEditDoc } from "../../lib/ipc";
import { AudioTab, VoiceoverPanel, snap, voiceoverMeter, type AudioTabProps } from "./AudioTab";

afterEach(cleanup);

const track = (over: Partial<RecordAudioTrack> = {}): RecordAudioTrack => ({
  id: "m1",
  kind: "music",
  file: "f00d.m4a",
  clip: 0,
  startMs: 0,
  durationMs: 30_000,
  offsetMs: 0,
  spanMs: null,
  gainDb: -18,
  looped: false,
  fadeInMs: 1_000,
  fadeOutMs: 1_000,
  muted: false,
  label: "Calm Piano",
  ...over,
});

const doc = (audioTracks: RecordAudioTrack[] = [], systemGainDb = 0): RecordEditDoc =>
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
    annotations: [],
    audioTracks,
    systemGainDb,
    camera: {} as RecordEditDoc["camera"],
    layoutRegions: [],
  }) as RecordEditDoc;

function mount(d: RecordEditDoc, selected: string | null = null, over: Partial<AudioTabProps> = {}) {
  const props: AudioTabProps = {
    doc: d,
    hasMic: true,
    selected,
    onSelect: vi.fn(),
    onCommit: vi.fn(),
    onLive: vi.fn(),
    onLiveEnd: vi.fn(),
    onAddMusic: vi.fn(),
    onVoiceover: vi.fn(),
    ...over,
  };
  render(<AudioTab {...props} />);
  return props;
}

const slider = (name: string) => screen.getByLabelText(name) as HTMLInputElement;
const last = <T,>(a: T[]): T | undefined => a[a.length - 1];
const lastLive = (p: AudioTabProps) => last((p.onLive as ReturnType<typeof vi.fn>).mock.calls)?.[0] as RecordEditDoc;

describe("AudioTab", () => {
  it("snaps levels to 0.5 dB and fades to 50 ms, inside their ranges", () => {
    expect(snap(3.3, 0.5, -12, 12)).toBe(3.5);
    expect(snap(-14, 0.5, -12, 12)).toBe(-12);
    expect(snap(1_234, 50, 0, 5_000)).toBe(1_250);
    expect(snap(9_000, 50, 0, 5_000)).toBe(5_000);
  });

  it("the system sound level moves live in ±12 dB and is one step on release", () => {
    const p = mount(doc());
    const s = slider("System sound");
    expect([s.min, s.max, s.step]).toEqual(["-12", "12", "0.5"]);
    fireEvent.change(s, { target: { value: "6.5" } });
    expect(lastLive(p).systemGainDb).toBe(6.5);
    fireEvent.pointerUp(s);
    expect(p.onLiveEnd).toHaveBeenCalled();
  });

  it("the mic level shows only with a microphone recording", () => {
    mount(doc());
    expect(slider("Mic").step).toBe("0.5");
    cleanup();
    mount(doc(), null, { hasMic: false });
    expect(screen.queryByLabelText("Mic")).toBeNull();
  });

  it("adds music and records a voiceover from its buttons", () => {
    const p = mount(doc());
    fireEvent.click(screen.getByTestId("ed-add-music"));
    fireEvent.click(screen.getByTestId("ed-voiceover"));
    expect(p.onAddMusic).toHaveBeenCalledTimes(1);
    expect(p.onVoiceover).toHaveBeenCalledTimes(1);
  });

  it("lists the tracks; a click selects one", () => {
    const p = mount(doc([track(), track({ id: "v1", kind: "voiceover", label: "", file: "voiceover-1.wav" })]));
    fireEvent.click(screen.getByText("voiceover-1"));
    expect(p.onSelect).toHaveBeenCalledWith("v1");
    expect(screen.queryByTestId("ed-audio-track-settings")).toBeNull();
  });

  it("edits the selected track: level, fades, loop, mute, name, delete", () => {
    const p = mount(doc([track()]), "m1");
    fireEvent.change(slider("Level"), { target: { value: "-6.7" } });
    expect(lastLive(p).audioTracks[0].gainDb).toBe(-6.5);
    fireEvent.change(slider("Fade in"), { target: { value: "1730" } });
    expect(lastLive(p).audioTracks[0].fadeInMs).toBe(1_750);
    fireEvent.change(slider("Fade out"), { target: { value: "0" } });
    expect(lastLive(p).audioTracks[0].fadeOutMs).toBe(0);
    const commits = (p.onCommit as ReturnType<typeof vi.fn>).mock.calls;
    fireEvent.click(screen.getByLabelText("Loop"));
    expect(last(commits)?.[0].audioTracks[0].looped).toBe(true);
    fireEvent.click(screen.getByLabelText("Mute"));
    expect(last(commits)?.[0].audioTracks[0].muted).toBe(true);
    const name = screen.getByLabelText("Name");
    fireEvent.change(name, { target: { value: "  Intro  " } });
    fireEvent.blur(name);
    expect(last(commits)?.[0].audioTracks[0].label).toBe("Intro");
    fireEvent.click(screen.getByText("Delete track"));
    expect(last(commits)?.[0].audioTracks).toEqual([]);
    expect(p.onSelect).toHaveBeenLastCalledWith(null);
  });

  it("builds on the newest document, not the rendered one", () => {
    const newer = doc([track({ gainDb: -3 })], 4);
    const p = mount(doc([track()]), "m1", { getDoc: () => newer });
    fireEvent.click(screen.getByLabelText("Mute"));
    const sent = (p.onCommit as ReturnType<typeof vi.fn>).mock.calls[0][0] as RecordEditDoc;
    expect(sent.systemGainDb).toBe(4);
    expect(sent.audioTracks[0]).toMatchObject({ gainDb: -3, muted: true });
  });
});

describe("VoiceoverPanel", () => {
  it("shows the level and the time; Stop and Discard", () => {
    const onStop = vi.fn();
    const onCancel = vi.fn();
    render(<VoiceoverPanel take={{ phase: "recording", rms: 0.1, elapsedMs: 2_500, maxMs: 6_000 }} onStop={onStop} onCancel={onCancel} />);
    expect(screen.getByTestId("ed-vo-meter").style.transform).toBe(`scaleX(${voiceoverMeter(0.1)})`);
    expect(voiceoverMeter(0.1)).toBeCloseTo(2 / 3, 3);
    fireEvent.click(screen.getByTestId("ed-vo-stop"));
    fireEvent.click(screen.getByTestId("ed-vo-cancel"));
    expect(onStop).toHaveBeenCalledTimes(1);
    expect(onCancel).toHaveBeenCalledTimes(1);
  });

  it("Stop waits until the take has started", () => {
    render(<VoiceoverPanel take={{ phase: "starting", rms: 0, elapsedMs: 0, maxMs: 0 }} onStop={vi.fn()} onCancel={vi.fn()} />);
    expect((screen.getByTestId("ed-vo-stop") as HTMLButtonElement).disabled).toBe(true);
    expect((screen.getByTestId("ed-vo-cancel") as HTMLButtonElement).disabled).toBe(false);
  });
});
