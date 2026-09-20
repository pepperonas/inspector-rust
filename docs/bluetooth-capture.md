# Bluetooth capture analyzer — `btsniff`

`btsniff` (aliases `btcap`, `btcapture`, `blecap`) imports a Bluetooth HCI
capture file and decodes it down to the GATT layer, with a byte-diff view built
for reverse-engineering an app↔device protocol. It is a **separate command** from
`bt` / `bluetooth` (the device manager) and never touches it.

## Two modes

1. **Live BLE scan** (macOS) — a CoreBluetooth advertisement scanner that logs
   every advertising device live (see *Live capture* below).
2. **Import** — parse and decode an exported `.pklg` / btsnoop / `.pcapng`
   capture (fully cross-platform).

## What live capture is — and honestly is not

The live scan is a **BLE advertisement scanner**, not a promiscuous wire
sniffer. macOS deliberately exposes **no public API for promiscuous HCI/ACL
capture** — that path is gated behind Apple-private entitlements used only by
Apple's own PacketLogger. Rather than fake it or lean on undocumented private
interfaces, `btsniff` uses the public CoreBluetooth API to do what it reliably
can: scan for advertising BLE devices and log each advertisement. To reverse a
phone **app ↔ device** session (e.g. a scooter and its app), that traffic is
between two other endpoints — capture it on the phone (Android *Enable Bluetooth
HCI snoop log*) and **import** the `btsnoop_hci.log` here.

Where to get one:

| Format | Source | Typical name |
|---|---|---|
| `.pklg` | macOS **PacketLogger** (Additional Tools for Xcode) | `*.pklg` |
| btsnoop | **Android** HCI-snoop logging (Developer options → *Enable Bluetooth HCI snoop log*), or BlueZ `hcidump` | `btsnoop_hci.log` |
| `.pcapng` / `.pcap` | **Wireshark** (Bluetooth HCI H4, incl. the H4-with-PHDR pseudo-header) | `*.pcapng` |

For sniffing an Android app talking to a BLE device (a scooter, a lock, a
wearable), the usual path is: turn on the HCI snoop log on the phone, drive the
app, pull `btsnoop_hci.log`, and open it here.

## What it decodes

- **HCI**: commands (with a common OGF/OCF opcode-name table) and events
  (including LE Meta subevents like *LE Advertising Report* /
  *LE Connection Complete*).
- **ACL → L2CAP**: connection handle, L2CAP CID → channel (ATT / SMP / signaling
  / dynamic).
- **ATT / GATT**: every ATT opcode named, with the attribute handle and any
  16-/128-bit UUID pulled out. The value-bearing and discovery operations
  (read/write/notify/indicate, read-by-type/group, find-information) are the
  "GATT" set the **GATT** filter shows and the timeline emphasises.
- **SMP**: pairing PDUs named.

Direction is shown as **→ TX** (host → controller) and **← RX** (controller →
host), never colour-only. Command and event directions are derived from the
packet type itself (a command is always TX, an event always RX), so the
ambiguous btsnoop direction flag only ever affects ACL rows.

## The timeline + inspector

- A **virtualised** timeline (`TIME · DIR · TYPE · HANDLE · INFO`) scrolls a
  large session without lag.
- **Filters**: All / HCI / ACL / L2CAP / ATT / GATT / → TX / ← RX, plus a
  full-text search over opcode, type tag, handle hex and UUID.
- Selecting a packet opens the **inspector**: decoded field list, a hex dump,
  copy-hex / copy-decoded, and the **byte-diff**.

### Byte-diff (the reverse-engineering lever)

The inspector highlights the bytes that changed versus the **previous packet on
the same ATT handle** (falling back to the same connection handle). An app that
pokes one characteristic repeatedly writes a series of near-identical payloads;
the changing bytes are the payload field you care about, and they light up.

## Session summary + export

A deterministic summary (packet / TX / RX counts, bytes, duration, ATT
read/write/notify/indicate counts, unique handles / attribute handles / UUIDs)
is computed from the packets — never accumulated, so it can't drift.

**Export to JSON** writes the whole decoded session (every packet as hex + its
decoded fields + the summary) for use in another tool. The default filename is
timestamp-only, and any custom name has MAC-address runs stripped — a device
address never lands in a filename.

## Keyboard

- **↑ ↓** (or `j` / `k`), **PgUp / PgDn**, **Home / End** — move the selection.
- **Cmd/Ctrl+F** or **/** — focus the search field.
- **Esc** — leave the search field, or exit the panel.

## Privacy

Everything is local: the file is read, parsed and decoded on-device; nothing is
uploaded, and no telemetry is sent. The capture never leaves the machine unless
you export it yourself.

## Safety

Capture files are treated as untrusted input. Every length field is
bounds-checked, absurd sizes are rejected, and a corrupt or truncated file is
parsed best-effort — the readable packets survive rather than the whole import
failing. The parser has a fuzz test driving random buffers through all three
formats to guarantee it never panics.

## Live capture (BLE advertisement scanner, macOS)

Pressing **Capture starten** in the idle panel starts a CoreBluetooth scan. Each
advertising device becomes a live packet as `LE Advertising Report`, carrying:

- the device **identifier** (CoreBluetooth's per-app UUID — **not** the hardware
  MAC, which the OS hides; this suits the privacy stance),
- the **name** (advertised local name, else the peripheral name),
- **RSSI**,
- advertised **service UUIDs** (the first fills the timeline's UUID column),
- **manufacturer-data bytes** as the packet payload — so the hex dump + byte-diff
  let you watch a beacon's/sensor's payload bytes change over time.

Because CoreBluetooth hides real ATT handles, the timeline groups a device's
successive advertisements under a **synthetic per-device handle** (a hash of its
identifier) purely so the byte-diff works per device — it is not a real BT
connection handle, and the UI/docs say so.

**Controls (§9):** *Pause* freezes the **view only** — the backend keeps
scanning into a bounded buffer and the header shows `PAUSED · N gepuffert`;
*Resume* flushes the buffered packets at once. *Stop* ends the scan (releasing
the CoreBluetooth objects — no orphaned session). *Leeren* clears the display
after a confirm.

**State machine (§19):** `Idle → Starting → Capturing ⇄ PausedView → Stopping →
Stopped`, with `Error` reachable from any active state; illegal transitions are
rejected in the backend.

**Memory (§17):** the buffer is capped (`LIVE_MAX_PACKETS`); on overflow the
oldest packets are shed and counted (shown as "N Pakete verworfen") — never a
silent loss. Packets stream to the UI in ~120 ms batches, not one render per
packet, and the timeline is virtualised.

**Setup / permissions (§16):** live scan needs Bluetooth **on** and the
**Bluetooth permission** (System Settings → Privacy & Security → Bluetooth). If
Bluetooth is off/unauthorised/unsupported the panel shows a specific, actionable
message instead of a generic error. `NSBluetoothAlwaysUsageDescription` must be
present in the app's Info.plist for the permission prompt.

⚠️ **Verification status:** the CoreBluetooth FFI is compile-verified. Runtime
verification needs a Mac with Bluetooth on, the permission granted, and a nearby
advertising device — the pure logic it drives (packet mapping, state machine,
buffering, batching) is unit-tested independently.

## Roadmap

- **Connect + subscribe (full GATT live log)** — connect to a selected
  peripheral, discover services/characteristics and log reads/writes/notifications
  live. (Today's live mode logs advertisements only.)
- **`.pklg` / `.pcapng` writing** — the analyzer *reads* all three formats;
  writing synthesised advertisement records for Wireshark is a follow-up. Live
  export is JSON today.
- **Windows/Linux live backends** behind the same `BluetoothCaptureBackend`
  abstraction (`live::LiveShared` / the `capture` state machine are already
  platform-agnostic).
