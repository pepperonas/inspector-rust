import { describe, it, expect, afterEach, vi } from "vitest";
import { render, screen, cleanup, fireEvent } from "@testing-library/react";
import { AudioMenu } from "./AudioMenu";
import { Footer } from "./Footer";
import type { AudioRouteView } from "../lib/audio-route";

afterEach(cleanup);

function view(over: Partial<AudioRouteView> = {}): AudioRouteView {
  return {
    rows: [
      { id: "10", name: "MacBook Pro-Lautsprecher", kind: "speaker", active: true },
      { id: "20", name: "MX Sound", kind: "bluetooth", active: false },
    ],
    activeName: "MacBook Pro-Lautsprecher",
    activeKind: "speaker",
    viaBoom: false,
    boomEnabled: false,
    boomInstalled: true,
    ...over,
  };
}

describe("AudioMenu", () => {
  it("shows the active device and, with boom, 'boom → device'", () => {
    const { rerender } = render(<AudioMenu view={view()} onSelect={() => {}} onToggleBoom={() => {}} />);
    expect(screen.getByTestId("audio-menu").textContent).toContain("MacBook Pro-Lautsprecher");
    rerender(
      <AudioMenu view={view({ viaBoom: true, boomEnabled: true })} onSelect={() => {}} onToggleBoom={() => {}} />,
    );
    expect(screen.getByTestId("audio-menu").textContent).toContain("boom → MacBook Pro-Lautsprecher");
  });

  it("opens upward, picks a device and toggles boom", () => {
    const onSelect = vi.fn();
    const onToggleBoom = vi.fn();
    const onOpen = vi.fn();
    render(<AudioMenu view={view()} onSelect={onSelect} onToggleBoom={onToggleBoom} onOpen={onOpen} />);
    fireEvent.click(screen.getByTestId("audio-menu"));
    expect(onOpen).toHaveBeenCalledOnce();
    fireEvent.click(screen.getByText("MX Sound"));
    expect(onSelect).toHaveBeenCalledWith("20");
    expect(screen.queryByRole("menu")).toBeNull();
    fireEvent.click(screen.getByTestId("audio-menu"));
    fireEvent.click(screen.getByTestId("audio-menu-boom"));
    expect(onToggleBoom).toHaveBeenCalledOnce();
  });

  it("picking the already-active device does nothing", () => {
    const onSelect = vi.fn();
    render(<AudioMenu view={view()} onSelect={onSelect} onToggleBoom={() => {}} />);
    fireEvent.click(screen.getByTestId("audio-menu"));
    fireEvent.click(screen.getByText("MacBook Pro-Lautsprecher", { selector: "span" , ignore: "[data-audio-trigger] *" }));
    expect(onSelect).not.toHaveBeenCalled();
  });

  it("Esc closes only the menu — it never reaches the window (popup close)", () => {
    const onWindowKey = vi.fn();
    window.addEventListener("keydown", onWindowKey);
    render(<AudioMenu view={view()} onSelect={() => {}} onToggleBoom={() => {}} />);
    fireEvent.click(screen.getByTestId("audio-menu"));
    fireEvent.keyDown(screen.getByRole("menu"), { key: "Escape" });
    expect(screen.queryByRole("menu")).toBeNull();
    expect(onWindowKey).not.toHaveBeenCalled();
    window.removeEventListener("keydown", onWindowKey);
  });

  it("hides the boom switch when the driver is not installed", () => {
    render(<AudioMenu view={view({ boomInstalled: false })} onSelect={() => {}} onToggleBoom={() => {}} />);
    fireEvent.click(screen.getByTestId("audio-menu"));
    expect(screen.queryByTestId("audio-menu-boom")).toBeNull();
  });

  it("sits in the footer's top row BEFORE the dark toggle", async () => {
    render(
      <Footer
        onDarkWakeToggle={() => {}}
        audio={{ view: view(), onSelect: () => {}, onToggleBoom: () => {} }}
      />,
    );
    const row = screen.getByTestId("footer-row-top");
    const audio = await screen.findByTestId("audio-menu"); // lazy chunk
    const dark = screen.getByText("dark");
    expect(row.contains(audio)).toBe(true);
    expect(audio.compareDocumentPosition(dark) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
  });
});
