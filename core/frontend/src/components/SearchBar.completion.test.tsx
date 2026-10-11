import { afterEach, describe, expect, it } from "vitest";
import { cleanup, render, screen } from "@testing-library/react";
import { SearchBar, type CompletionHint } from "./SearchBar";

afterEach(cleanup);

function hint(over: Partial<CompletionHint> = {}): CompletionHint {
  return { ghost: "story", value: "history", hint: "Verlauf nach Tagen", index: -1, total: 1, cycling: false, ...over };
}

describe("SearchBar completion", () => {
  it("shows the rest of the word as ghost text right after the typed text", () => {
    render(<SearchBar value="spotify hi" onChange={() => {}} completion={hint()} />);
    const ghost = screen.getByTestId("completion-ghost");
    const [typed, rest] = Array.from(ghost.children) as HTMLElement[];
    expect(typed.textContent).toBe("spotify hi");
    expect(typed.className).toContain("invisible");
    expect(rest.textContent).toBe("story");
    expect(screen.getByRole("textbox").getAttribute("aria-autocomplete")).toBe("inline");
  });

  it("names the word Tab takes, its meaning and how many alternatives there are", () => {
    render(<SearchBar value="spotify p" onChange={() => {}} completion={hint({ ghost: "lay", value: "play", hint: "abspielen", total: 3 })} />);
    const pill = screen.getByTestId("completion-pill");
    expect(pill.textContent).toContain("⇥ Tab");
    expect(pill.textContent).toContain("play · abspielen");
    expect(pill.textContent).toContain("+2");
    // It floats under the field — in the row it took the input's width
    // (117 px next to the tabs) and the ghost got cut off.
    expect(pill.className).toContain("absolute");
  });

  it("while cycling, shows what the next Tab brings and the position", () => {
    render(
      <SearchBar
        value="spotify play"
        onChange={() => {}}
        completion={hint({ ghost: "", value: "pause", hint: "pausieren", index: 0, total: 3, cycling: true })}
      />,
    );
    expect(screen.queryByTestId("completion-ghost")).toBeNull();
    const pill = screen.getByTestId("completion-pill");
    expect(pill.textContent).toContain("next: pause · pausieren");
    expect(pill.textContent).toContain("1/3");
  });

  it("shows a typo correction as typed → corrected", () => {
    render(
      <SearchBar
        value="spotify histroy"
        onChange={() => {}}
        completion={hint({ ghost: "", fix: "histroy", value: "history" })}
      />,
    );
    const pill = screen.getByTestId("completion-pill");
    expect(pill.textContent).toContain("histroy → history · Verlauf nach Tagen");
    expect(pill.querySelector(".line-through")?.textContent).toBe("histroy");
    expect(screen.queryByTestId("completion-ghost")).toBeNull();
  });

  it("renders nothing extra without a completion", () => {
    render(<SearchBar value="hallo" onChange={() => {}} />);
    expect(screen.queryByTestId("completion-ghost")).toBeNull();
    expect(screen.queryByTestId("completion-pill")).toBeNull();
    expect(screen.getByRole("textbox").getAttribute("aria-autocomplete")).toBeNull();
  });
});
