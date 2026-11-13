import { afterEach, describe, expect, it } from "vitest";
import { cleanup, fireEvent, render } from "@testing-library/react";
import { CalendarPanel } from "./CalendarPanel";

afterEach(() => {
  cleanup();
  document.body.innerHTML = "";
});

function searchField(): HTMLInputElement {
  const el = document.createElement("input");
  el.setAttribute("data-main-search", "");
  document.body.appendChild(el);
  return el;
}

describe("CalendarPanel keys from the search field", () => {
  it("a typed t is text (`cal oktober`); ⌘T jumps to today", () => {
    const { container } = render(<CalendarPanel focused arg="märz 1990" onExit={() => {}} />);
    expect(container.textContent).toContain("1990");
    const search = searchField();
    fireEvent.keyDown(search, { key: "t" });
    expect(container.textContent).toContain("1990");
    fireEvent.keyDown(search, { key: "t", metaKey: true });
    expect(container.textContent).toContain(String(new Date().getFullYear()));
    expect(container.textContent).not.toContain("1990");
  });

  it("the arrows still turn the pages from the search field", () => {
    const { container } = render(<CalendarPanel focused arg="märz 1990" onExit={() => {}} />);
    fireEvent.keyDown(searchField(), { key: "ArrowDown" });
    expect(container.textContent).toContain("1991");
  });
});
