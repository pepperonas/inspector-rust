# `speedtest` — internet speed in the preview

Type `speedtest` and press Enter. The preview measures latency, jitter,
download and upload against Cloudflare's public speed-test servers
(`speed.cloudflare.com`) and shows the result with a plain-language verdict,
the server location and a comparison with your earlier runs.

## How it measures

| Phase | Method |
|---|---|
| Server | `/cdn-cgi/trace` gives the Cloudflare data centre (IATA code, e.g. `TXL`) and your country. |
| Latency | One warm-up request, then 20 empty requests (`__down?bytes=0`) on the same connection. Latency = median, jitter = mean difference between consecutive samples. |
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

## Data

* Cloudflare receives your public IP. Nothing else leaves the machine.
* A full run transfers up to ~200 MB on a fast line.
* Every result is stored locally in `speedtest_history` (last 200 rows,
  not encrypted — it holds rates and a data-centre code only). The panel shows
  the last 8 with download/upload bars; the bin icon clears the history.
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
