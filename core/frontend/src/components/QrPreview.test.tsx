import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { QrPreview } from "./QrPreview";
import { qrSave } from "../lib/ipc";

vi.mock("../lib/ipc", () => ({ qrSave: vi.fn() }));
// The real encoder runs; only the browser canvas boundary is replaced.
vi.mock("../lib/qr", async (original) => ({
  ...(await original<typeof import("../lib/qr")>()),
  drawQr: vi.fn(),
  qrPngBase64: () => "png-payload",
}));
beforeEach(() => { vi.mocked(qrSave).mockReset(); });
afterEach(cleanup);

describe("QR exports", () => {
  it("saves PNG and reports the actual destination", async () => {
    vi.mocked(qrSave).mockResolvedValue("/Downloads/qr-test.png");
    render(<QrPreview text="https://example.com" />);
    fireEvent.click(screen.getByText("Save PNG"));
    await waitFor(() =>
      expect(screen.getByRole("status").textContent).toContain("/Downloads/qr-test.png"),
    );
    expect(qrSave).toHaveBeenCalledWith({ pngB64: "png-payload" });
  });

  it("exports the QR matrix for STL and explains the colour change", async () => {
    vi.mocked(qrSave).mockResolvedValue("/Downloads/qr-test.stl");
    render(<QrPreview text="hello" />);
    fireEvent.click(screen.getByText("Save STL"));
    await screen.findByRole("status");
    const output = vi.mocked(qrSave).mock.calls[0][0];
    expect("matrix" in output && output.matrix.length).toBe(21);
    expect(screen.getByText(/switch to dark filament at 2 mm/)).toBeTruthy();
  });

  it("shows write failures and allows retry", async () => {
    vi.mocked(qrSave).mockRejectedValue(new Error("Disk full"));
    render(<QrPreview text="hello" />);
    fireEvent.click(screen.getByText("Save PNG"));
    expect((await screen.findByRole("alert")).textContent).toContain("Disk full");
    expect((screen.getByText("Save PNG") as HTMLButtonElement).disabled).toBe(false);
  });

  it("removes the canvas and disables exports for invalid content", () => {
    render(<QrPreview text={"x".repeat(2332)} />);
    expect(screen.queryByLabelText("QR code preview")).toBeNull();
    expect(screen.getByRole("alert").textContent).toContain("too long");
    expect((screen.getByText("Save STL") as HTMLButtonElement).disabled).toBe(true);
    expect((screen.getByText("Save PNG") as HTMLButtonElement).disabled).toBe(true);
  });
});
