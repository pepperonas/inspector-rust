import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { act, cleanup, fireEvent, render } from "@testing-library/react";
import type { RecordPresetEntry } from "../../lib/ipc";

const list = vi.fn();
const save = vi.fn();
const rename = vi.fn();
const del = vi.fn();
const setDefault = vi.fn();
const reveal = vi.fn();

vi.mock("../../lib/ipc", () => ({
  recordPresetsList: () => list(),
  recordPresetRename: (file: string, name: string) => rename(file, name),
  recordPresetDelete: (file: string) => del(file),
  recordPresetSetDefault: (file: string | null) => setDefault(file),
  recordPresetsReveal: () => reveal(),
}));

import { PresetsMenu } from "./PresetsMenu";

const dark: RecordPresetEntry = { name: "Dark", file: "Dark.irpreset", ok: true, error: null };
const broken: RecordPresetEntry = { name: "Half", file: "Half.irpreset", ok: false, error: "record.editor.preset_unreadable" };

async function mount(onApply = vi.fn(), onClose = vi.fn()) {
  const r = render(<PresetsMenu onSave={(name) => save(7, name)} onApply={onApply} onClose={onClose} />);
  await act(async () => undefined);
  return { ...r, onApply, onClose };
}

beforeEach(() => {
  list.mockResolvedValue({ presets: [dark, broken], defaultFile: null });
  save.mockResolvedValue({ name: "New", file: "New.irpreset", ok: true, error: null });
  rename.mockResolvedValue(dark);
  del.mockResolvedValue(undefined);
  setDefault.mockResolvedValue(undefined);
  reveal.mockResolvedValue(undefined);
});
afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});

describe("PresetsMenu", () => {
  it("picking a preset applies it; a broken one is shown with its reason and cannot be applied", async () => {
    const { getAllByTestId, getByText, onApply } = await mount();
    const [ok, bad] = getAllByTestId("ed-preset-apply") as HTMLButtonElement[];
    expect(bad.disabled).toBe(true);
    expect(getByText("Damaged file")).toBeTruthy();
    fireEvent.click(ok);
    expect(onApply).toHaveBeenCalledTimes(1);
    expect(onApply).toHaveBeenCalledWith(dark);
  });

  it("saves the current style under the typed name and reads the list again", async () => {
    const { getByTestId } = await mount();
    fireEvent.click(getByTestId("ed-preset-save-open"));
    fireEvent.change(getByTestId("ed-preset-name"), { target: { value: "  New " } });
    await act(async () => {
      fireEvent.keyDown(getByTestId("ed-preset-name"), { key: "Enter" });
    });
    expect(save).toHaveBeenCalledWith(7, "New");
    expect(list).toHaveBeenCalledTimes(2);
  });

  it("a refused name is shown in the menu and keeps the form open", async () => {
    save.mockRejectedValue("record.editor.preset_name_taken");
    const { getByTestId } = await mount();
    fireEvent.click(getByTestId("ed-preset-save-open"));
    fireEvent.change(getByTestId("ed-preset-name"), { target: { value: "dark" } });
    await act(async () => {
      fireEvent.click(getByTestId("ed-preset-save"));
    });
    expect(getByTestId("ed-preset-error").textContent).toBe("A preset with this name already exists.");
    expect(getByTestId("ed-preset-name")).toBeTruthy();
  });

  it("delete asks inline first; Esc disarms instead of closing", async () => {
    const { getAllByTestId, getByTestId, queryByTestId, onClose } = await mount();
    fireEvent.click(getAllByTestId("ed-preset-delete")[0]);
    expect(del).not.toHaveBeenCalled();
    expect(getByTestId("ed-preset-armed").textContent).toContain("Delete “Dark”?");
    fireEvent.keyDown(window, { key: "Escape" });
    expect(queryByTestId("ed-preset-armed")).toBeNull();
    expect(onClose).not.toHaveBeenCalled();
    fireEvent.click(getAllByTestId("ed-preset-delete")[0]);
    await act(async () => {
      fireEvent.click(getByTestId("ed-preset-delete-confirm"));
    });
    expect(del).toHaveBeenCalledWith("Dark.irpreset");
    fireEvent.keyDown(window, { key: "Escape" });
    expect(onClose).toHaveBeenCalled();
  });

  it("a broken default can still be cleared with its star", async () => {
    list.mockResolvedValue({ presets: [dark, broken], defaultFile: "Half.irpreset" });
    const { getAllByTestId } = await mount();
    const star = getAllByTestId("ed-preset-default")[1];
    expect(star.getAttribute("aria-pressed")).toBe("true");
    expect(star.hasAttribute("disabled")).toBe(false);
    await act(async () => {
      fireEvent.click(star);
    });
    expect(setDefault).toHaveBeenCalledWith(null);
  });

  it("the star sets and clears the default for new projects; a broken preset cannot become it", async () => {
    const { getAllByTestId } = await mount();
    expect(getAllByTestId("ed-preset-default")[1].hasAttribute("disabled")).toBe(true);
    list.mockResolvedValue({ presets: [dark, broken], defaultFile: "Dark.irpreset" });
    await act(async () => {
      fireEvent.click(getAllByTestId("ed-preset-default")[0]);
    });
    expect(setDefault).toHaveBeenCalledWith("Dark.irpreset");
    expect(getAllByTestId("ed-preset-default")[0].getAttribute("aria-pressed")).toBe("true");
    await act(async () => {
      fireEvent.click(getAllByTestId("ed-preset-default")[0]);
    });
    expect(setDefault).toHaveBeenLastCalledWith(null);
  });

  it("renames inline and shows the folder in the Finder", async () => {
    const { getAllByLabelText, getByTestId } = await mount();
    fireEvent.click(getAllByLabelText("Rename")[0]);
    fireEvent.change(getByTestId("ed-preset-rename-input"), { target: { value: "Night" } });
    await act(async () => {
      fireEvent.keyDown(getByTestId("ed-preset-rename-input"), { key: "Enter" });
    });
    expect(rename).toHaveBeenCalledWith("Dark.irpreset", "Night");
    await act(async () => {
      fireEvent.click(getByTestId("ed-preset-reveal"));
    });
    expect(reveal).toHaveBeenCalled();
  });
});
