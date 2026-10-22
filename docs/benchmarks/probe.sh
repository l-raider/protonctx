#!/usr/bin/env bash
# usage: xvfb-run -a -s "-screen 0 1000x700x24" docs/benchmarks/probe.sh <label> <binary>
# prints: label startup_ms idle30_ms hover10_ms pss_idle_kb rss_idle_kb pss_hover_kb
label=$1; bin=$2
unset WAYLAND_DISPLAY
hz=$(getconf CLK_TCK)
"$bin" >/dev/null 2>&1 &
pid=$!
ticks() { awk '{print $14 + $15}' /proc/$pid/stat; }
sleep 4
a=$(ticks)
pss=$(awk '/^Pss:/{print $2}' /proc/$pid/smaps_rollup)
rss=$(awk '/^Rss:/{print $2}' /proc/$pid/smaps_rollup)
sleep 30
b=$(ticks)
python3 -I "$(dirname "$0")/hover.py" 0 0 10
c=$(ticks)
pssh=$(awk '/^Pss:/{print $2}' /proc/$pid/smaps_rollup)
echo "$label $((a*1000/hz)) $(((b-a)*1000/hz)) $(((c-b)*1000/hz)) $pss $rss $pssh"
kill $pid; wait $pid 2>/dev/null
