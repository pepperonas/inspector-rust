import { describe, it, expect } from "vitest";
import {
  protocolTag,
  matchesFilter,
  matchesSearch,
  filterPackets,
  isGattOp,
  directionGlyph,
  directionLabel,
  formatHandle,
  formatRelTime,
  formatBytes,
  formatDuration,
  hexDumpRows,
  hexString,
  decodedText,
  diffChanged,
  isLiveRunning,
  formatLiveDuration,
  defaultCaptureFilename,
  type BtPacketSlim,
} from "./bluetooth-capture";

function slim(over: Partial<BtPacketSlim>): BtPacketSlim {
  return {
    index: 0,
    t_us: 0,
    abs_ms: 0,
    direction: "tx",
    protocol: "att",
    handle: 0x0040,
    opcode: "Write Command",
    att_handle: 0x0025,
    uuid: "2A19",
    length: 4,
    summary: "Write Command · handle 0x0025",
    ...over,
  };
}

describe("protocolTag", () => {
  it("mirrors the Rust tags", () => {
    expect(protocolTag("hci_cmd")).toBe("CMD");
    expect(protocolTag("hci_evt")).toBe("EVT");
    expect(protocolTag("l2cap_sig")).toBe("L2CAP");
    expect(protocolTag("att")).toBe("ATT");
    expect(protocolTag("other")).toBe("—");
    expect(protocolTag("unknown")).toBe("?");
  });
});

describe("matchesFilter", () => {
  it("all lets everything through", () => {
    expect(matchesFilter(slim({ protocol: "hci_cmd" }), "all")).toBe(true);
  });
  it("hci matches both command and event", () => {
    expect(matchesFilter(slim({ protocol: "hci_cmd" }), "hci")).toBe(true);
    expect(matchesFilter(slim({ protocol: "hci_evt" }), "hci")).toBe(true);
    expect(matchesFilter(slim({ protocol: "att" }), "hci")).toBe(false);
  });
  it("l2cap includes the signaling channel", () => {
    expect(matchesFilter(slim({ protocol: "l2cap" }), "l2cap")).toBe(true);
    expect(matchesFilter(slim({ protocol: "l2cap_sig" }), "l2cap")).toBe(true);
    expect(matchesFilter(slim({ protocol: "att" }), "l2cap")).toBe(false);
  });
  it("gatt is att with a GATT op, plain att is not gatt", () => {
    expect(matchesFilter(slim({ protocol: "att", opcode: "Write Command" }), "gatt")).toBe(true);
    expect(matchesFilter(slim({ protocol: "att", opcode: "Exchange MTU Request" }), "gatt")).toBe(
      false,
    );
    // a GATT op that isn't over ATT can't exist, but guard anyway
    expect(matchesFilter(slim({ protocol: "acl", opcode: "Write Command" }), "gatt")).toBe(false);
  });
  it("tx / rx match direction", () => {
    expect(matchesFilter(slim({ direction: "tx" }), "tx")).toBe(true);
    expect(matchesFilter(slim({ direction: "rx" }), "tx")).toBe(false);
    expect(matchesFilter(slim({ direction: "rx" }), "rx")).toBe(true);
  });
});

describe("isGattOp", () => {
  it("recognises value/discovery ops, rejects others and null", () => {
    expect(isGattOp("Handle Value Notification")).toBe(true);
    expect(isGattOp("Read Request")).toBe(true);
    expect(isGattOp("Exchange MTU Request")).toBe(false);
    expect(isGattOp(null)).toBe(false);
  });
});

describe("matchesSearch", () => {
  it("empty query matches everything", () => {
    expect(matchesSearch(slim({}), "")).toBe(true);
    expect(matchesSearch(slim({}), "   ")).toBe(true);
  });
  it("matches summary, opcode, tag, handle hex and uuid", () => {
    const p = slim({});
    expect(matchesSearch(p, "write")).toBe(true); // opcode/summary
    expect(matchesSearch(p, "att")).toBe(true); // tag
    expect(matchesSearch(p, "0x0025")).toBe(true); // att handle hex
    expect(matchesSearch(p, "2a19")).toBe(true); // uuid, case-insensitive
    expect(matchesSearch(p, "zzz")).toBe(false);
  });
  it("requires every whitespace term", () => {
    const p = slim({});
    expect(matchesSearch(p, "write 0x0025")).toBe(true);
    expect(matchesSearch(p, "write 0x9999")).toBe(false);
  });
});

describe("filterPackets", () => {
  it("applies filter and search together", () => {
    const pkts = [
      slim({ index: 0, protocol: "hci_cmd", opcode: "Reset", summary: "Reset", att_handle: null }),
      slim({ index: 1, protocol: "att", opcode: "Write Command", summary: "write a" }),
      slim({ index: 2, protocol: "att", opcode: "Read Request", summary: "read b" }),
    ];
    const out = filterPackets(pkts, "gatt", "write");
    expect(out.map((p) => p.index)).toEqual([1]);
  });
});

describe("formatting", () => {
  it("direction glyphs and labels", () => {
    expect(directionGlyph("tx")).toBe("→");
    expect(directionGlyph("rx")).toBe("←");
    expect(directionGlyph("unknown")).toBe("·");
    expect(directionLabel("tx")).toBe("TX");
    expect(directionLabel("unknown")).toBe("—");
  });
  it("handle formatting", () => {
    expect(formatHandle(0x40)).toBe("0x0040");
    expect(formatHandle(0x2a19)).toBe("0x2A19");
    expect(formatHandle(null)).toBe("—");
  });
  it("relative time in seconds", () => {
    expect(formatRelTime(0)).toBe("0.000");
    expect(formatRelTime(1_234_567)).toBe("1.235");
  });
  it("bytes 1024-based", () => {
    expect(formatBytes(512)).toBe("512 B");
    expect(formatBytes(2048)).toBe("2.0 KB");
    expect(formatBytes(3 * 1024 * 1024)).toBe("3.00 MB");
  });
  it("duration", () => {
    expect(formatDuration(1_230_000)).toBe("1.23 s");
    expect(formatDuration(125_000_000)).toBe("2:05.0");
  });
});

describe("hex dump / copy", () => {
  it("rows split at 16 bytes with offset/hex/ascii", () => {
    const bytes = [0x52, 0x25, 0x00, 0x41, 0x42];
    const rows = hexDumpRows(bytes);
    expect(rows).toHaveLength(1);
    expect(rows[0].offset).toBe("0000");
    expect(rows[0].hex).toBe("52 25 00 41 42");
    expect(rows[0].ascii).toBe("R%.AB");
  });
  it("wraps to a second row past 16 bytes", () => {
    const bytes = Array.from({ length: 20 }, (_, i) => i);
    const rows = hexDumpRows(bytes);
    expect(rows).toHaveLength(2);
    expect(rows[1].offset).toBe("0010");
  });
  it("hexString is space-separated lowercase", () => {
    expect(hexString([0x52, 0x25, 0x00, 0x61])).toBe("52 25 00 61");
  });
  it("decodedText joins label: value lines", () => {
    expect(
      decodedText([
        ["ATT Opcode", "0x52"],
        ["ATT Handle", "0x0025"],
      ]),
    ).toBe("ATT Opcode: 0x52\nATT Handle: 0x0025");
  });
  it("diffChanged is safe on a null mask", () => {
    expect(diffChanged(null, 0)).toBe(false);
    expect(diffChanged([false, true], 1)).toBe(true);
    expect(diffChanged([false, true], 0)).toBe(false);
    expect(diffChanged([false], 9)).toBe(false);
  });
});

describe("live capture helpers", () => {
  it("isLiveRunning covers the active states only", () => {
    expect(isLiveRunning("starting")).toBe(true);
    expect(isLiveRunning("capturing")).toBe(true);
    expect(isLiveRunning("paused_view")).toBe(true);
    expect(isLiveRunning("idle")).toBe(false);
    expect(isLiveRunning("stopped")).toBe(false);
    expect(isLiveRunning("error")).toBe(false);
  });

  it("formatLiveDuration is MM:SS.mmm under an hour, H:MM:SS.mmm past it", () => {
    expect(formatLiveDuration(0)).toBe("00:00.000");
    expect(formatLiveDuration(1_042)).toBe("00:01.042");
    expect(formatLiveDuration(62_381)).toBe("01:02.381");
    expect(formatLiveDuration(3_600_000 + 62_381)).toBe("1:01:02.381");
    expect(formatLiveDuration(-5)).toBe("00:00.000");
  });

  it("defaultCaptureFilename is timestamped .pklg with no device data", () => {
    const d = new Date(2026, 2, 7, 5, 31, 42); // local time
    expect(defaultCaptureFilename(d)).toBe("inspector-bluetooth-2026-03-07_05-31-42.pklg");
  });
});
