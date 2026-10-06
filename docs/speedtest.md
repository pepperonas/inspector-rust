# `speedtest` — internet speed in the preview

Type `speedtest` and press Enter. The preview measures latency, jitter,
download and upload against Cloudflare's public speed-test servers
(`speed.cloudflare.com`) and shows the result with a plain-language verdict,
the server location and a comparison with your earlier runs.

## How it measures

| Phase | Method |
|---|---|
| Server | `/cdn-cgi/trace` gives the Cloudflare data centre (IATA code, e.g. `TXL`) and your country. |
| Latency | One warm-up request, then 20 empty requests (`__down?bytes=0`) on the same connection. Each sample = request time **minus the server time** Cloudflare reports in `Server-Timing` (the sum of every `dur=`, e.g. `cfSpeedEdge;dur=2, cfSpeedWorker;dur=15`). Latency = median, jitter = mean difference between consecutive samples. |
| Ping under load | While download and upload run, a second connection pings every 150 ms; the median of those pings (≥ 3 needed) is the latency under load. |
| Download | Transfers of 100 kB ×4, 1 MB ×6, 10 MB ×4, 25 MB ×3 (`__down?bytes=N`). Only the body transfer is timed; the headers are latency. |
| Upload | 100 kB ×4, 1 MB ×6, 10 MB ×3 (`POST __up`). Round trip minus one latency. |

Each transfer is one bandwidth sample; the result is the **90th percentile**
of the samples that took at least 10 ms (shorter ones measure request overhead,
not the line). That is the same summary Cloudflare's own test uses, so the
numbers are comparable.

**Slow lines stop early.** After each stage the next one is predicted from the
median duration of this one; if a transfer of the next size would take longer
than 2.5 s, the plan ends there. On 8 Mbit/s the 10 MB and 25 MB stages never
start, so the whole run stays around 10–20 s.

A phase that fails stays empty (`—`), never 0; the run only fails if no phase
produced a value.

### Why the server time is subtracted (v0.199.1)

The first version reported the raw request time: 39 ms, where speedtest.net
said 14. In those 39 ms sat 17–25 ms of Cloudflare's own processing (the
worker that answers the request). Subtracted, 14 ms remained — matching
speedtest.net and Cloudflare's own TCP measurement (`cfL4 rtt`, 12–16 ms).
Without a `Server-Timing` header the raw time is used, never a guess.

### Ping under load vs. speedtest.net

speedtest.net loads the line with many parallel connections, which fills the
router's buffer — hence its 185 ms during download. One connection fills it
less; expect lower values here. They are still useful for comparing your own
runs over time.

## Data

* Cloudflare receives your public IP. Nothing else leaves the machine.
* A full run transfers up to ~200 MB on a fast line.
* Every result is stored locally in `speedtest_history` (last 200 rows,
  not encrypted — it holds rates and a data-centre code only). The panel shows
  median / min / max per metric, one small chart per metric with its own scale,
  and the last 30 runs with every value; the bin icon clears the history.
* The comparison (`+12 % ggü. früher`) is against the **median** of the earlier
  runs, so one odd run doesn't skew it.

## Behaviour

* Enter starts the run — typing the word does nothing, a run costs bandwidth.
* The run lives in the backend: Esc closes the panel, the run keeps going and
  is stored; reopening the panel shows "läuft" and then the result.
* R measures again, Esc closes.

## Limits

* Sequential, single-connection transfers. On gigabit lines the result can
  stay below what tests with several parallel connections report.
* Upload is timed as round trip minus latency, which can read a bit high on
  very fast uplinks with small transfers.

## Code

* `core/rust-lib/src/speedtest.rs` — plan, maths (pure, tested), network, history.
* `commands.rs` — `speedtest_run` (async; emits `speedtest-progress` /
  `speedtest-done`), `speedtest_running`, `speedtest_history`,
  `speedtest_clear_history`.
* `core/frontend/src/components/SpeedtestPanel.tsx`, `lib/speedtest.ts`
  (formatting, verdict, comparison — tested).
* Live check: `cargo test -p inspector-rust-core --lib speedtest_live -- --ignored --nocapture`.
