import { afterEach, describe, expect, it } from "vitest";
import { cleanup, render, screen } from "@testing-library/react";
import { SearchBar } from "./SearchBar";

afterEach(cleanup);

describe("SearchBar completion", () => {
  it("shows the rest of the word as ghost text right after the typed text", () => {
    render(<SearchBar value="spotify hi" onChange={() => {}} completion={{ ghost: "story" }} />);
    const ghost = screen.getByTestId("completion-ghost");
    const [typed, rest] = Array.from(ghost.children) as HTMLElement[];
    expect(typed.textContent).toBe("spotify hi");
    expect(typed.className).toContain("invisible");
    expect(rest.textContent).toBe("story");
    expect(screen.getByRole("textbox").getAttribute("aria-autocomplete")).toBe("inline");
  });

  it("draws no floating hint — the choices live as rows in the list", () => {
    // v0.218.0: the pill under the field covered the first list row and
    // repeated what the selected row already says.
    const { container } = render(
      <SearchBar value="spotify p" onChange={() => {}} completion={{ ghost: "lay" }} />,
    );
    expect(screen.queryByTestId("completion-pill")).toBeNull();
    expect(container.textContent).not.toContain("Tab");
  });

  it("an empty ghost (typo fix, nothing to append) renders no overlay", () => {
    render(<SearchBar value="spotify histroy" onChange={() => {}} completion={{ ghost: "" }} />);
    expect(screen.queryByTestId("completion-ghost")).toBeNull();
    expect(screen.getByRole("textbox").getAttribute("aria-autocomplete")).toBeNull();
  });

  it("renders nothing extra without a completion", () => {
    render(<SearchBar value="hallo" onChange={() => {}} />);
    expect(screen.queryByTestId("completion-ghost")).toBeNull();
    expect(screen.getByRole("textbox").getAttribute("aria-autocomplete")).toBeNull();
  });
});
