import { describe, it, expect, vi, afterEach } from "vitest";
import { render, screen, fireEvent, cleanup, within } from "@testing-library/react";
import type { CommandDoc } from "../lib/commandDocs";
import { COMMAND_DOCS } from "../lib/commandDocs";
import CommandHelp from "./CommandHelp";

/** A doc with every optional section EMPTY — the conditional-rendering baseline. */
const MINIMAL: CommandDoc = {
  command: "widget",
  aliases: [],
  category: "System",
  version_added: "1.2.3",
  tagline: "Does a widget thing.",
  synopsis: "widget <name>",
  description: "The long description of widget.",
  arguments: [],
  flags: [],
  examples: [{ input: "widget foo", result: "a foo widget" }],
  tips: [],
  caveats: [],
  related: [],
};

/** …and one with every optional section POPULATED. */
const FULL: CommandDoc = {
  ...MINIMAL,
  aliases: ["wdg", "wid"],
  arguments: [
    { name: "name", required: true, description: "What to widget." },
    { name: "count", required: false, description: "How many.", default: "1" },
  ],
  flags: [
    { flag: "--loud", description: "Be loud." },
    { flag: "--out", value_type: "<path>", description: "Where to write.", default: "~/Downloads" },
  ],
  examples: [
    { input: "widget foo", result: "a foo widget" },
    { input: "widget bar --loud", result: "a LOUD bar widget", note: "shouts" },
  ],
  tips: ["Hold Shift for extra widget."],
  caveats: ["Widgets are not refundable."],
  related: ["kill", "clean"],
  see_also: "docs/widget.md",
};

afterEach(cleanup);

describe("CommandHelp — doc view", () => {
  it("shows the command, its version and the whole body", () => {
    render(<CommandHelp target={{ kind: "doc", doc: FULL }} onNavigate={() => {}} />);

    expect(screen.getByRole("heading", { name: "widget" })).toBeTruthy();
    expect(screen.getByText("since v1.2.3")).toBeTruthy();
    expect(screen.getByText("Does a widget thing.")).toBeTruthy();
    expect(screen.getByText("widget <name>")).toBeTruthy();
    expect(screen.getByText("The long description of widget.")).toBeTruthy();
  });

  it("renders arguments and flags with their optional/default annotations", () => {
    render(<CommandHelp target={{ kind: "doc", doc: FULL }} onNavigate={() => {}} />);

    expect(screen.getByText("— What to widget.")).toBeTruthy();
    expect(screen.getByText("optional")).toBeTruthy(); // only `count` is optional
    expect(screen.getByText("(default: 1)")).toBeTruthy();
    expect(screen.getByText("--loud")).toBeTruthy();
    expect(screen.getByText("--out <path>")).toBeTruthy(); // flag + value_type
    expect(screen.getByText("(default: ~/Downloads)")).toBeTruthy();
  });

  it("omits every empty optional section", () => {
    // A doc with no args/flags/tips/caveats/aliases must not render bare
    // section headers — Examples is the one always-present section.
    render(<CommandHelp target={{ kind: "doc", doc: MINIMAL }} onNavigate={() => {}} />);

    expect(screen.queryByText("Arguments")).toBeNull();
    expect(screen.queryByText("Flags")).toBeNull();
    expect(screen.queryByText("Tips")).toBeNull();
    expect(screen.queryByText("Caveats")).toBeNull();
    expect(screen.queryByText(/^Aliases:/)).toBeNull();
    expect(screen.getByText("Examples")).toBeTruthy();
  });

  it("lists the aliases WITHOUT repeating the primary command name", () => {
    render(<CommandHelp target={{ kind: "doc", doc: FULL }} onNavigate={() => {}} />);
    const aliases = screen.getByText(/^Aliases:/);
    expect(within(aliases).getByText("wdg")).toBeTruthy();
    expect(within(aliases).getByText("wid")).toBeTruthy();
    expect(within(aliases).queryByText("widget")).toBeNull();
  });

  it("clicking an example puts THAT example into the search bar (not a help query)", () => {
    // The Example.input contract: examples are Tab-fillable, i.e. runnable.
    const onNavigate = vi.fn();
    render(<CommandHelp target={{ kind: "doc", doc: FULL }} onNavigate={onNavigate} />);

    fireEvent.click(screen.getByText("widget bar --loud"));
    expect(onNavigate).toHaveBeenCalledWith("widget bar --loud");
  });

  it("clicking a related command navigates to ITS doc (keyword + ?)", () => {
    const onNavigate = vi.fn();
    render(<CommandHelp target={{ kind: "doc", doc: FULL }} onNavigate={onNavigate} />);

    fireEvent.click(screen.getByRole("button", { name: "clean" }));
    expect(onNavigate).toHaveBeenCalledWith("clean?");
  });

  it("the ← Index chip goes back to the browsable index", () => {
    const onNavigate = vi.fn();
    render(<CommandHelp target={{ kind: "doc", doc: FULL }} onNavigate={onNavigate} />);

    fireEvent.click(screen.getByText("← Index"));
    expect(onNavigate).toHaveBeenCalledWith("?");
  });

  it("shows the see-also docs path only when the doc has one", () => {
    render(<CommandHelp target={{ kind: "doc", doc: FULL }} onNavigate={() => {}} />);
    expect(screen.getByText("docs/widget.md")).toBeTruthy();
    cleanup();
    render(<CommandHelp target={{ kind: "doc", doc: MINIMAL }} onNavigate={() => {}} />);
    expect(screen.queryByText("docs/widget.md")).toBeNull();
  });
});

describe("CommandHelp — index view", () => {
  it("lists the real registry, grouped by category", () => {
    render(<CommandHelp target={{ kind: "index", filter: "" }} onNavigate={() => {}} />);

    expect(screen.getByRole("heading", { name: "Command index" })).toBeTruthy();
    // Every documented command is reachable from the index — the index is the
    // discovery surface, so a doc missing from it is invisible.
    for (const doc of COMMAND_DOCS) {
      expect(screen.getAllByText(doc.command).length, `${doc.command} in index`).toBeGreaterThan(0);
    }
  });

  it("shows NO fallback notice for the plain `?` index", () => {
    render(<CommandHelp target={{ kind: "index", filter: "" }} onNavigate={() => {}} />);
    expect(screen.queryByText(/showing everything/)).toBeNull();
  });

  it("explains itself when a filter matched nothing, and still lists everything", () => {
    // `? zzzz` falls back to the full index — silently showing an unfiltered
    // list would read as "the filter is broken".
    render(<CommandHelp target={{ kind: "index", filter: "zzzz" }} onNavigate={() => {}} />);

    expect(screen.getByText(/No command matches/)).toBeTruthy();
    expect(screen.getByText(/zzzz/)).toBeTruthy();
    expect(screen.getAllByText(COMMAND_DOCS[0].command).length).toBeGreaterThan(0);
  });

  it("treats a whitespace-only filter as no filter", () => {
    render(<CommandHelp target={{ kind: "index", filter: "   " }} onNavigate={() => {}} />);
    expect(screen.queryByText(/No command matches/)).toBeNull();
  });

  it("clicking an index row opens that command's doc", () => {
    const onNavigate = vi.fn();
    render(<CommandHelp target={{ kind: "index", filter: "" }} onNavigate={onNavigate} />);

    const target = COMMAND_DOCS[0];
    fireEvent.click(screen.getAllByText(target.command)[0]);
    expect(onNavigate).toHaveBeenCalledWith(`${target.command}?`);
  });
});
