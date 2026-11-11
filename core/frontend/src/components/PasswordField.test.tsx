import { describe, it, expect, vi } from "vitest";
import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import { PasswordField } from "./PasswordField";

describe("PasswordField", () => {
  it("says plainly when no password is stored, and Save needs input", () => {
    render(<PasswordField stored={false} onSave={vi.fn()} />);
    expect(screen.getByTestId("password-field-missing").textContent).toContain("No password saved yet");
    expect(screen.queryByTestId("password-field-stored")).toBeNull();
    expect((screen.getByText("Save") as HTMLButtonElement).disabled).toBe(true);
  });

  it("shows the green check for a stored password", () => {
    render(<PasswordField stored onSave={vi.fn()} />);
    const ok = screen.getByTestId("password-field-stored");
    expect(ok.textContent).toContain("Password saved");
    expect(ok.className).toContain("text-emerald-500");
    expect(ok.querySelector("svg")).toBeTruthy();
  });

  it("saves on Enter and on the button, then clears the field", async () => {
    const onSave = vi.fn().mockResolvedValue(true);
    const { container } = render(<PasswordField stored={false} onSave={onSave} />);
    const field = container.querySelector("input")!;
    fireEvent.change(field, { target: { value: "geheim" } });
    fireEvent.keyDown(field, { key: "Enter" });
    await waitFor(() => expect(onSave).toHaveBeenCalledWith("geheim"));
    await waitFor(() => expect(field.value).toBe(""));
    fireEvent.change(field, { target: { value: "zwei" } });
    fireEvent.click(screen.getByText("Save"));
    await waitFor(() => expect(onSave).toHaveBeenLastCalledWith("zwei"));
  });

  it("a save that did not stick is an error, and the typed password stays", async () => {
    const { container } = render(<PasswordField stored={false} onSave={vi.fn().mockResolvedValue(false)} />);
    const field = container.querySelector("input")!;
    fireEvent.change(field, { target: { value: "geheim" } });
    fireEvent.click(screen.getByText("Save"));
    await waitFor(() => expect(screen.getByRole("alert").textContent).toContain("could not be stored"));
    expect(field.value).toBe("geheim");
  });

  it("a failing save shows the reason", async () => {
    const { container } = render(<PasswordField stored={false} onSave={vi.fn().mockRejectedValue("keychain locked")} />);
    fireEvent.change(container.querySelector("input")!, { target: { value: "x" } });
    fireEvent.click(screen.getByText("Save"));
    await waitFor(() => expect(screen.getByRole("alert").textContent).toContain("keychain locked"));
  });

  it("Remove deletes the stored password", async () => {
    const onSave = vi.fn().mockResolvedValue(false);
    render(<PasswordField stored onSave={onSave} />);
    fireEvent.click(screen.getByText("Remove"));
    await waitFor(() => expect(onSave).toHaveBeenCalledWith(""));
    expect(screen.queryByRole("alert")).toBeNull();
  });
});
