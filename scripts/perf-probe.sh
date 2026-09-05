#!/usr/bin/env bash
# perf-probe.sh — one JSON line of the numbers PERFORMANCE-PLAN.md is measured
# against. Run before and after every performance change; paste the line into
# the commit message. Read-only: never touches the app or the DB.
#
#   bash scripts/perf-probe.sh            # 20 s idle-CPU sample
#   PROBE_CPU_SECS=60 bash scripts/perf-probe.sh
#
# Fields:
#   version            running app version (Info.plist)
#   startup_ms         log: "… starting" → last monitor "armed" line of that start
#   setup_start_ms     log: "… starting" → "db at …" (= Tauri setup begins: plugins + hidden
#                      popup webview built; the DB itself opens right after — see db_ready_ms)
#   db_ready_ms        log: "… starting" → "db ready" (crypto init + open + table init), null on old builds
#   idle_cpu_pct       mean %CPU over PROBE_CPU_SECS (top -l samples, 1 s apart)
#   rss_mb / threads   live process (RSS counts shared framework pages — see footprint_mb)
#   footprint_mb       vmmap 'Physical footprint' — the honest macOS memory number
#   db_mb / wal_mb     history.db + WAL on disk
#   freelist_pct       PRAGMA freelist_count / page_count (dead space)
#   app_chunk_kb / css_kb / entry_kb   last frontend build in core/frontend/dist
set -eu
export LC_ALL=C   # decimal points, not commas, whatever the user locale

CPU_SECS="${PROBE_CPU_SECS:-20}"
APP_DIR="/Applications/InspectorRust.app"
DATA_DIR="$HOME/Library/Application Support/InspectorRust"
REPO="$(cd "$(dirname "$0")/.." && pwd)"

version=$(defaults read "$APP_DIR/Contents/Info.plist" CFBundleShortVersionString 2>/dev/null || echo "?")
pid=$(pgrep -f "$APP_DIR/Contents/MacOS" | head -1 || true)

# ── startup timings from the newest log ─────────────────────────────────────
log=$(ls -t "$DATA_DIR"/logs/inspector-rust.log.* 2>/dev/null | head -1 || true)
startup_ms="null"; setup_start_ms="null"; db_ready_ms="null"
if [ -n "$log" ]; then
  # Take the LAST "starting" block in the file.
  block=$(awk '/Inspector Rust v[0-9.]+ starting/{buf=""} {buf=buf $0 "\n"} END{printf "%s", buf}' "$log")
  t0=$(printf "%s" "$block" | grep -m1 " starting (logs" | cut -c1-27)
  tdb=$(printf "%s" "$block" | grep -m1 " db at " | cut -c1-27)
  trdy=$(printf "%s" "$block" | grep -m1 " db ready" | cut -c1-27)
  # Last "armed" line within the first 10 s of the start (monitors/hotkeys).
  tarm=$(printf "%s" "$block" | grep " armed" | head -20 | tail -1 | cut -c1-27)
  ms() { python3 -c "
import sys,datetime
def p(s):
    s=s.strip().replace('Z','')
    return datetime.datetime.fromisoformat(s)
a,b=sys.argv[1],sys.argv[2]
print(int((p(b)-p(a)).total_seconds()*1000))" "$1" "$2" 2>/dev/null || echo null; }
  [ -n "$t0" ] && [ -n "$tdb" ] && setup_start_ms=$(ms "$t0" "$tdb")
  [ -n "$t0" ] && [ -n "$trdy" ] && db_ready_ms=$(ms "$t0" "$trdy")
  [ -n "$t0" ] && [ -n "$tarm" ] && startup_ms=$(ms "$t0" "$tarm")
fi

# ── live process ────────────────────────────────────────────────────────────
rss_mb="null"; threads="null"; idle_cpu_pct="null"; footprint_mb="null"
if [ -n "$pid" ]; then
  rss_mb=$(ps -p "$pid" -o rss= | awk '{printf "%.1f", $1/1024}')
  threads=$(ps -M -p "$pid" | tail -n +2 | wc -l | tr -d ' ')
  footprint_mb=$(vmmap -summary "$pid" 2>/dev/null | awk '/^Physical footprint:/ {gsub("M","",$3); print $3; exit}' || echo null)
  [ -z "$footprint_mb" ] && footprint_mb=null
  # top prints one sample per second; skip the first (it is cumulative).
  # `-stats pid,cpu` → "PID  %CPU" rows; guard on the pid column so a header
  # or a stray line can never be summed as CPU (the first draft summed PIDs).
  idle_cpu_pct=$(top -l $((CPU_SECS + 1)) -s 1 -pid "$pid" -stats pid,cpu 2>/dev/null \
    | awk -v p="$pid" '$1==p {rows[n++]=$2} END{ for(i=1;i<n;i++){s+=rows[i]; m++}; if(m) printf "%.2f", s/m; else print "null"}')
fi

# ── database ────────────────────────────────────────────────────────────────
db="$DATA_DIR/history.db"
db_mb="null"; wal_mb="null"; freelist_pct="null"
if [ -f "$db" ]; then
  db_mb=$(stat -f %z "$db" | awk '{printf "%.1f", $1/1048576}')
  wal_mb=$( [ -f "$db-wal" ] && stat -f %z "$db-wal" | awk '{printf "%.1f", $1/1048576}' || echo 0 )
  freelist_pct=$(sqlite3 -readonly "$db" "SELECT printf('%.1f', 100.0*(SELECT freelist_count FROM pragma_freelist_count)/(SELECT page_count FROM pragma_page_count));" 2>/dev/null || echo null)
fi

# ── frontend build ──────────────────────────────────────────────────────────
dist="$REPO/core/frontend/dist/assets"
kb() { [ -n "$1" ] && stat -f %z "$1" | awk '{printf "%.0f", $1/1024}' || echo null; }
app_chunk_kb=$(kb "$(ls "$dist"/App-*.js 2>/dev/null | head -1)")
css_kb=$(kb "$(ls "$dist"/index-*.css 2>/dev/null | head -1)")
entry_kb=$(kb "$(ls -S "$dist"/index-*.js 2>/dev/null | head -1)")

printf '{"version":"%s","startup_ms":%s,"setup_start_ms":%s,"db_ready_ms":%s,"idle_cpu_pct":%s,"rss_mb":%s,"footprint_mb":%s,"threads":%s,"db_mb":%s,"wal_mb":%s,"freelist_pct":%s,"app_chunk_kb":%s,"css_kb":%s,"entry_kb":%s,"cpu_secs":%s}\n' \
  "$version" "$startup_ms" "$setup_start_ms" "$db_ready_ms" "$idle_cpu_pct" "$rss_mb" "$footprint_mb" "$threads" "$db_mb" "$wal_mb" "$freelist_pct" "$app_chunk_kb" "$css_kb" "$entry_kb" "$CPU_SECS"
