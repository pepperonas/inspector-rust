import { describe, it, expect, vi, afterEach } from "vitest";
import { render, screen, cleanup, fireEvent, act } from "@testing-library/react";

const { writeText } = vi.hoisted(() => ({ writeText: vi.fn(async () => undefined) }));
vi.mock("@tauri-apps/plugin-clipboard-manager", () => ({ writeText }));

import { ConvertPanel } from "./ConvertPanel";
import { parseConvertArg } from "../lib/convert-cmd";
import type { FxRates } from "../lib/ipc";

const FX: FxRates = {
  eur_per: { EUR: 1, USD: 1 / 1.1712, BTC: 73319 },
  ecb_date: "2026-09-25",
  ecb_fetched_ms: 1,
  crypto_fetched_ms: 1,
  stale: false,
  error: null,
};

function mount(arg: string, over: Partial<Parameters<typeof ConvertPanel>[0]> = {}) {
  const props = {
    parse: parseConvertArg(arg),
    rates: FX.eur_per,
    fx: FX,
    fxLoading: false,
    focused: false,
    onArgChange: vi.fn(),
    onPick: vi.fn(),
    onRefreshRates: vi.fn(),
    onExit: vi.fn(),
    ...over,
  };
  render(<ConvertPanel {...props} />);
  return props;
}

const units = () =>
  [...document.querySelectorAll<HTMLElement>("[data-unit]")].map((el) => el.dataset.unit);

afterEach(() => {
  cleanup();
  writeText.mockClear();
});

describe("ConvertPanel", () => {
  it("shows the whole category of the typed unit, value converted", () => {
    mount("165 mph");
    expect(units()).toEqual(["kmh", "mph", "mps", "kn", "fps"]);
    expect(document.querySelector('[data-unit="kmh"]')!.textContent).toContain("265,5418");
    expect(screen.getByRole("tab", { name: /Geschwindigkeit/ }).getAttribute("aria-selected")).toBe("true");
  });

  it("the value field writes back into the search bar", () => {
    const p = mount("165 mph");
    fireEvent.change(screen.getByLabelText("Wert"), { target: { value: "200" } });
    expect(p.onArgChange).toHaveBeenCalledWith("200 mph");
  });

  it("a target select writes `in <ziel>`", () => {
    const p = mount("165 mph");
    fireEvent.change(screen.getByLabelText("Ziel"), { target: { value: "kn" } });
    expect(p.onArgChange).toHaveBeenCalledWith("165 mph in kn");
  });

  it("a category chip switches to its default unit, keeping the number", () => {
    const p = mount("165 mph");
    fireEvent.click(screen.getByRole("tab", { name: /Länge/ }));
    expect(p.onArgChange).toHaveBeenCalledWith("165 m");
  });

  it("an unfinished unit previews the best completion, not an unrelated default", () => {
    mount("165 mp");
    expect(units()).toContain("kmh");
    expect(screen.getByText(/Vorschau für mph/)).toBeTruthy();
  });

  it("browse mode (bare cv) shows the default category", () => {
    mount("");
    expect(units()[0]).toBe("mm");
    expect(units()).toContain("mi");
  });

  it("clicking a row copies its plain number", async () => {
    mount("165 mph");
    await act(async () => {
      fireEvent.click(document.querySelector('[data-unit="kmh"]')!);
    });
    expect(writeText).toHaveBeenCalledWith("265.54176");
  });

  it("with focus: ↓ selects the next row and Enter pastes it", () => {
    const p = mount("165 mph", { focused: true });
    // Selection starts on the source (mph, row 1); ↓ → mps.
    fireEvent.keyDown(window, { key: "ArrowDown" });
    fireEvent.keyDown(window, { key: "Enter" });
    expect(p.onPick).toHaveBeenCalledWith("73.7616");
  });

  it("with focus: → moves to the next category", () => {
    const p = mount("165 mph", { focused: true });
    fireEvent.keyDown(window, { key: "ArrowRight" });
    expect(p.onArgChange).toHaveBeenCalledWith("165 h"); // speed → time
  });

  it("never swallows keys typed into its own value field", () => {
    const p = mount("165 mph", { focused: true });
    const input = screen.getByLabelText("Wert");
    fireEvent.keyDown(input, { key: "ArrowRight" });
    fireEvent.keyDown(input, { key: "Enter" });
    expect(p.onArgChange).not.toHaveBeenCalled();
    expect(p.onPick).not.toHaveBeenCalled();
  });

  it("currency lists only currencies that have a rate and names the source", () => {
    mount("100 usd");
    expect(units()).toEqual(["eur", "usd", "btc"]);
    expect(document.querySelector('[data-unit="eur"]')!.textContent).toContain("85,38");
    expect(screen.getByText(/EZB-Referenzkurs, Stand 25\.09\.2026/)).toBeTruthy();
  });

  it("currency without any rates says why instead of showing an empty list", () => {
    mount("100 usd", {
      rates: undefined,
      fx: { ...FX, eur_per: {}, error: "ECB: offline? (dns)" },
    });
    expect(units()).toEqual([]);
    expect(screen.getByText("ECB: offline? (dns)")).toBeTruthy();
  });

  it("stale rates are flagged", () => {
    mount("100 usd", { fx: { ...FX, stale: true, error: "ECB: HTTP 500" } });
    expect(screen.getByText(/veraltet/)).toBeTruthy();
  });
});
