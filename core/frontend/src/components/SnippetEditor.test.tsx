import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen, fireEvent, cleanup, waitFor } from "@testing-library/react";
import type { SnippetCategory } from "../lib/ipc";

const { upsertSnippet, createSnippetCategory } = vi.hoisted(() => ({
  upsertSnippet: vi.fn(async () => undefined),
  createSnippetCategory: vi.fn(async () => 42),
}));
vi.mock("../lib/ipc", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../lib/ipc")>()),
  upsertSnippet,
  createSnippetCategory,
}));

import { SnippetEditor, EMPTY_DRAFT, type SnippetDraft } from "./SnippetEditor";

const CATEGORIES: SnippetCategory[] = [
  { id: 1, name: "AI Prompts", sort_order: 1, count: 27 },
  { id: 2, name: "Colors", sort_order: 2, count: 255 },
];

const EXISTING: SnippetDraft = {
  id: 7,
  abbreviation: "mfg",
  title: "Sign-off",
  body: "Mit freundlichen Grüßen",
  categoryId: 1,
  version: 3,
};

function setup(initial: SnippetDraft = EMPTY_DRAFT, over: Partial<Parameters<typeof SnippetEditor>[0]> = {}) {
  const onSaved = vi.fn();
  const onCancel = vi.fn();
  render(
    <SnippetEditor
      initial={initial}
      categories={CATEGORIES}
      onSaved={onSaved}
      onCancel={onCancel}
      {...over}
    />,
  );
  return { onSaved, onCancel };
}

const abbrevField = () => screen.getByPlaceholderText("e.g. mfg");
const titleField = () => screen.getByPlaceholderText("e.g. Signing off");
const bodyField = () => screen.getByPlaceholderText("Template text that gets pasted…");
const groupSelect = () => screen.getByRole("combobox");
const type = (el: Element, value: string) => fireEvent.change(el, { target: { value } });

afterEach(cleanup);
beforeEach(() => {
  upsertSnippet.mockClear();
  upsertSnippet.mockResolvedValue(undefined);
  createSnippetCategory.mockClear();
  createSnippetCategory.mockResolvedValue(42);
});

describe("SnippetEditor — validation", () => {
  it("refuses to save without an abbreviation and says so", async () => {
    const { onSaved } = setup();
    type(bodyField(), "some body");

    fireEvent.click(screen.getByText("Save"));

    expect(await screen.findByText("Abbreviation is required.")).toBeTruthy();
    expect(upsertSnippet).not.toHaveBeenCalled();
    expect(onSaved).not.toHaveBeenCalled();
  });

  it("refuses to save without a body", async () => {
    const { onSaved } = setup();
    type(abbrevField(), "mfg");

    fireEvent.click(screen.getByText("Save"));

    expect(await screen.findByText("Body text is required.")).toBeTruthy();
    expect(upsertSnippet).not.toHaveBeenCalled();
    expect(onSaved).not.toHaveBeenCalled();
  });

  it("treats whitespace-only input as empty", async () => {
    setup();
    type(abbrevField(), "   ");
    type(bodyField(), "   ");

    fireEvent.click(screen.getByText("Save"));

    expect(await screen.findByText("Abbreviation is required.")).toBeTruthy();
    expect(upsertSnippet).not.toHaveBeenCalled();
  });

  it("surfaces a backend failure instead of silently closing", async () => {
    upsertSnippet.mockRejectedValueOnce(new Error("db is locked"));
    const { onSaved } = setup();
    type(abbrevField(), "mfg");
    type(bodyField(), "body");

    fireEvent.click(screen.getByText("Save"));

    expect(await screen.findByText(/db is locked/)).toBeTruthy();
    expect(onSaved).not.toHaveBeenCalled();
    // The Save button must come back — a stuck "Saving…" would strand the user.
    await waitFor(() => expect(screen.getByText("Save")).toBeTruthy());
  });
});

describe("SnippetEditor — saving", () => {
  it("saves a NEW snippet with no group", async () => {
    const { onSaved } = setup();
    type(abbrevField(), "mfg");
    type(titleField(), "Sign-off");
    type(bodyField(), "Mit freundlichen Grüßen");

    fireEvent.click(screen.getByText("Save"));

    await waitFor(() => expect(onSaved).toHaveBeenCalledTimes(1));
    expect(upsertSnippet).toHaveBeenCalledWith(null, "mfg", "Sign-off", "Mit freundlichen Grüßen", null);
  });

  it("saves an EDIT against the existing id, preserving its group", async () => {
    const { onSaved } = setup(EXISTING);
    type(bodyField(), "Viele Grüße");

    fireEvent.click(screen.getByText("Save"));

    await waitFor(() => expect(onSaved).toHaveBeenCalledTimes(1));
    expect(upsertSnippet).toHaveBeenCalledWith(7, "mfg", "Sign-off", "Viele Grüße", 1);
  });

  it("re-assigns the group from the picker", async () => {
    setup(EXISTING);
    type(groupSelect(), "2");

    fireEvent.click(screen.getByText("Save"));

    await waitFor(() => expect(upsertSnippet).toHaveBeenCalled());
    expect(upsertSnippet).toHaveBeenCalledWith(7, "mfg", "Sign-off", "Mit freundlichen Grüßen", 2);
  });

  it("ungroups a snippet when 'No group' is picked", async () => {
    setup(EXISTING);
    type(groupSelect(), "");

    fireEvent.click(screen.getByText("Save"));

    await waitFor(() => expect(upsertSnippet).toHaveBeenCalled());
    expect(upsertSnippet).toHaveBeenCalledWith(7, "mfg", "Sign-off", "Mit freundlichen Grüßen", null);
  });
});

describe("SnippetEditor — inline new group", () => {
  it("creates the pending group on save and files the snippet under it", async () => {
    // The group is deliberately NOT created when typed — only if the save goes
    // through, so an abandoned edit can't leave an orphan group behind.
    setup(EXISTING);
    type(groupSelect(), "__new__");

    const nameField = screen.getByPlaceholderText("New group name");
    type(nameField, "Work");
    expect(createSnippetCategory).not.toHaveBeenCalled();

    fireEvent.click(screen.getByText("Save"));

    await waitFor(() => expect(createSnippetCategory).toHaveBeenCalledWith("Work"));
    expect(upsertSnippet).toHaveBeenCalledWith(7, "mfg", "Sign-off", "Mit freundlichen Grüßen", 42);
  });

  it("does not create a group for a blank name — keeps the previous one", async () => {
    setup(EXISTING);
    type(groupSelect(), "__new__");
    type(screen.getByPlaceholderText("New group name"), "   ");

    fireEvent.click(screen.getByText("Save"));

    await waitFor(() => expect(upsertSnippet).toHaveBeenCalled());
    expect(createSnippetCategory).not.toHaveBeenCalled();
    expect(upsertSnippet).toHaveBeenCalledWith(7, "mfg", "Sign-off", "Mit freundlichen Grüßen", 1);
  });

  it("backs out of the inline field via its × button", () => {
    setup(EXISTING);
    type(groupSelect(), "__new__");
    expect(screen.getByPlaceholderText("New group name")).toBeTruthy();

    fireEvent.click(screen.getByTitle("Cancel new group"));

    expect(screen.queryByPlaceholderText("New group name")).toBeNull();
    expect(screen.getByRole("combobox")).toBeTruthy();
  });

  it("Esc in the inline group field closes only THAT field, not the whole form", () => {
    // Otherwise abandoning a mistyped group name would throw away the edit.
    const { onCancel } = setup(EXISTING);
    type(groupSelect(), "__new__");

    fireEvent.keyDown(screen.getByPlaceholderText("New group name"), { key: "Escape" });

    expect(screen.queryByPlaceholderText("New group name")).toBeNull();
    expect(onCancel).not.toHaveBeenCalled();
  });
});

describe("SnippetEditor — keyboard", () => {
  it("Cmd+Enter saves from the BODY, where plain Enter must insert a newline", async () => {
    const { onSaved } = setup();
    type(abbrevField(), "mfg");
    type(bodyField(), "line one");

    fireEvent.keyDown(bodyField(), { key: "Enter" });
    expect(upsertSnippet).not.toHaveBeenCalled(); // plain Enter = newline

    fireEvent.keyDown(bodyField(), { key: "Enter", metaKey: true });
    await waitFor(() => expect(onSaved).toHaveBeenCalledTimes(1));
  });

  it("Ctrl+Enter saves too (non-mac)", async () => {
    const { onSaved } = setup();
    type(abbrevField(), "mfg");
    type(bodyField(), "body");

    fireEvent.keyDown(bodyField(), { key: "Enter", ctrlKey: true });
    await waitFor(() => expect(onSaved).toHaveBeenCalledTimes(1));
  });

  it("plain Enter in the single-line fields saves", async () => {
    const { onSaved } = setup();
    type(abbrevField(), "mfg");
    type(bodyField(), "body");

    fireEvent.keyDown(abbrevField(), { key: "Enter" });
    await waitFor(() => expect(onSaved).toHaveBeenCalledTimes(1));
  });

  it("Esc cancels without saving", () => {
    const { onCancel, onSaved } = setup(EXISTING);

    fireEvent.keyDown(bodyField(), { key: "Escape" });

    expect(onCancel).toHaveBeenCalledTimes(1);
    expect(onSaved).not.toHaveBeenCalled();
    expect(upsertSnippet).not.toHaveBeenCalled();
  });

  it("the Cancel button discards the edit", () => {
    const { onCancel } = setup(EXISTING);
    type(bodyField(), "changed my mind");

    fireEvent.click(screen.getByText("Cancel"));

    expect(onCancel).toHaveBeenCalledTimes(1);
    expect(upsertSnippet).not.toHaveBeenCalled();
  });
});

describe("SnippetEditor — header", () => {
  it("distinguishes a new snippet from an edit", () => {
    setup(EMPTY_DRAFT);
    expect(screen.getByText("New Snippet")).toBeTruthy();
    expect(screen.queryByText(/^v\d/)).toBeNull(); // no revision for a new one
    cleanup();

    setup(EXISTING);
    expect(screen.getByText("Edit Snippet")).toBeTruthy();
    expect(screen.getByText("v3")).toBeTruthy();
  });

  it("shows v1 for an existing snippet with no recorded revision", () => {
    setup({ ...EXISTING, version: undefined });
    expect(screen.getByText("v1")).toBeTruthy();
  });
});
