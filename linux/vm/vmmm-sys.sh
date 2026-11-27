#!/bin/sh
# Control: the same Mod Manager on the host's own WebKitGTK, without our engine.
NAME=${1:-vm}
MM_SECONDS=${MM_SECONDS:-60}
H=http://10.0.2.2:8765
put() { curl -sf -T "$1" "$H/logs/$2" 2>/dev/null || wget -q -O /dev/null --method=PUT --body-file="$1" "$H/logs/$2"; }
D=$(dirname "$(ls -d /tmp/tmp.*/rt | head -1)")
cd "$D" || exit 1
pkill -f "$D/mm/usr/bin" 2>/dev/null
sleep 2
"$D/mm/usr/bin/Dota 2 Mod Manager" --user-data-dir="$D/jail/ud2" --write-jail="$D/jail" >mm-sys.out 2>&1 &
sleep "$MM_SECONDS"
{
  echo "### $NAME, Mod Manager on the host's WebKitGTK"
  pgrep -f "$D/mm/usr/bin" >/dev/null && echo "running after ${MM_SECONDS}s" || echo "not running"
  grep -vE "Fontconfig|Gtk-Message" mm-sys.out | tail -15
} >mm-sys.log 2>&1
put mm-sys.log "$NAME-mm-sys.log"
