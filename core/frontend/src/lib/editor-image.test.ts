import { describe, expect, it } from "vitest";
import type { Annotation, ImageAnnotation } from "./editor-geometry";
import { boundsOf, hitTest, moveAnnotation, normalizeAnnotation, resizeAnnotation, restyleAnnotation, styleOfAnnotation } from "./editor-geometry";
import { isSafeImageSrc, placeImage, scaleImage } from "./editor-image";
import { resizeBy } from "./editor-selection";
import { transformDoc } from "./editor-transform";

const SRC = "data:image/png;base64,iVBORw0KGgo=";
const img: ImageAnnotation = { id: "i", type: "image", x: 100, y: 50, w: 200, h: 100, src: SRC };

describe("placeImage", () => {
  it("never upscales a small picture and centres it", () => {
    expect(placeImage(1000, 800, 120, 60)).toEqual({ x: 440, y: 370, w: 120, h: 60 });
  });
  it("shrinks a big picture to half the screenshot, keeping its shape", () => {
    const r = placeImage(1000, 800, 4000, 1000);
    expect(r.w).toBeCloseTo(500);
    expect(r.h).toBeCloseTo(125);
    expect(r.x + r.w / 2).toBeCloseTo(500);
    expect(r.y + r.h / 2).toBeCloseTo(400);
  });
  it("limits by height too", () => {
    const r = placeImage(1000, 400, 300, 600);
    expect(r.h).toBeCloseTo(200);
    expect(r.w).toBeCloseTo(100);
  });
  it("survives a zero-size picture", () => {
    const r = placeImage(1000, 800, 0, 0);
    expect(r.w).toBeGreaterThan(0);
    expect(r.h).toBeGreaterThan(0);
  });
});

describe("isSafeImageSrc", () => {
  it("accepts inline raster images only", () => {
    expect(isSafeImageSrc(SRC)).toBe(true);
    expect(isSafeImageSrc("data:image/jpeg;base64,/9j/")).toBe(true);
    expect(isSafeImageSrc("https://example.com/a.png")).toBe(false);
    expect(isSafeImageSrc("data:image/svg+xml;base64,PHN2Zz4=")).toBe(false);
    expect(isSafeImageSrc("data:text/html;base64,PGh0bWw+")).toBe(false);
    expect(isSafeImageSrc(42)).toBe(false);
  });
});

describe("image annotation", () => {
  it("is a box: bounds, move, hit anywhere inside", () => {
    expect(boundsOf(img)).toEqual({ x: 100, y: 50, w: 200, h: 100 });
    expect(moveAnnotation(img, 10, -5)).toMatchObject({ x: 110, y: 45, w: 200, h: 100, src: SRC });
    expect(hitTest([img], { x: 200, y: 100 })).toBe("i");
  });
  it("a corner handle keeps the picture's shape", () => {
    const r = normalizeAnnotation(resizeAnnotation(img, "se", 100, 0)) as ImageAnnotation;
    expect(r.w / r.h).toBeCloseTo(2);
    expect(r.x).toBe(100);
    expect(r.y).toBe(50);
    expect(r.w).toBeCloseTo(300);
  });
  it("a corner handle on the opposite side keeps that corner fixed", () => {
    const r = normalizeAnnotation(resizeAnnotation(img, "nw", -100, 0)) as ImageAnnotation;
    expect(r.w).toBeCloseTo(300);
    expect(r.h).toBeCloseTo(150);
    expect(r.x + r.w).toBeCloseTo(300);
    expect(r.y + r.h).toBeCloseTo(150);
  });
  it("an edge handle stretches one side only", () => {
    const r = resizeAnnotation(img, "e", 50, 0) as ImageAnnotation;
    expect(r).toMatchObject({ w: 250, h: 100 });
  });
  it("takes opacity and shadow, nothing else", () => {
    const r = restyleAnnotation(img, { color: "#000", width: 9, opacity: 0.5, shadow: true, fontSize: 40 }) as Annotation & Record<string, unknown>;
    expect(r.opacity).toBe(0.5);
    expect(r.shadow).toBe(true);
    expect(r.color).toBeUndefined();
    expect(r.width).toBeUndefined();
    expect(styleOfAnnotation({ ...img, opacity: 0.4, shadow: true })).toEqual({ opacity: 0.4, shadow: true });
  });
  it("+/- scales it around its centre", () => {
    const r = resizeBy(img, 1) as ImageAnnotation;
    expect(r.w).toBeGreaterThan(200);
    expect(r.w / r.h).toBeCloseTo(2);
    expect(r.x + r.w / 2).toBeCloseTo(200);
    expect(scaleImage(img, 0.5)).toMatchObject({ w: 100, h: 50, x: 150, y: 75 });
  });
  it("a turn moves it by its centre and keeps it upright", () => {
    const d = transformDoc({ annotations: [img], crop: null, background: null }, 400, 200, "cw");
    const r = d.annotations[0] as ImageAnnotation;
    expect(r.w).toBe(200);
    expect(r.h).toBe(100);
    // centre (200,100) turned clockwise in a 400x200 image → (100,200)
    expect(r.x + r.w / 2).toBeCloseTo(100);
    expect(r.y + r.h / 2).toBeCloseTo(200);
  });
});
