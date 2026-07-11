#!/bin/bash
# snitch-blockd — Inspector Rust's best-effort per-app outbound blocker (root).
#
# NOT a real firewall: macOS per-app outbound filtering needs a NetworkExtension
# system extension (special Apple entitlement) a self-signed app can't have.
# This polls each blocked app's live connections (lsof) and pushes their remote
# IPs into a pf block table. New connections leak their first packets before the
# next poll; blocking to a shared server IP affects every app talking to it.
# All of that is surfaced to the user. Fail-OPEN: if this dies, blocks lapse.
#
# Uses the same reversible pf pattern as the maintainer's network-lock tool:
# an anchor file + a backed-up /etc/pf.conf, restored verbatim on teardown.
# Only ever manages ITS OWN table + anchor — never `pfctl -F all`.
set -u

DIR="${1:?data dir required}"
ANCHOR="io.celox.inspector-rust.snitch"
ANCHORFILE="/etc/pf.anchors/${ANCHOR}"
BLOCKLIST="${DIR}/snitch-blocklist.txt"
STOPFILE="${DIR}/snitch-blockd.stop"
PIDFILE="${DIR}/snitch-blockd.pid"
LOG="${DIR}/snitch-blockd.log"
PFBAK="${DIR}/pf.conf.snitch-bak"
WASENABLED="${DIR}/snitch-pf.was-enabled"
POLL="${SNITCH_POLL:-2}"

log() { echo "$(date '+%H:%M:%S') $*" >>"$LOG"; }

setup() {
  cat > "$ANCHORFILE" <<EOF
table <ir_snitch> persist
block drop out quick proto { tcp udp } from any to <ir_snitch>
EOF
  if ! grep -q "$ANCHOR" /etc/pf.conf 2>/dev/null; then
    cp /etc/pf.conf "$PFBAK"
    printf 'anchor "%s"\nload anchor "%s" from "%s"\n' "$ANCHOR" "$ANCHOR" "$ANCHORFILE" >> /etc/pf.conf
  fi
  if pfctl -si 2>/dev/null | grep -q 'Status: Enabled'; then
    echo yes > "$WASENABLED"   # pf was already on (e.g. Internet Sharing) — leave it on at teardown
  else
    rm -f "$WASENABLED"
  fi
  pfctl -f /etc/pf.conf 2>>"$LOG"
  pfctl -e 2>>"$LOG" || true
  log "setup done (anchor loaded, pf enabled)"
}

teardown() {
  pfctl -a "$ANCHOR" -t ir_snitch -T flush 2>>"$LOG" || true
  if [ -f "$PFBAK" ]; then
    cp "$PFBAK" /etc/pf.conf
    rm -f "$PFBAK"
  else
    sed -i '' "\|$ANCHOR|d" /etc/pf.conf 2>/dev/null || true
  fi
  rm -f "$ANCHORFILE"
  pfctl -f /etc/pf.conf 2>>"$LOG" || true
  if [ ! -f "$WASENABLED" ]; then
    pfctl -d 2>>"$LOG" || true   # we enabled pf → turn it back off
  fi
  rm -f "$WASENABLED" "$PIDFILE"
  log "teardown done (pf.conf restored)"
}

# Extract the remote IPs of every connection whose COMMAND is in the blocklist.
# lsof NAME field is `local->remote`; remote is `ip:port` or `[v6]:port`.
collect_ips() {
  [ -s "$BLOCKLIST" ] || return 0
  lsof -nP -iTCP -iUDP 2>/dev/null | awk -v listf="$BLOCKLIST" '
    BEGIN { while ((getline line < listf) > 0) if (line != "") blocked[line]=1 }
    NR==1 { next }
    !($1 in blocked) { next }
    {
      name=$9
      p=index(name, "->"); if (p==0) next
      remote=substr(name, p+2)
      if (substr(remote,1,1)=="[") {         # [v6]:port
        e=index(remote, "]"); if (e==0) next
        ip=substr(remote, 2, e-2)
      } else {                                # v4:port
        c=0; for (i=length(remote); i>0; i--) if (substr(remote,i,1)==":") { c=i; break }
        if (c==0) next
        ip=substr(remote, 1, c-1)
      }
      if (ip ~ /\*/ || ip=="") next
      print ip
    }' | sort -u
}

trap 'touch "$STOPFILE"' TERM INT
echo $$ > "$PIDFILE"
rm -f "$STOPFILE"
setup

while [ ! -f "$STOPFILE" ]; do
  ips=$(collect_ips)
  if [ -n "$ips" ]; then
    echo "$ips" | pfctl -a "$ANCHOR" -t ir_snitch -T replace -f - 2>>"$LOG" || true
  else
    pfctl -a "$ANCHOR" -t ir_snitch -T flush 2>>"$LOG" || true
  fi
  sleep "$POLL"
done

teardown
