import { afterEach, describe, expect, it, vi } from "vitest";
import { act, cleanup, fireEvent, render } from "@testing-library/react";
import { CONFIRM_ARM_MS, ConfirmButton } from "./ConfirmButton";

afterEach(() => {
  cleanup();
  vi.useRealTimers();
});

function setup() {
  const onConfirm = vi.fn();
  const view = render(
    <ConfirmButton onConfirm={onConfirm} title="Delete" className="idle">
      x
    </ConfirmButton>,
  );
  return { onConfirm, button: view.getByRole("button") };
}

describe("ConfirmButton", () => {
  it("the first click only arms it", () => {
    const { onConfirm, button } = setup();
    fireEvent.click(button);
    expect(onConfirm).not.toHaveBeenCalled();
    expect(button.getAttribute("data-armed")).toBe("true");
    expect(button.getAttribute("title")).toBe("Really delete?");
  });

  it("the second click runs the action once and disarms", () => {
    const { onConfirm, button } = setup();
    fireEvent.click(button);
    fireEvent.click(button);
    expect(onConfirm).toHaveBeenCalledTimes(1);
    expect(button.getAttribute("data-armed")).toBeNull();
  });

  it("disarms when the pointer leaves, so a later click arms again", () => {
    const { onConfirm, button } = setup();
    fireEvent.click(button);
    fireEvent.mouseLeave(button);
    fireEvent.click(button);
    expect(onConfirm).not.toHaveBeenCalled();
  });

  it("disarms by itself after the waiting time", () => {
    vi.useFakeTimers();
    const { onConfirm, button } = setup();
    fireEvent.click(button);
    act(() => void vi.advanceTimersByTime(CONFIRM_ARM_MS + 10));
    expect(button.getAttribute("data-armed")).toBeNull();
    fireEvent.click(button);
    expect(onConfirm).not.toHaveBeenCalled();
  });

  it("a click does not reach the row behind it", () => {
    const row = vi.fn();
    const view = render(
      <div onClick={row}>
        <ConfirmButton onConfirm={() => {}} title="Delete" className="idle">
          x
        </ConfirmButton>
      </div>,
    );
    fireEvent.click(view.getByRole("button"));
    expect(row).not.toHaveBeenCalled();
  });
});
