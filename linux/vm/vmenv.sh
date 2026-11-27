#!/bin/sh
# Tries Mod Manager on our engine with extra variables, one set at a time, and reports which sets
# keep it alive (no SIGSEGV within SECS).
NAME=${1:-vm}
SECS=${SECS:-35}
H=http://10.0.2.2:8765
put() { curl -sf -T "$1" "$H/logs/$2" 2>/dev/null || wget -q -O /dev/null --method=PUT --body-file="$1" "$H/logs/$2"; }
D=$(dirname "$(ls -d /tmp/tmp.*/rt | head -1)")
cd "$D" || exit 1
MM="$D/mm/usr/bin/Dota 2 Mod Manager"
OUT=$(mktemp)
try() {
  label=$1
  shift
  pkill -f "$D/mm/usr/bin" 2>/dev/null
  sleep 1
  env "$@" ./webview-runtime-linux-x64 run --base "$D/rt" --manifest m.json -- "$MM" --user-data-dir="$D/jail/ud-$$" --write-jail="$D/jail" >try.out 2>&1 &
  pid=$!
  sleep "$SECS"
  if kill -0 "$pid" 2>/dev/null; then r=alive; else wait "$pid"; r="dead ($?)"; fi
  egl=$(grep -cE 'failed to create dri2|failed to authenticate' try.out)
  echo "$r | egl-warnings=$egl | $label" >>"$OUT"
  put "$OUT" "$NAME-env.log"
}
echo "### $NAME" >"$OUT"
L="$(ls -d "$D"/rt/linux-x64-*)/lib/x86_64-linux-gnu"
mkdir -p "$D/aside"
mv "$L"/libGLESv2.so* "$L"/libOpenGL.so* "$D/aside/"
try "without GLES and OpenGL"
try "without GLES, software" LIBGL_ALWAYS_SOFTWARE=1 WEBKIT_DISABLE_COMPOSITING_MODE=1
mv "$D"/aside/* "$L/"
try "with GLES again"
pkill -f "$D/mm/usr/bin" 2>/dev/null
echo "### done" >>"$OUT"
put "$OUT" "$NAME-env.log"
