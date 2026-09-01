import { describe, it, expect, vi, afterEach } from "vitest";
import { render, screen, cleanup } from "@testing-library/react";

// The overlay talks IPC on mount (entry list + live codes + suppress-hide)
// and registers the Tauri drag-drop listener — all stubbed so the component
// renders standalone. Empty-list stubs keep the List tab on its empty state.
vi.mock("../lib/ipc", () => ({
  setSuppressHide: vi.fn(async () => undefined),
  totpAdd: vi.fn(async () => undefined),
  totpCurrentCodesAll: vi.fn(async () => []),
  totpDelete: vi.fn(async () => undefined),
  totpDeleteAll: vi.fn(async () => 0),
  totpExport: vi.fn(async () => ""),
  totpImport: vi.fn(async () => ({ added: 0 })),
  totpImportFile: vi.fn(async () => ({ added: 0 })),
  totpList: vi.fn(async () => []),
  totpRemoveDuplicates: vi.fn(async () => 0),
  totpSetOrder: vi.fn(async () => undefined),
  totpUpdate: vi.fn(async () => undefined),
}));
vi.mock("@tauri-apps/api/webviewWindow", () => ({
  getCurrentWebviewWindow: () => ({
    onDragDropEvent: async () => () => undefined,
  }),
}));

import { TotpOverlay } from "./TotpOverlay";

afterEach(cleanup);

const SECRET_PLACEHOLDER = "JBSW Y3DP EHPK 3PXP";

describe("TotpOverlay initialTab / initialIssuer (2fa add, v0.104.0)", () => {
  it("opens on the List tab by default", async () => {
    render(<TotpOverlay onExit={() => undefined} />);
    expect(await screen.findByText("No 2FA entries yet.")).toBeTruthy();
    // The Add form is NOT showing.
    expect(screen.queryByPlaceholderText(SECRET_PLACEHOLDER)).toBeNull();
  });

  it('initialTab="add" opens straight on the Add form', async () => {
    render(<TotpOverlay onExit={() => undefined} initialTab="add" />);
    expect(await screen.findByText("Issuer / Service")).toBeTruthy();
    expect(screen.getByPlaceholderText(SECRET_PLACEHOLDER)).toBeTruthy();
    expect(screen.queryByText("No 2FA entries yet.")).toBeNull();
  });

  it("initialIssuer pre-fills the Issuer field", async () => {
    render(
      <TotpOverlay onExit={() => undefined} initialTab="add" initialIssuer="GitHub" />,
    );
    const issuer = (await screen.findByPlaceholderText(
      "Amazon, GitHub, …",
    )) as HTMLInputElement;
    expect(issuer.value).toBe("GitHub");
  });

  it("with a pre-filled issuer, focus starts on the Account field", async () => {
    render(
      <TotpOverlay onExit={() => undefined} initialTab="add" initialIssuer="GitHub" />,
    );
    const account = await screen.findByPlaceholderText("user@example.com");
    expect(document.activeElement).toBe(account);
  });

  it("without a pre-fill, focus starts on the Issuer field", async () => {
    render(<TotpOverlay onExit={() => undefined} initialTab="add" />);
    const issuer = await screen.findByPlaceholderText("Amazon, GitHub, …");
    expect(document.activeElement).toBe(issuer);
  });
});

describe("TotpOverlay brand icons (v0.161.0)", () => {
  it("a known issuer renders its brand SVG, an unknown one a monogram", async () => {
    const ipc = await import("../lib/ipc");
    (ipc.totpList as ReturnType<typeof vi.fn>).mockResolvedValueOnce([
      { id: 1, issuer: "GitHub", account: "martin", digits: 6, period: 30, algorithm: "SHA1", sort_order: 0 },
      { id: 2, issuer: "Zzz Unbekannt", account: "", digits: 6, period: 30, algorithm: "SHA1", sort_order: 1 },
    ]);
    render(<TotpOverlay onExit={() => undefined} />);
    // The icon chunk loads lazily — the brand SVG appears once it lands.
    const svg = await screen.findByRole("img", { name: "GitHub" }, { timeout: 4000 });
    expect(svg.tagName.toLowerCase()).toBe("svg");
    // fill as a PRESENTATION ATTRIBUTE (survives stylesheet/CSP loss — the
    // icon-catalogue hardening lesson), on the svg AND its path.
    expect(svg.getAttribute("fill")).toBe("#181717");
    expect(svg.querySelector("path")?.getAttribute("fill")).toBe("#181717");
    // The unknown issuer gets its deterministic monogram, never a guess.
    expect(await screen.findByText("Z")).toBeTruthy();
  });
});
