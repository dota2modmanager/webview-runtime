#!/bin/sh
# Checks the Linux engine on a distribution other than the one it was built on, the way the app
# would use it there:
#   1. the bundle is whole: every library each of its programs and libraries needs resolves inside
#      it (asked of the bundle's own loader, so this works on musl and NixOS too);
#   2. the helper processes are where the patched libwebkit2gtk looks for them;
#   3. the static launcher installs the archive (hash, unpack, @ROOT@) and decides, from the host,
#      whether the bundle runs on the host's glibc or its own;
#   4. with a display (Xvfb), wvr-selftest draws a page, runs its script and checks the pixels.
#
#   sh linux/smoke.sh ARCHIVE ENTRY.json LAUNCHER
# REQUIRE_SELFTEST=1 fails when no Xvfb is there to draw on.
set -eu
ARCHIVE=$1
ENTRY=$2
LAUNCHER=$3
LIB=lib/x86_64-linux-gnu

name=$( (. /etc/os-release && echo "$PRETTY_NAME") 2>/dev/null || echo unknown)
echo "== $name"

# --- 1 and 2: the bundle as it is in the archive ----------------------------------------------
DIR=$(mktemp -d)
tar -C "$DIR" -xzf "$ARCHIVE"
ROOT="$DIR/webkit"
LOADER="$ROOT/glibc/ld-linux-x86-64.so.2"
PATHS="$ROOT/glibc:$ROOT/$LIB"

for helper in WebKitWebProcess WebKitNetworkProcess; do
  [ -x "$ROOT/$LIB/webkit2gtk-4.1/$helper" ] || { echo "no $helper" >&2; exit 1; }
  [ -f "$ROOT/$LIB/webkit2gtk-4.1/$helper.real" ] || { echo "$helper is not wrapped" >&2; exit 1; }
done
grep -q "././/$LIB/webkit2gtk-4.1" "$ROOT/$LIB/libwebkit2gtk-4.1.so.0" || { echo "helper path not patched" >&2; exit 1; }

LIST=$(mktemp)
find "$ROOT" -type f \( -name '*.so*' -o -name '*.real' \) ! -path "$ROOT/glibc/*" >"$LIST"
bad=0
checked=0
while read -r f; do
  head -c 4 "$f" | grep -q 'ELF' || continue
  checked=$((checked + 1))
  out=$("$LOADER" --library-path "$PATHS" --list "$f" 2>&1 || true)
  if echo "$out" | grep -q 'not found'; then
    echo "$f:" >&2
    echo "$out" | grep 'not found' >&2
    bad=1
  fi
  outside=$(echo "$out" | awk '/=> \//{print $3}' | grep -v "^$ROOT/" || true)
  if [ -n "$outside" ]; then
    echo "$f takes from outside the bundle: $outside" >&2
    bad=1
  fi
done <"$LIST"
[ "$bad" -eq 0 ] || exit 1
echo "whole: $checked ELF files resolve inside the bundle"
# the unpacked copy is 400 MB, and a live system keeps /tmp in memory
rm -rf "$DIR" "$LIST"

# --- 3: installed by the launcher, as the app's start would ----------------------------------
BASE=$(mktemp -d)
trap 'rm -rf "$BASE"' EXIT
printf '{"format":1,"runtimes":[%s]}\n' "$(cat "$ENTRY")" >"$BASE/manifest.json"
"$LAUNCHER" install --base "$BASE/rt" --manifest "$BASE/manifest.json" --from "$ARCHIVE" >/dev/null
"$LAUNCHER" status --base "$BASE/rt" --manifest "$BASE/manifest.json"

# --- 4: a page drawn ------------------------------------------------------------------------------
# The pixels alone are not enough: the snapshot is drawn without the screen's GL, so a bundle
# whose GL cannot be found still passes it while an app's window stays black. libepoxy says so.
selftest() {
  out=$("$LAUNCHER" selftest --base "$BASE/rt" --manifest "$BASE/manifest.json" "$@" 2>&1)
  code=$?
  echo "$out" | grep -v '^Fontconfig'
  if echo "$out" | grep -q 'No provider of'; then
    echo "no GL for WebKit: libepoxy found no provider" >&2
    exit 1
  fi
  [ "$code" -eq 0 ] || exit "$code"
}
if [ -n "${DISPLAY:-}${WAYLAND_DISPLAY:-}" ] && [ "${SMOKE_XVFB:-0}" != 1 ]; then
  # a real desktop: draw on it
  export NO_AT_BRIDGE=1
  echo "drawing on the desktop's display (DISPLAY=${DISPLAY:-} WAYLAND_DISPLAY=${WAYLAND_DISPLAY:-})"
  selftest
  selftest --software
  echo "ok: $name"
  exit 0
fi
if ! command -v Xvfb >/dev/null 2>&1; then
  if [ "${REQUIRE_SELFTEST:-0}" = 1 ]; then
    echo "no Xvfb to draw on" >&2
    exit 1
  fi
  echo "no Xvfb: the page was not drawn"
  exit 0
fi
Xvfb :99 -screen 0 640x480x24 -nolisten tcp >/dev/null 2>&1 &
xvfb=$!
trap 'kill $xvfb 2>/dev/null || true; rm -rf "$BASE"' EXIT
i=0
while [ ! -e /tmp/.X11-unix/X99 ] && [ $i -lt 50 ]; do sleep 0.2; i=$((i + 1)); done
export DISPLAY=:99 NO_AT_BRIDGE=1
selftest
selftest --software
echo "ok: $name"
