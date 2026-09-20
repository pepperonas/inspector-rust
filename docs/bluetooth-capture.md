# Bluetooth capture analyzer — `btsniff`

`btsniff` (aliases `btcap`, `btcapture`, `blecap`) imports a Bluetooth HCI
capture file and decodes it down to the GATT layer, with a byte-diff view built
for reverse-engineering an app↔device protocol. It is a **separate command** from
`bt` / `bluetooth` (the device manager) and never touches it.

## What it is — and honestly is not

It analyses **exported capture files**. It does **not** sniff live traffic.

macOS deliberately exposes **no public API for promiscuous HCI/ACL capture** —
that path is gated behind Apple-private entitlements used only by Apple's own
PacketLogger. Rather than fake a live capture or lean on undocumented private
interfaces, `btsniff` does the part that is fully reliable and fully
cross-platform: parse and decode a capture you already have.

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

## Roadmap

A CoreBluetooth **live-GATT** backend (scan / connect / subscribe to the
connections Inspector Rust itself makes) is the planned second stage; it will be
macOS-only and gate itself with a clean message elsewhere. The file analyzer
described here works on every platform.
