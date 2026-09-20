// Pure logic for the btsniff Bluetooth-capture analyzer panel.
// Mirrors the Rust wire types (serde snake_case) and holds the filter / search
// / formatting / hex-dump helpers so they can be unit-tested without a backend.

export type BtDirection = "tx" | "rx" | "unknown";

export type BtProtocol =
  | "hci_cmd"
  | "hci_evt"
  | "acl"
  | "sco"
  | "l2cap"
  | "l2cap_sig"
  | "att"
  | "smp"
  | "other"
  | "unknown";

export interface BtPacketSlim {
  index: number;
  t_us: number;
  abs_ms: number;
  direction: BtDirection;
  protocol: BtProtocol;
  handle: number | null;
  opcode: string | null;
  att_handle: number | null;
  uuid: string | null;
  length: number;
  summary: string;
}

export interface BtPacketDetail {
  index: number;
  direction: BtDirection;
  protocol: BtProtocol;
  handle: number | null;
  opcode: string | null;
  att_handle: number | null;
  uuid: string | null;
  t_us: number;
  abs_ms: number;
  raw: number[];
  decoded: [string, string][];
  prev_index: number | null;
  diff: boolean[] | null;
}

export interface CaptureStats {
  packets: number;
  tx_packets: number;
  rx_packets: number;
  bytes: number;
  tx_bytes: number;
  rx_bytes: number;
  att_writes: number;
  att_reads: number;
  notifications: number;
  indications: number;
  duration_us: number;
  unique_handles: number;
  unique_att_handles: number;
  unique_uuids: number;
}

export interface OpenResult {
  format: string;
  source_label: string;
  packets: BtPacketSlim[];
  stats: CaptureStats;
}

// ── live capture (Stage 2) ──

export type CaptureState =
  | "unavailable"
  | "idle"
  | "starting"
  | "capturing"
  | "paused_view"
  | "stopping"
  | "stopped"
  | "error";

export type BackendAvailability = "available" | "unsupported_platform";

export interface SetupStatus {
  availability: BackendAvailability;
  message: string;
  source_label: string;
}

export interface LiveStatus {
  state: CaptureState;
  error: string | null;
  started_at_ms: number;
  view_paused: boolean;
  packets: number;
  buffered: number;
  dropped: number;
  stats: CaptureStats;
}

/** Is the session actively running (backend live), incl. a frozen view? */
export function isLiveRunning(state: CaptureState): boolean {
  return state === "starting" || state === "capturing" || state === "paused_view";
}

/** Live clock: `MM:SS.mmm`, or `H:MM:SS.mmm` once past an hour (§3/§4). */
export function formatLiveDuration(ms: number): string {
  if (ms < 0) ms = 0;
  const totalMs = Math.floor(ms);
  const msPart = totalMs % 1000;
  const totalSec = Math.floor(totalMs / 1000);
  const s = totalSec % 60;
  const m = Math.floor(totalSec / 60) % 60;
  const h = Math.floor(totalSec / 3600);
  const p2 = (n: number) => String(n).padStart(2, "0");
  const p3 = (n: number) => String(n).padStart(3, "0");
  return h > 0 ? `${h}:${p2(m)}:${p2(s)}.${p3(msPart)}` : `${p2(m)}:${p2(s)}.${p3(msPart)}`;
}

/** Default capture filename (§3), timestamped, no device data. */
export function defaultCaptureFilename(now: Date): string {
  const p2 = (n: number) => String(n).padStart(2, "0");
  const stamp =
    `${now.getFullYear()}-${p2(now.getMonth() + 1)}-${p2(now.getDate())}` +
    `_${p2(now.getHours())}-${p2(now.getMinutes())}-${p2(now.getSeconds())}`;
  return `inspector-bluetooth-${stamp}.pklg`;
}

/** Short uppercase tag for the TYPE column — mirrors Rust `BtProtocol::tag`. */
export function protocolTag(p: BtProtocol): string {
  switch (p) {
    case "hci_cmd":
      return "CMD";
    case "hci_evt":
      return "EVT";
    case "acl":
      return "ACL";
    case "sco":
      return "SCO";
    case "l2cap":
    case "l2cap_sig":
      return "L2CAP";
    case "att":
      return "ATT";
    case "smp":
      return "SMP";
    case "other":
      return "—";
    default:
      return "?";
  }
}

// ── filtering ──

export type BtFilter = "all" | "hci" | "acl" | "l2cap" | "att" | "gatt" | "tx" | "rx";

export const BT_FILTERS: { id: BtFilter; label: string }[] = [
  { id: "all", label: "Alle" },
  { id: "hci", label: "HCI" },
  { id: "acl", label: "ACL" },
  { id: "l2cap", label: "L2CAP" },
  { id: "att", label: "ATT" },
  { id: "gatt", label: "GATT" },
  { id: "tx", label: "→ TX" },
  { id: "rx", label: "← RX" },
];

/** GATT operations — the value-bearing + discovery ATT PDUs. Both the GATT
 * filter and the timeline's "emphasise this op" styling key off this. */
const GATT_OPS = new Set([
  "Read Request",
  "Read Response",
  "Read Blob Request",
  "Read Blob Response",
  "Read By Type Request",
  "Read By Type Response",
  "Read By Group Type Request",
  "Read By Group Type Response",
  "Write Request",
  "Write Response",
  "Write Command",
  "Signed Write Command",
  "Prepare Write Request",
  "Prepare Write Response",
  "Execute Write Request",
  "Execute Write Response",
  "Handle Value Notification",
  "Handle Value Indication",
  "Handle Value Confirmation",
  "Find Information Request",
  "Find Information Response",
]);

export function isGattOp(opcode: string | null): boolean {
  return opcode != null && GATT_OPS.has(opcode);
}

export function matchesFilter(p: BtPacketSlim, filter: BtFilter): boolean {
  switch (filter) {
    case "all":
      return true;
    case "hci":
      return p.protocol === "hci_cmd" || p.protocol === "hci_evt";
    case "acl":
      return p.protocol === "acl";
    case "l2cap":
      return p.protocol === "l2cap" || p.protocol === "l2cap_sig";
    case "att":
      return p.protocol === "att";
    case "gatt":
      return p.protocol === "att" && isGattOp(p.opcode);
    case "tx":
      return p.direction === "tx";
    case "rx":
      return p.direction === "rx";
  }
}

// ── search ──

/** All whitespace-separated terms must appear (case-insensitive) across the
 * packet's summary / opcode / type tag / handles / uuid. */
export function matchesSearch(p: BtPacketSlim, query: string): boolean {
  const terms = query.trim().toLowerCase().split(/\s+/).filter(Boolean);
  if (terms.length === 0) return true;
  const hay = [
    p.summary,
    p.opcode ?? "",
    protocolTag(p.protocol),
    formatHandle(p.handle),
    formatHandle(p.att_handle),
    p.uuid ?? "",
  ]
    .join(" ")
    .toLowerCase();
  return terms.every((t) => hay.includes(t));
}

export function filterPackets(
  packets: BtPacketSlim[],
  filter: BtFilter,
  query: string,
): BtPacketSlim[] {
  return packets.filter((p) => matchesFilter(p, filter) && matchesSearch(p, query));
}

// ── display formatting ──

export function directionGlyph(d: BtDirection): string {
  return d === "tx" ? "→" : d === "rx" ? "←" : "·";
}

export function directionLabel(d: BtDirection): string {
  return d === "tx" ? "TX" : d === "rx" ? "RX" : "—";
}

/** `0x0040`, or `—` when absent. */
export function formatHandle(h: number | null): string {
  if (h == null) return "—";
  return "0x" + h.toString(16).toUpperCase().padStart(4, "0");
}

/** Relative timestamp in seconds with millisecond precision (`12.345`). */
export function formatRelTime(t_us: number): string {
  return (t_us / 1_000_000).toFixed(3);
}

export function formatBytes(n: number): string {
  if (n < 1024) return `${n} B`;
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} KB`;
  return `${(n / (1024 * 1024)).toFixed(2)} MB`;
}

/** Session duration from microseconds → `1.23 s` or `2:05.4`. */
export function formatDuration(us: number): string {
  const s = us / 1_000_000;
  if (s < 60) return `${s.toFixed(2)} s`;
  const m = Math.floor(s / 60);
  const rem = s - m * 60;
  return `${m}:${rem.toFixed(1).padStart(4, "0")}`;
}

// ── hex dump / copy ──

export interface HexRow {
  offset: string;
  hex: string;
  ascii: string;
}

/** Classic offset / hex / ASCII dump. `changed` (from the diff mask) marks
 * which byte indices differ from the previous comparable packet. */
export function hexDumpRows(bytes: number[], perRow = 16): HexRow[] {
  const rows: HexRow[] = [];
  for (let i = 0; i < bytes.length; i += perRow) {
    const slice = bytes.slice(i, i + perRow);
    const hex = slice.map((b) => b.toString(16).padStart(2, "0")).join(" ");
    const ascii = slice
      .map((b) => (b >= 0x20 && b < 0x7f ? String.fromCharCode(b) : "."))
      .join("");
    rows.push({
      offset: i.toString(16).padStart(4, "0"),
      hex,
      ascii,
    });
  }
  return rows;
}

/** Space-separated lowercase hex for the "Copy Hex" action. */
export function hexString(bytes: number[]): string {
  return bytes.map((b) => b.toString(16).padStart(2, "0")).join(" ");
}

/** The decoded field list as `Label: value` lines for "Copy Decoded". */
export function decodedText(decoded: [string, string][]): string {
  return decoded.map(([k, v]) => `${k}: ${v}`).join("\n");
}

/** Whether byte `i` changed vs the previous packet (safe on a null mask). */
export function diffChanged(diff: boolean[] | null, i: number): boolean {
  return diff != null && diff[i] === true;
}
