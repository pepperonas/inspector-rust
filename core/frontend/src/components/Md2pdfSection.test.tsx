import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent, waitFor } from "@testing-library/react";

const footer = { template: "", default_template: "© {year} {user}", preview: "© 2026 Ada" };
const ipc = vi.hoisted(() => ({ getMd2pdfFooter: vi.fn(), setMd2pdfFooter: vi.fn() }));
vi.mock("../lib/ipc", () => ipc);

import { Md2pdfSection } from "./Md2pdfSection";

describe("Md2pdfSection", () => {
  beforeEach(() => {
    ipc.getMd2pdfFooter.mockReset().mockResolvedValue(footer);
    ipc.setMd2pdfFooter.mockReset().mockImplementation(async (template: string) => ({
      ...footer,
      template: template.trim(),
      preview: template.trim() ? "© 2026 Ada Lovelace | example.org" : footer.preview,
    }));
  });

  it("shows the default as placeholder and what will be printed", async () => {
    render(<Md2pdfSection />);
    await waitFor(() => expect(screen.getByTestId("md2pdf-footer-preview").textContent).toContain("© 2026 Ada"));
    expect((screen.getByTestId("md2pdf-footer") as HTMLInputElement).placeholder).toBe("© {year} {user}");
    // Nothing stored: no reset button.
    expect(screen.queryByText("Default")).toBeNull();
  });

  it("saves on Enter, shows the new line, and resets to the default", async () => {
    render(<Md2pdfSection />);
    const field = (await screen.findByTestId("md2pdf-footer")) as HTMLInputElement;
    await waitFor(() => expect(ipc.getMd2pdfFooter).toHaveBeenCalled());
    fireEvent.change(field, { target: { value: "© {year} Ada Lovelace | example.org" } });
    fireEvent.keyDown(field, { key: "Enter" });
    await waitFor(() => expect(ipc.setMd2pdfFooter).toHaveBeenCalledWith("© {year} Ada Lovelace | example.org"));
    await waitFor(() => expect(screen.getByTestId("md2pdf-footer-preview").textContent).toContain("Ada Lovelace | example.org"));
    fireEvent.click(screen.getByText("Default"));
    await waitFor(() => expect(ipc.setMd2pdfFooter).toHaveBeenLastCalledWith(""));
    await waitFor(() => expect(field.value).toBe(""));
  });

  it("an unchanged field is not saved again", async () => {
    render(<Md2pdfSection />);
    const field = await screen.findByTestId("md2pdf-footer");
    await waitFor(() => expect(ipc.getMd2pdfFooter).toHaveBeenCalled());
    await waitFor(() => expect(screen.getByTestId("md2pdf-footer-preview")).toBeTruthy());
    fireEvent.blur(field);
    expect(ipc.setMd2pdfFooter).not.toHaveBeenCalled();
  });
});
