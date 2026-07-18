import { describe, it, expect, beforeEach, afterEach, vi } from "vitest";
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
    saveHighScore("flappy", 12.9);
    expect(loadHighScore("flappy")).toBe(12);
    saveHighScore("flappy", -5);
    expect(loadHighScore("flappy")).toBe(0);
  });

  it("treats a corrupt value as 0", () => {
    localStorage.setItem("inspector-rust.game.pong.best", "not-a-number");
    expect(loadHighScore("pong")).toBe(0);
  });
});

describe("commitHighScore", () => {
  it("only raises the stored best, never lowers it", () => {
    expect(commitHighScore("flappy", 30)).toBe(30);
    expect(commitHighScore("flappy", 10)).toBe(30); // worse run doesn't overwrite
    expect(loadHighScore("flappy")).toBe(30);
    expect(commitHighScore("flappy", 55)).toBe(55); // better run wins
    expect(loadHighScore("flappy")).toBe(55);
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
    saveHighScore("flappy", 99);
    saveGame("flappy", { score: 12 });
    clearSavedGame("flappy");
    expect(loadSavedGame("flappy")).toBeNull();
    expect(loadHighScore("flappy")).toBe(99); // best survives a state clear
  });

  it("treats corrupt JSON as no saved run", () => {
    localStorage.setItem("inspector-rust.game.pong.state", "{not json");
    expect(loadSavedGame("pong")).toBeNull();
  });

  it("keeps suspended runs separate per game", () => {
    saveGame("pong", { wins: 2 });
    expect(loadSavedGame("space")).toBeNull();
    expect(loadSavedGame("pong")).toEqual({ wins: 2 });
  });

  it("round-trips Unicode content in the state", () => {
    const state = { player: "Grüße 🚀", note: "üöä" };
    saveGame("space", state);
    expect(loadSavedGame("space")).toEqual(state);
  });

  it("clearing a missing run is a no-op", () => {
    expect(() => clearSavedGame("never-saved")).not.toThrow();
    expect(loadSavedGame("never-saved")).toBeNull();
  });
});

describe("high score — hostile stored values", () => {
  it("degrades NaN / Infinity saves to 0 on load", () => {
    saveHighScore("pong", NaN); // stores "NaN"
    expect(loadHighScore("pong")).toBe(0);
    saveHighScore("pong", Infinity); // stores "Infinity" → parseInt NaN
    expect(loadHighScore("pong")).toBe(0);
  });

  it("treats zero and negative stored strings as no high score", () => {
    localStorage.setItem("inspector-rust.game.pong.best", "0");
    expect(loadHighScore("pong")).toBe(0);
    localStorage.setItem("inspector-rust.game.pong.best", "-3");
    expect(loadHighScore("pong")).toBe(0);
  });

  it("parses a decimal stored string by truncation", () => {
    localStorage.setItem("inspector-rust.game.pong.best", "12.9");
    expect(loadHighScore("pong")).toBe(12);
  });

  it("commitHighScore never stores below 0", () => {
    expect(commitHighScore("pong", -5)).toBe(0);
    expect(loadHighScore("pong")).toBe(0);
  });
});

describe("throwing localStorage degrades to 'no saved data'", () => {
  const throwing = {
    getItem: () => {
      throw new Error("storage disabled");
    },
    setItem: () => {
      throw new Error("quota exceeded");
    },
    removeItem: () => {
      throw new Error("storage disabled");
    },
  } as unknown as Storage;

  beforeEach(() => {
    vi.stubGlobal("localStorage", throwing);
  });
  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it("loads report empty instead of crashing", () => {
    expect(loadHighScore("pong")).toBe(0);
    expect(loadSavedGame("pong")).toBeNull();
  });

  it("saves and clears are silent no-ops", () => {
    expect(() => saveHighScore("pong", 10)).not.toThrow();
    expect(() => saveGame("pong", { a: 1 })).not.toThrow();
    expect(() => clearSavedGame("pong")).not.toThrow();
  });

  it("commitHighScore still returns the in-memory best", () => {
    // Nothing persists, but the game can keep showing the session best.
    expect(commitHighScore("pong", 17)).toBe(17);
  });
});
