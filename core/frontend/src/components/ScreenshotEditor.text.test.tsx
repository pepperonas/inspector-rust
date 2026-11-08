import { describe, it, expect, vi } from "vitest";
import { render } from "@testing-library/react";
import { createRef } from "react";
import { TextInputOverlay } from "./ScreenshotEditor";

describe("TextInputOverlay", () => {
  it("takes the keyboard focus as soon as it appears, so typing goes into it", () => {
    const canvas = document.createElement("canvas");
    canvas.width = 800;
    canvas.height = 600;
    document.body.appendChild(canvas);
    const canvasRef = createRef<HTMLCanvasElement>() as { current: HTMLCanvasElement | null };
    canvasRef.current = canvas;
    const { container } = render(
      <TextInputOverlay
        canvasRef={canvasRef}
        input={{ x: 100, y: 100, value: "" }}
        color="#ff0000"
        fontSize={28}
        onChange={vi.fn()}
        onCommit={vi.fn()}
        onCancel={vi.fn()}
      />,
    );
    const input = container.querySelector("input");
    expect(input).not.toBeNull();
    expect(document.activeElement).toBe(input);
  });
});
