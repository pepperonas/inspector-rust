/**
 * Pure helpers for the `clock` world-clock panel (v0.121.0): the IANA
 * timezone/city catalogue, autocomplete matching, and time formatting via the
 * platform `Intl` API (no data tables — the OS owns DST + offsets). The
 * component owns only rendering + the tick.
 */

export interface CityZone {
  /** Display city name. */
  city: string;
  /** Country/region label for the autocomplete second line. */
  region: string;
  /** IANA timezone id (the stable key — what's persisted). */
  tz: string;
}

/**
 * Curated set of major world cities + capitals, one per relevant timezone,
 * spanning every UTC offset in use. IANA ids are the source of truth for
 * offset + DST (resolved live by `Intl`). Kept broad but not exhaustive —
 * autocomplete also matches on the tz id, so any IANA zone the OS knows is
 * reachable by typing e.g. "pacific/auckland".
 */
export const CITY_ZONES: readonly CityZone[] = [
  { city: "Honolulu", region: "USA · Hawaii", tz: "Pacific/Honolulu" },
  { city: "Anchorage", region: "USA · Alaska", tz: "America/Anchorage" },
  { city: "Los Angeles", region: "USA · Kalifornien", tz: "America/Los_Angeles" },
  { city: "Vancouver", region: "Kanada", tz: "America/Vancouver" },
  { city: "Denver", region: "USA · Colorado", tz: "America/Denver" },
  { city: "Mexico City", region: "Mexiko", tz: "America/Mexico_City" },
  { city: "Chicago", region: "USA · Illinois", tz: "America/Chicago" },
  { city: "New York", region: "USA · Ostküste", tz: "America/New_York" },
  { city: "Toronto", region: "Kanada", tz: "America/Toronto" },
  { city: "Bogotá", region: "Kolumbien", tz: "America/Bogota" },
  { city: "Lima", region: "Peru", tz: "America/Lima" },
  { city: "Santiago", region: "Chile", tz: "America/Santiago" },
  { city: "Caracas", region: "Venezuela", tz: "America/Caracas" },
  { city: "São Paulo", region: "Brasilien", tz: "America/Sao_Paulo" },
  { city: "Buenos Aires", region: "Argentinien", tz: "America/Argentina/Buenos_Aires" },
  { city: "Azoren", region: "Portugal", tz: "Atlantic/Azores" },
  { city: "Reykjavík", region: "Island", tz: "Atlantic/Reykjavik" },
  { city: "London", region: "Vereinigtes Königreich", tz: "Europe/London" },
  { city: "Lissabon", region: "Portugal", tz: "Europe/Lisbon" },
  { city: "Dublin", region: "Irland", tz: "Europe/Dublin" },
  { city: "Berlin", region: "Deutschland", tz: "Europe/Berlin" },
  { city: "Paris", region: "Frankreich", tz: "Europe/Paris" },
  { city: "Madrid", region: "Spanien", tz: "Europe/Madrid" },
  { city: "Rom", region: "Italien", tz: "Europe/Rome" },
  { city: "Amsterdam", region: "Niederlande", tz: "Europe/Amsterdam" },
  { city: "Zürich", region: "Schweiz", tz: "Europe/Zurich" },
  { city: "Wien", region: "Österreich", tz: "Europe/Vienna" },
  { city: "Stockholm", region: "Schweden", tz: "Europe/Stockholm" },
  { city: "Warschau", region: "Polen", tz: "Europe/Warsaw" },
  { city: "Athen", region: "Griechenland", tz: "Europe/Athens" },
  { city: "Helsinki", region: "Finnland", tz: "Europe/Helsinki" },
  { city: "Istanbul", region: "Türkei", tz: "Europe/Istanbul" },
  { city: "Kairo", region: "Ägypten", tz: "Africa/Cairo" },
  { city: "Johannesburg", region: "Südafrika", tz: "Africa/Johannesburg" },
  { city: "Lagos", region: "Nigeria", tz: "Africa/Lagos" },
  { city: "Nairobi", region: "Kenia", tz: "Africa/Nairobi" },
  { city: "Moskau", region: "Russland", tz: "Europe/Moscow" },
  { city: "Riad", region: "Saudi-Arabien", tz: "Asia/Riyadh" },
  { city: "Dubai", region: "VAE", tz: "Asia/Dubai" },
  { city: "Teheran", region: "Iran", tz: "Asia/Tehran" },
  { city: "Karatschi", region: "Pakistan", tz: "Asia/Karachi" },
  { city: "Mumbai", region: "Indien", tz: "Asia/Kolkata" },
  { city: "Kathmandu", region: "Nepal", tz: "Asia/Kathmandu" },
  { city: "Dhaka", region: "Bangladesch", tz: "Asia/Dhaka" },
  { city: "Bangkok", region: "Thailand", tz: "Asia/Bangkok" },
  { city: "Jakarta", region: "Indonesien", tz: "Asia/Jakarta" },
  { city: "Singapur", region: "Singapur", tz: "Asia/Singapore" },
  { city: "Hongkong", region: "China", tz: "Asia/Hong_Kong" },
  { city: "Peking", region: "China", tz: "Asia/Shanghai" },
  { city: "Taipeh", region: "Taiwan", tz: "Asia/Taipei" },
  { city: "Seoul", region: "Südkorea", tz: "Asia/Seoul" },
  { city: "Tokio", region: "Japan", tz: "Asia/Tokyo" },
  { city: "Adelaide", region: "Australien", tz: "Australia/Adelaide" },
  { city: "Sydney", region: "Australien", tz: "Australia/Sydney" },
  { city: "Brisbane", region: "Australien", tz: "Australia/Brisbane" },
  { city: "Auckland", region: "Neuseeland", tz: "Pacific/Auckland" },
  { city: "Suva", region: "Fidschi", tz: "Pacific/Fiji" },
];

/** Default set on first use — a spread across the main business regions. */
export const DEFAULT_ZONES: readonly string[] = [
  "America/Los_Angeles",
  "America/New_York",
  "Europe/London",
  "Europe/Berlin",
  "Asia/Dubai",
  "Asia/Kolkata",
  "Asia/Shanghai",
  "Asia/Tokyo",
  "Australia/Sydney",
];

/** Look up a catalogue entry by tz id. */
export function zoneByTz(tz: string): CityZone | undefined {
  return CITY_ZONES.find((z) => z.tz === tz);
}

/** Display a tz that isn't in the catalogue: last path segment, underscores
 *  to spaces ("Pacific/Chatham" → "Chatham"). */
export function tzFallbackCity(tz: string): string {
  const seg = tz.split("/").pop() ?? tz;
  return seg.replace(/_/g, " ");
}

/**
 * Autocomplete: match `query` against city, region and tz id. City-prefix
 * ranks first, then city-substring, then region/tz substring; ties by city
 * name. Already-added zones are excluded. Empty query → [].
 */
export function matchCities(
  query: string,
  added: readonly string[],
  limit = 8,
): CityZone[] {
  const q = query.trim().toLowerCase();
  if (!q) return [];
  const addedSet = new Set(added);
  const scored: { z: CityZone; score: number }[] = [];
  for (const z of CITY_ZONES) {
    if (addedSet.has(z.tz)) continue;
    const city = z.city.toLowerCase();
    const region = z.region.toLowerCase();
    const tz = z.tz.toLowerCase();
    let score: number;
    if (city.startsWith(q)) score = 0;
    else if (city.includes(q)) score = 1;
    else if (region.includes(q)) score = 2;
    else if (tz.includes(q)) score = 3;
    else continue;
    scored.push({ z, score });
  }
  scored.sort((a, b) => a.score - b.score || a.z.city.localeCompare(b.z.city));
  return scored.slice(0, limit).map((s) => s.z);
}

/** Sanitize/dedupe a persisted zone list; drop unknown-format ids defensively
 *  but KEEP valid-looking IANA ids not in the catalogue (user added by tz). */
export function normalizeZones(zones: unknown): string[] {
  if (!Array.isArray(zones)) return [...DEFAULT_ZONES];
  const seen = new Set<string>();
  const out: string[] = [];
  for (const z of zones) {
    if (typeof z !== "string") continue;
    const t = z.trim();
    // A plausible IANA id: "Area/Location" or "UTC".
    if (!/^[A-Za-z]+(?:\/[A-Za-z0-9_+-]+){0,2}$/.test(t)) continue;
    if (seen.has(t)) continue;
    seen.add(t);
    out.push(t);
  }
  return out;
}

export interface ZoneTime {
  /** "HH:MM" in the zone. */
  time: string;
  /** ":SS" for a subtle seconds tick. */
  seconds: string;
  /** e.g. "Mo., 25. Aug." */
  date: string;
  /** Day-offset vs the local day: -1 (yesterday), 0 (today), +1 (tomorrow). */
  dayDelta: number;
  /** UTC offset label, e.g. "UTC+2". */
  offset: string;
  /** True when it's night there (before 6:00 or from 20:00) — drives the
   *  sun/moon accent. */
  night: boolean;
}

/** Format `date` in `tz` via Intl. Throwing tz ids (should never happen
 *  post-normalize) fall back to a dashy placeholder. */
export function zoneTime(date: Date, tz: string): ZoneTime {
  try {
    const parts = new Intl.DateTimeFormat("de-DE", {
      timeZone: tz,
      hour: "2-digit",
      minute: "2-digit",
      second: "2-digit",
      weekday: "short",
      day: "numeric",
      month: "short",
      hour12: false,
    }).formatToParts(date);
    const get = (t: string) => parts.find((p) => p.type === t)?.value ?? "";
    const hour = Number(get("hour"));
    const time = `${get("hour")}:${get("minute")}`;
    const seconds = `:${get("second")}`;
    const dateStr = `${get("weekday")}, ${get("day")}. ${get("month")}`;
    return {
      time,
      seconds,
      date: dateStr,
      dayDelta: dayDelta(date, tz),
      offset: utcOffset(date, tz),
      night: hour < 6 || hour >= 20,
    };
  } catch {
    return { time: "--:--", seconds: ":--", date: "—", dayDelta: 0, offset: "", night: false };
  }
}

/** Which calendar day the instant lands on in `tz` vs a reference zone
 *  (default: the machine's local zone — the day-delta chip is relative to the
 *  user's own day): -1/0/+1. `refTz` is injectable so tests can pin it to a
 *  fixed zone (the machine's zone is unknowable in CI). Compares the
 *  yyyy-mm-dd of the same instant in each zone. */
export function dayDelta(date: Date, tz: string, refTz?: string): number {
  const dayIn = (z?: string) =>
    new Intl.DateTimeFormat("en-CA", {
      timeZone: z,
      year: "numeric",
      month: "2-digit",
      day: "2-digit",
    }).format(date);
  const there = dayIn(tz);
  const here = dayIn(refTz); // undefined → local zone
  if (there === here) return 0;
  return there > here ? 1 : -1;
}

/** UTC offset of `tz` at `date` as "UTC±H[:MM]" via the longOffset format. */
export function utcOffset(date: Date, tz: string): string {
  const parts = new Intl.DateTimeFormat("en-US", {
    timeZone: tz,
    timeZoneName: "shortOffset",
  }).formatToParts(date);
  const raw = parts.find((p) => p.type === "timeZoneName")?.value ?? "";
  // Intl gives "GMT+2" / "GMT+5:30" / "GMT" — normalise to "UTC…".
  return raw.replace("GMT", "UTC") || "UTC";
}
