import { describe, it, expect, vi, afterEach } from "vitest";
import { render, cleanup, fireEvent } from "@testing-library/react";
import { SearchBar } from "./SearchBar";

afterEach(cleanup);

describe("SearchBar — clear button", () => {
  it("is hidden while the input is empty", () => {
    const { queryByLabelText } = render(<SearchBar value="" onChange={() => {}} />);
    expect(queryByLabelText("Clear input")).toBeNull();
  });

  it("shows once there is text and clears the whole input on click", () => {
    const onChange = vi.fn();
    const { getByLabelText } = render(<SearchBar value="disk /home" onChange={onChange} />);
    fireEvent.click(getByLabelText("Clear input"));
    expect(onChange).toHaveBeenCalledWith("");
  });

  it("prevents mousedown so the input keeps focus (typing continues after clearing)", () => {
    const { getByLabelText } = render(<SearchBar value="x" onChange={() => {}} />);
    // fireEvent returns false when the event was cancelled (preventDefault).
    expect(fireEvent.mouseDown(getByLabelText("Clear input"))).toBe(false);
  });
});

it("intercepts multiline translation pastes before the input strips line breaks", () => {
  const onChange = vi.fn();
  const { getByRole } = render(<SearchBar value="tren " onChange={onChange} />);
  const input = getByRole("textbox") as HTMLInputElement;
  input.setSelectionRange(5, 5);
  expect(fireEvent.paste(input, {
    clipboardData: { getData: () => "Hello\n\nWorld" },
  })).toBe(false);
  expect(onChange).toHaveBeenCalledWith("tren Hello↵↵World");
});
