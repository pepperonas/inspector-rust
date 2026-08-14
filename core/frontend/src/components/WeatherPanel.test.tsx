import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen, cleanup } from "@testing-library/react";
import type { WeatherReport } from "../lib/ipc";

const { weatherFetch } = vi.hoisted(() => ({ weatherFetch: vi.fn() }));
vi.mock("../lib/ipc", async (importOriginal) => ({
  // Keep the real types/constants (WEATHER_NO_KEY etc.) — only the network
  // call is replaced.
  ...(await importOriginal<typeof import("../lib/ipc")>()),
  weatherFetch,
  setWeatherConfig: vi.fn(async () => undefined),
}));

import { WeatherPanel } from "./WeatherPanel";

// 12:00 UTC slots on an arbitrary fixed day; tz_offset 7200 = Berlin summer.
const slotUtc = (hour: number) => Date.UTC(2026, 7, 14, hour, 0, 0) / 1000;

const REPORT: WeatherReport = {
  location: "Berlin, DE",
  lat: 52.5,
  lon: 13.4,
  current: {
    temp: 21.4,
    feels_like: 21.0,
    temp_min: 18,
    temp_max: 24,
    humidity: 55,
    pressure: 1013,
    wind_speed: 3.4,
    wind_deg: 200,
    description: "leicht bewölkt",
    icon: "02d",
    kind: "clouds",
    sunrise: 0,
    sunset: 0,
  },
  hourly: [
    { dt: slotUtc(12), temp: 21.4, icon: "02d", kind: "clouds", description: "bewölkt", pop: 0 },
    { dt: slotUtc(15), temp: 23.1, icon: "10d", kind: "rain", description: "regen", pop: 0.62 },
    { dt: slotUtc(18), temp: 20.0, icon: "02d", kind: "clouds", description: "bewölkt", pop: 0.1 },
  ],
  daily: [],
  tz_offset: 7200,
  units: "metric",
};

afterEach(cleanup);
beforeEach(() => {
  weatherFetch.mockReset();
  weatherFetch.mockResolvedValue(REPORT);
});

describe("WeatherPanel — next-12-hours strip (v0.106.0)", () => {
  it("labels the hourly slots in the LOCATION's local time", async () => {
    render(<WeatherPanel focused={false} arg="" onExit={() => undefined} />);
    expect(await screen.findByText("Next 12 hours")).toBeTruthy();
    // 12:00/15:00/18:00 UTC + tz_offset 7200 → 14:00/17:00/20:00 local.
    expect(screen.getByText("14:00")).toBeTruthy();
    expect(screen.getByText("17:00")).toBeTruthy();
    expect(screen.getByText("20:00")).toBeTruthy();
    expect(screen.getByText("23°")).toBeTruthy(); // slot temp, rounded
  });

  it("shows the rain-probability badge only from 20 %", async () => {
    render(<WeatherPanel focused={false} arg="" onExit={() => undefined} />);
    expect(await screen.findByText("62%")).toBeTruthy(); // pop 0.62
    expect(screen.queryByText("10%")).toBeNull(); // pop 0.1 → below the bar
    expect(screen.queryByText("0%")).toBeNull(); // pop 0 → no badge
  });

  it("omits the section entirely when no hourly data came back", async () => {
    weatherFetch.mockResolvedValue({ ...REPORT, hourly: [] });
    render(<WeatherPanel focused={false} arg="" onExit={() => undefined} />);
    expect(await screen.findByText("Berlin, DE")).toBeTruthy();
    expect(screen.queryByText("Next 12 hours")).toBeNull();
  });
});
