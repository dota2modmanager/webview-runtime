#!/bin/sh
# Runs Mod Manager on our engine under gdb, with the launcher's own environment, and sends back the
# backtraces of every thread at the crash.
NAME=${1:-vm}
H=http://10.0.2.2:8765
put() { curl -sf -T "$1" "$H/logs/$2" 2>/dev/null || wget -q -O /dev/null --method=PUT --body-file="$1" "$H/logs/$2"; }
command -v gdb >/dev/null || sudo apt-get install -y -qq gdb >/dev/null 2>&1 || sudo pacman -S --noconfirm gdb >/dev/null 2>&1
D=$(dirname "$(ls -d /tmp/tmp.*/rt | head -1)")
cd "$D" || exit 1
pkill -f "$D/mm/usr/bin" 2>/dev/null
ROOT=$(ls -d "$D"/rt/linux-x64-*)
./webview-runtime-linux-x64 run --base "$D/rt" --manifest m.json -- env | grep -E '^(LD_LIBRARY_PATH|GDK_|GIO_|GTK_|GSETTINGS_|GST_|LIBGL_|GBM_|__EGL_|FONTCONFIG_|XDG_DATA_DIRS|WEBKIT_|D2MM_)' >gdb-env.txt
OUT=$(mktemp)
{
  echo "### $NAME gdb"
  cd "$ROOT" || exit 1
  sed 's/^/set environment /' "$D/gdb-env.txt" >"$D/gdb-cmds.txt"
  timeout 180 gdb -q -batch -x "$D/gdb-cmds.txt" -ex "set pagination off" -ex "handle SIGPIPE nostop" -ex run -ex "bt 40" -ex "info registers rip" \
    --args "$D/mm/usr/bin/Dota 2 Mod Manager" --user-data-dir="$D/jail/ud3" --write-jail="$D/jail" 2>&1 | grep -vE 'Fontconfig|^\[New Thread|^\[Thread .* exited' | tail -70
} >"$OUT" 2>&1
put "$OUT" "$NAME-gdb.log"
echo "sent $NAME-gdb.log"
