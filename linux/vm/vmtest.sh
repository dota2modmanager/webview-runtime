#!/bin/sh
# Runs in a test VM: fetches the bundle and launcher from the host (10.0.2.2:8765), runs smoke.sh
# on the VM's own desktop, then starts Dota 2 Mod Manager (4.7.1, from its .deb) on our engine and
# leaves its window up for MM_SECONDS. When the self-test fails it narrows down why. The log goes
# back to the host as logs/<NAME>.log.
#   curl -s http://10.0.2.2:8765/vmtest.sh | sh -s NAME
NAME=${1:-vm}
MM_SECONDS=${MM_SECONDS:-40}
H=http://10.0.2.2:8765
get() { curl -sfO "$H/$1" 2>/dev/null || wget -q "$H/$1"; }
put() { curl -sf -T "$1" "$H/logs/$2" 2>/dev/null || wget -q -O /dev/null --method=PUT --body-file="$1" "$H/logs/$2"; }
say() { echo "$*" >>"$D/log.txt"; put "$D/log.txt" "$NAME.log"; }

D=$(mktemp -d)
cd "$D" || exit 1
for f in webkitgtk-linux-x64.tar.gz runtime-linux-x64.json webview-runtime-linux-x64 smoke.sh mm/mm-linux.tar.gz; do get "$f"; done
chmod +x webview-runtime-linux-x64
L=./webview-runtime-linux-x64

{
  echo "### $NAME: $(uname -r)"
  head -3 /etc/os-release
  echo "session: ${XDG_SESSION_TYPE:-?} DISPLAY=${DISPLAY:-} WAYLAND_DISPLAY=${WAYLAND_DISPLAY:-}"
  echo "### smoke"
  sh smoke.sh webkitgtk-linux-x64.tar.gz runtime-linux-x64.json "$L"
  echo "### smoke exit $?"
} >log.txt 2>&1
put log.txt "$NAME.log"

printf '{"format":1,"runtimes":[%s]}\n' "$(cat runtime-linux-x64.json)" >m.json
"$L" install --base "$D/rt" --manifest m.json --from webkitgtk-linux-x64.tar.gz >/dev/null

# --- Dota 2 Mod Manager on our engine ---------------------------------------------------------
mkdir -p mm jail
tar -xzf mm-linux.tar.gz -C mm
MM="$D/mm/usr/bin/Dota 2 Mod Manager"
chmod +x "$MM"
say "### Mod Manager 4.7.1 on our engine, window up for ${MM_SECONDS}s"
"$L" run --base "$D/rt" --manifest m.json -- "$MM" --user-data-dir="$D/jail/ud" --write-jail="$D/jail" >mm.out 2>&1 &
pid=$!
sleep "$MM_SECONDS"
if kill -0 "$pid" 2>/dev/null; then
  say "Mod Manager is running after ${MM_SECONDS}s (pid $pid)"
  pkill -f "$D/mm/usr/bin" 2>/dev/null
  kill "$pid" 2>/dev/null
else
  wait "$pid"
  say "Mod Manager exited with $? before ${MM_SECONDS}s"
fi
sleep 1
say "--- its output (last 30 lines):"
tail -30 mm.out >>log.txt
say "### done"
echo "log sent as $NAME.log"
