import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, cleanup, fireEvent, waitFor, screen } from "@testing-library/react";

// The queue is the whole feature, so the IPC is stubbed and what matters is
// HOW it gets called: once per link, never in parallel, `reveal: false` on
// every one, and Finder raised exactly once at the end.
const socialDownload = vi.fn<(url: string, mode: string, reveal?: boolean) => Promise<string>>();
const revealPath = vi.fn<(p: string) => Promise<void>>(async () => undefined);
const setSuppressHide = vi.fn<(s: boolean) => Promise<void>>(async () => undefined);
vi.mock("../lib/ipc", () => ({
  socialDownload: (u: string, m: string, r?: boolean) => socialDownload(u, m, r),
  revealPath: (p: string) => revealPath(p),
  setSuppressHide: (s: boolean) => setSuppressHide(s),
}));

import { LinkGrabber } from "./LinkGrabber";

const YT_A = "https://youtu.be/aaa";
const YT_B = "https://youtu.be/bbb";
const YT_C = "https://youtu.be/ccc";

const paste = (text: string) => {
  const box = screen.getByRole("textbox");
  fireEvent.change(box, { target: { value: text } });
};
const start = () => fireEvent.click(screen.getByRole("button", { name: /Download/i }));

beforeEach(() => {
  socialDownload.mockReset();
  revealPath.mockReset();
  setSuppressHide.mockReset();
  socialDownload.mockImplementation(async (u) => `/Users/x/Downloads/${u.split("/").pop()}.mp4`);
});
afterEach(cleanup);

describe("LinkGrabber", () => {
  it("downloads every pasted link, deduplicated", async () => {
    render(<LinkGrabber />);
    paste(`${YT_A}\n${YT_B}\n${YT_A}`);
    start();
    await waitFor(() => expect(socialDownload).toHaveBeenCalledTimes(2));
    expect(socialDownload.mock.calls.map((c) => c[0])).toEqual([YT_A, YT_B]);
  });

  it("a failing link does not stop the queue", async () => {
    // One dead link in a batch is normal. If it aborted the run, the feature
    // would be useless for exactly the lists people actually paste.
    socialDownload.mockImplementation(async (u) => {
      if (u === YT_B) throw new Error("HTTP 410");
      return `/d/${u.slice(-3)}.mp4`;
    });
    render(<LinkGrabber />);
    paste([YT_A, YT_B, YT_C].join("\n"));
    start();
    await waitFor(() => expect(socialDownload).toHaveBeenCalledTimes(3));
    await waitFor(() => expect(screen.getByText(/2 \/ 3 done/)).toBeTruthy());
    // ⚠️ "1 failed" appears in the counter AND on the retry button — match the
    // button, which is the unambiguous one.
    expect(screen.getByRole("button", { name: /Retry 1 failed/ })).toBeTruthy();
  });

  it("never reveals per file, and raises Finder exactly once at the end", async () => {
    // Each reveal steals focus, and a focus loss hides the popup — which would
    // unmount the component driving the run.
    render(<LinkGrabber />);
    paste(`${YT_A} ${YT_B}`);
    start();
    await waitFor(() => expect(revealPath).toHaveBeenCalledTimes(1));
    expect(socialDownload.mock.calls.every((c) => c[2] === false)).toBe(true);
  });

  it("does not raise Finder when nothing succeeded", async () => {
    socialDownload.mockImplementation(async () => {
      throw new Error("nope");
    });
    render(<LinkGrabber />);
    paste(YT_A);
    start();
    await waitFor(() => expect(screen.getByText(/0 \/ 1 done/)).toBeTruthy());
    expect(revealPath).not.toHaveBeenCalled();
  });

  it("pins the popup for the run and releases it afterwards", async () => {
    render(<LinkGrabber />);
    paste(YT_A);
    start();
    await waitFor(() => expect(setSuppressHide).toHaveBeenCalledWith(true));
    await waitFor(() => expect(setSuppressHide).toHaveBeenCalledWith(false));
  });

  it("retry re-runs only the failures", async () => {
    socialDownload.mockImplementation(async (u) => {
      if (u === YT_B) throw new Error("HTTP 410");
      return `/d/${u.slice(-3)}.mp4`;
    });
    render(<LinkGrabber />);
    paste(`${YT_A}\n${YT_B}`);
    start();
    await waitFor(() => expect(screen.getByText(/Retry 1 failed/)).toBeTruthy());
    socialDownload.mockReset();
    socialDownload.mockImplementation(async () => "/d/ok.mp4");
    fireEvent.click(screen.getByRole("button", { name: /Retry 1 failed/ }));
    await waitFor(() => expect(socialDownload).toHaveBeenCalledTimes(1));
    expect(socialDownload.mock.calls[0][0]).toBe(YT_B);
  });

  it("offers audio only when every link is YouTube", async () => {
    // The single-link bar promises audio for YouTube only; two different
    // answers to one question would be worse than a missing option.
    const { rerender } = render(<LinkGrabber />);
    paste(`${YT_A} https://vm.tiktok.com/x`);
    expect(screen.queryByRole("button", { name: /audio/i })).toBeNull();
    rerender(<LinkGrabber />);
    paste(`${YT_A} ${YT_B}`);
    await waitFor(() => expect(screen.getByRole("button", { name: /audio/i })).toBeTruthy());
  });

  it("says nothing is queued before a link is pasted", () => {
    render(<LinkGrabber />);
    expect(screen.getByText(/No links yet/)).toBeTruthy();
  });
});
