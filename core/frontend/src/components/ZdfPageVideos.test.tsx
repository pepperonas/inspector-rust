import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent, waitFor } from "@testing-library/react";

const ipc = vi.hoisted(() => ({
  zdfPageVideos: vi.fn(),
  socialDownload: vi.fn(),
  revealPath: vi.fn(() => Promise.resolve()),
  setSuppressHide: vi.fn(() => Promise.resolve()),
}));
vi.mock("../lib/ipc", () => ipc);
vi.mock("./SocialMeta", () => ({ useMeta: () => ({ state: "ok", meta: { duration_s: 168 } }) }));

import { ZdfPageVideos } from "./ZdfPageVideos";

const V = (id: string, title: string) => ({
  id,
  url: `https://www.zdfheute.de/api/video/${id}.m3u8`,
  title,
  description: `about ${title}`,
  thumbnail: null,
});
const PAGE = "https://www.zdfheute.de/wirtschaft/x-100.html";

describe("ZdfPageVideos", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    ipc.revealPath.mockResolvedValue(undefined);
    ipc.setSuppressHide.mockResolvedValue(undefined);
  });

  it("stays hidden when the page holds only the lead video", async () => {
    ipc.zdfPageVideos.mockResolvedValue([V("a-100", "A")]);
    const { container } = render(<ZdfPageVideos url={PAGE} />);
    await waitFor(() => expect(ipc.zdfPageVideos).toHaveBeenCalledWith(PAGE));
    await waitFor(() => expect(container.querySelector("[data-testid=zdf-page-videos]")).toBeNull());
    expect(screen.queryByText(/Looking for/)).toBeNull();
  });

  it("downloads every video in order, named by the page, revealing once", async () => {
    ipc.zdfPageVideos.mockResolvedValue([V("a-100", "Lead"), V("b-100", "Interview"), V("c-100", "Teaser")]);
    ipc.socialDownload.mockImplementation((u: string) =>
      u.includes("b-100") ? Promise.reject(new Error("boom")) : Promise.resolve(`/d/${u.slice(-12)}.mp4`),
    );
    render(<ZdfPageVideos url={PAGE} />);
    fireEvent.click(await screen.findByText(/Download all 3/));
    await waitFor(() => expect(ipc.socialDownload).toHaveBeenCalledTimes(3));
    expect(ipc.socialDownload.mock.calls.map((c) => [c[0], c[2], c[4]])).toEqual([
      ["https://www.zdfheute.de/api/video/a-100.m3u8", false, "Lead"],
      ["https://www.zdfheute.de/api/video/b-100.m3u8", false, "Interview"],
      ["https://www.zdfheute.de/api/video/c-100.m3u8", false, "Teaser"],
    ]);
    // A failure in the middle does not stop the run; Finder opens once.
    await waitFor(() => expect(ipc.revealPath).toHaveBeenCalledTimes(1));
    expect(screen.getByText("boom")).toBeTruthy();
    expect(ipc.setSuppressHide).toHaveBeenLastCalledWith(false);
  });

  it("a row downloads only its own video", async () => {
    ipc.zdfPageVideos.mockResolvedValue([V("a-100", "Lead"), V("b-100", "Interview")]);
    ipc.socialDownload.mockResolvedValue("/d/x.mp4");
    render(<ZdfPageVideos url={PAGE} />);
    const rows = await screen.findAllByTitle("Download this video");
    fireEvent.click(rows[1]);
    await waitFor(() => expect(ipc.socialDownload).toHaveBeenCalledTimes(1));
    expect(ipc.socialDownload.mock.calls[0][4]).toBe("Interview");
  });
});
