#!/bin/sh
# Starts Mod Manager on our engine again, in the mode given ($2: normal or software), using the
# engine and app vmtest.sh left in /tmp, and keeps it up for MM_SECONDS.
NAME=${1:-vm}
MODE=${2:-software}
MM_SECONDS=${MM_SECONDS:-60}
H=http://10.0.2.2:8765
put() { curl -sf -T "$1" "$H/logs/$2" 2>/dev/null || wget -q -O /dev/null --method=PUT --body-file="$1" "$H/logs/$2"; }
D=$(dirname "$(ls -d /tmp/tmp.*/rt | head -1)")
cd "$D" || exit 1
pkill -f "$D/mm/usr/bin" 2>/dev/null
flag=""
[ "$MODE" = software ] && flag=--software
./webview-runtime-linux-x64 run --base "$D/rt" --manifest m.json $flag -- "$D/mm/usr/bin/Dota 2 Mod Manager" --user-data-dir="$D/jail/ud" --write-jail="$D/jail" >mm-$MODE.out 2>&1 &
sleep "$MM_SECONDS"
{
  echo "### $NAME, Mod Manager, $MODE"
  pgrep -f "$D/mm/usr/bin" >/dev/null && echo "running after ${MM_SECONDS}s" || echo "not running"
  grep -vE "Fontconfig|Gtk-Message" mm-$MODE.out | tail -15
} >mm-$MODE.log 2>&1
put mm-$MODE.log "$NAME-mm-$MODE.log"
