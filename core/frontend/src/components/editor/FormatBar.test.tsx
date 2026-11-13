// @vitest-environment happy-dom
import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { FormatBar } from "./FormatBar";

afterEach(cleanup);

const mount = (formats: ("auto" | "16:9" | "9:16")[], active: "auto" | "16:9" | "9:16") => {
  const on = { onSwitch: vi.fn(), onAdd: vi.fn(), onRemove: vi.fn() };
  render(<FormatBar formats={formats} active={active} {...on} />);
  return on;
};

describe("FormatBar", () => {
  it("shows one chip per format and switches on a click", () => {
    const on = mount(["16:9", "9:16"], "16:9");
    expect(screen.getByTestId("ed-format-16:9").getAttribute("aria-pressed")).toBe("true");
    fireEvent.click(screen.getByTestId("ed-format-16:9"));
    expect(on.onSwitch).not.toHaveBeenCalled();
    fireEvent.click(screen.getByTestId("ed-format-9:16"));
    expect(on.onSwitch).toHaveBeenCalledWith("9:16");
  });

  it("adds a format the project does not have yet", () => {
    const on = mount(["auto"], "auto");
    expect(screen.getByTestId("ed-format-auto").textContent).toBe("Auto");
    expect(screen.queryByTestId("ed-format-remove")).toBeNull();
    fireEvent.click(screen.getByTestId("ed-format-add"));
    expect(screen.getByTestId("ed-format-add-9:16").textContent).toContain("TikTok");
    fireEvent.click(screen.getByTestId("ed-format-add-9:16"));
    expect(on.onAdd).toHaveBeenCalledWith("9:16");
    expect(screen.queryByRole("menu")).toBeNull();
  });

  it("removes the active format only after a second click", () => {
    const on = mount(["16:9", "9:16"], "9:16");
    fireEvent.click(screen.getByTestId("ed-format-remove"));
    expect(on.onRemove).not.toHaveBeenCalled();
    expect(screen.getByTestId("ed-format-remove").textContent).toContain("Remove?");
    fireEvent.click(screen.getByTestId("ed-format-remove"));
    expect(on.onRemove).toHaveBeenCalledWith("9:16");
  });
});
