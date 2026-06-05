import { describe, it, expect, beforeEach } from "vitest";
import {
  loadHighScore,
  saveHighScore,
  commitHighScore,
  loadSavedGame,
  saveGame,
  clearSavedGame,
} from "./game-storage";

beforeEach(() => {
  localStorage.clear();
});

describe("high score", () => {
  it("returns 0 when nothing is stored", () => {
    expect(loadHighScore("pong")).toBe(0);
  });

  it("round-trips a saved value", () => {
    saveHighScore("snake-classic", 42);
    expect(loadHighScore("snake-classic")).toBe(42);
  });

  it("keeps the two Snake variants separate", () => {
    saveHighScore("snake-classic", 10);
    saveHighScore("snake-wrap", 7);
    expect(loadHighScore("snake-classic")).toBe(10);
    expect(loadHighScore("snake-wrap")).toBe(7);
  });

  it("floors and clamps negatives away", () => {
    saveHighScore("space", 12.9);
    expect(loadHighScore("space")).toBe(12);
    saveHighScore("space", -5);
    expect(loadHighScore("space")).toBe(0);
  });

  it("treats a corrupt value as 0", () => {
    localStorage.setItem("inspector-rust.game.pong.best", "not-a-number");
    expect(loadHighScore("pong")).toBe(0);
  });
});

describe("commitHighScore", () => {
  it("only raises the stored best, never lowers it", () => {
    expect(commitHighScore("space", 30)).toBe(30);
    expect(commitHighScore("space", 10)).toBe(30); // worse run doesn't overwrite
    expect(loadHighScore("space")).toBe(30);
    expect(commitHighScore("space", 55)).toBe(55); // better run wins
    expect(loadHighScore("space")).toBe(55);
  });
});

describe("suspended run", () => {
  it("returns null when nothing is stored", () => {
    expect(loadSavedGame("pong")).toBeNull();
  });

  it("round-trips an arbitrary state object", () => {
    const state = { score: 3, snake: [{ x: 1, y: 2 }], dir: "left" };
    saveGame("snake-wrap", state);
    expect(loadSavedGame("snake-wrap")).toEqual(state);
  });

  it("is cleared independently of the high score", () => {
    saveHighScore("space", 99);
    saveGame("space", { score: 12 });
    clearSavedGame("space");
    expect(loadSavedGame("space")).toBeNull();
    expect(loadHighScore("space")).toBe(99); // best survives a state clear
  });

  it("treats corrupt JSON as no saved run", () => {
    localStorage.setItem("inspector-rust.game.pong.state", "{not json");
    expect(loadSavedGame("pong")).toBeNull();
  });
});
