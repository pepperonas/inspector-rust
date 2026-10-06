# pulse — memory pressure, swap and SSD writes

`pulse` answers one question: **how much does this Mac write to its SSD, when, and why?** On a machine whose memory pressure is mostly red with ~20 GB swapped out, most of those writes are swap — and swap wears the SSD down. `pulse` makes that visible.

Type `pulse` in the search bar. The preview shows the live state right away; Enter hands it the keyboard (← / → switches between **Live** and **Verlauf**, 1–5 picks 1 h · 24 h · 7 d · 30 d · 90 d, Esc goes back). macOS only.

## What is measured

A background sampler starts with the app and runs independently of any window. It reads everything directly from the kernel — no shell commands in the loop.

| Every | Source | Value |
|---|---|---|
| 5 s | `kern.memorystatus_vm_pressure_level` | pressure: 1 normal (green), 2 warn (yellow), 4 critical (red) |
| 5 s | `vm.swapusage` | swap used / total |
| 5 s | `host_statistics64(HOST_VM_INFO64)` | swap-outs, swap-ins, page-outs, compressions, decompressions (as rates); wired, active, inactive, free and compressor pages. Page size from `vm_page_size`, never assumed. |
| 5 s | IOKit `IOBlockStorageDriver` → `Statistics` | bytes written / read of the **internal** SSD |
| 5 s | `HOST_CPU_LOAD_INFO`, `NSProcessInfo.thermalState` | CPU load, thermal state |
| 30 s | `proc_pid_rusage(RUSAGE_INFO_V4)` | per process: physical footprint and bytes written |
| daily | `smartctl -a -j disk0` (if installed) | wear in percent, lifetime bytes written |

**The internal SSD** is the block-storage driver whose `Physical Interconnect Location` is `Internal` and whose bus is not removable — the built-in SD card reader also reports itself as internal (with zero counters) and is skipped, as are USB and Thunderbolt disks.

**Swap share of the writes** = swap-outs × page size, capped at what the disk actually took in the interval. It is an estimate: the kernel can batch or compress before writing.

### Counters and resets

All kernel and disk counters are cumulative since boot. A rate is the difference of two readings divided by the **real elapsed time** (monotonic clock), so a late tick never inflates it. A counter that went *down* has restarted (reboot, driver reload) — that interval counts as 0, never as a wrapped giant number. A disk whose identity changed between readings also counts 0. A gap of more than 30 s between ticks (thread stalled, sleep/wake) re-baselines instead of averaging over an unknown period.

### Grouping the causers

Processes are grouped so the list reads like Activity Monitor, with rules for the usual memory hogs first:

1. `java` with `GradleDaemon` / `org.gradle` in its arguments → **Gradle-Daemon**; with `KotlinCompileDaemon` → **Kotlin-Daemon**; any other `java` → **Java**. Arguments are read only for `java` processes (`KERN_PROCARGS2`) and cached per process.
2. Anything inside `Docker.app` or named `com.docker.*`, `vpnkit`, `qemu-*` → **Docker**.
3. Claude Code (its binary is named after its version) → **Claude Code**.
4. The *responsible* app (`responsibility_get_pid_responsible_for_pid`, a private libsystem function Activity Monitor uses, looked up at runtime) — so all Chrome helpers become **Google Chrome**. Terminals (iTerm, Terminal, Warp, kitty, Ghostty, …) are skipped here: they are "responsible" for everything started in them, and grouping cargo, node and rustc under "iTerm" would hide exactly the process you're looking for.
5. The outermost `.app` bundle the binary sits in, else the process name.

## What it can't see

- **Swap can't be attributed to a process.** The kernel writes it, and `ri_diskio_byteswritten` excludes it. The causers list therefore ranks by memory footprint (what *causes* swapping) and shows each group's own writes next to it.
- **Without admin rights**, processes of other users — root daemons, `kernel_task` — can't be read. The panel says how many were skipped.
- **The responsible-app lookup is a private API.** If a future macOS drops it, grouping falls back to the bundle and name rules.
- **Disk counters reset at boot**, so the first interval after a restart contributes nothing.

## Storage and retention

Raw 5-s samples never reach the disk: they live in a RAM ring buffer for the last hour (720 samples). Once per wall-clock minute the sampler writes **one** aggregate row and re-derives the 15-minute and day rows that contain it — three small upserts in one transaction, in a separate `pulse.db` (WAL) next to `history.db` in the app's data directory.

| Level | Bucket | Kept |
|---|---|---|
| raw | 5 s | 1 hour, RAM only |
| 1 | 1 minute | 7 days |
| 15 | 15 minutes | 90 days |
| 1440 | local calendar day | forever |

Each aggregate stores: measured seconds; seconds spent at normal / warn / critical pressure; swap min / time-weighted average / max / total; bytes written and read; estimated swap bytes written; swap-in bytes; compressions; average CPU; and the top 8 causers of the period (peak footprint, bytes written). Roll-ups add sums, keep extremes and merge causer lists. Day rows follow the local calendar (DST-safe), so a day card means your day, not a UTC one.

The history view serves 1 h from the RAM ring (per minute, including the minute in progress), 24 h from 15-minute rows, 7 d as hourly bars merged from 15-minute rows, and 30 d / 90 d from day rows.

## Forecast

`years = (TBW − lifetime writes) / average bytes per day / 365`

- The average comes from the last 30 days of day rows (less if less is recorded). Less than one day of data → no forecast at all.
- **TBW**: Settings → Pulse; empty means 600 TB per TB of the drive's capacity, a common consumer-NVMe rating. Apple doesn't publish a TBW for its SSDs.
- **Lifetime writes** come from smartctl when it's installed (`brew install smartmontools`); without it the forecast counts from zero and says so.

It is always labelled an estimate: drives often outlast their rating, and the rating itself is a guess for Apple drives.

## Notification

Optional (Settings → Pulse, off by default): when pressure has been red for X minutes (default 10), a macOS notification names the biggest consumer — at most once an hour.

## Settings

| Key | Default | |
|---|---|---|
| `pulse.enabled` | `true` | sampler on/off; applies live, nothing is read or written while off |
| `pulse.notify` | `false` | red-pressure notification |
| `pulse.notify_minutes` | `10` | 1–240 |
| `pulse.tbw_tb` | `0` | 0 = automatic |

## Overhead

Measured in the running app (M1 Pro, release build, 10 minutes, during heavy compiling):

| | |
|---|---|
| sampler thread CPU | **0.094 %** (0.56 s CPU in 601 s) — budget was 0.5 % |
| system reading (every 5 s) | avg 1.16 ms, max 2.85 ms |
| process scan (every 30 s, ~420 processes) | avg 21.7 ms, max 28 ms |
| memory | ring buffer 720 samples ≈ 120 KB |
| own disk writes | ~12 KB per minute into the WAL (≈ 0.75 MB/h, ~35 MB/day including checkpoints) |
| `pulse.db` size | ~1 KB per stored row; steady state ≈ 6 MB (7 days of minutes + 90 days of quarter hours) plus ~1 KB per day |

The sampler measures itself: the panel's footnote shows CPU share and milliseconds per reading and scan, and every 10 minutes it logs a line `pulse: overhead …` to the app log. The SSD counter was cross-checked against `iostat` for the same minute (1 196 MB read+written by pulse vs ~1 136 MB by iostat, windows offset by a few seconds).

## Code

`core/rust-lib/src/pulse/`: `macos.rs` (readers, all `unsafe` with `SAFETY` notes), `mod.rs` (pure model: deltas, disk choice, grouping, aggregation, forecast), `store.rs` (`pulse.db`), `sampler.rs` (thread, ring, IPC payloads). Frontend: `components/PulsePanel.tsx`, `lib/pulse.ts`. Live check of the readers on a real machine:

```
cargo test -p inspector-rust-core --lib pulse_live -- --ignored --nocapture
```
