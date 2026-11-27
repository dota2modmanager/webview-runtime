#!/bin/sh
# Checks the Linux engine on a distribution other than the one it was built on: every ELF in the
# bundle finds every library it needs with only the host's glibc, and the helper processes are
# where the patched libwebkit2gtk looks for them.
#
#   docker run --rm -v "$PWD:/w" -w /w fedora:41 sh linux/smoke.sh dist/webkitgtk-linux-x64-*.tar.gz
set -eu
ARCHIVE=$1
DIR=$(mktemp -d)
tar -C "$DIR" -xzf "$ARCHIVE"
ROOT="$DIR/webkit"
LIB=lib/x86_64-linux-gnu

for helper in WebKitWebProcess WebKitNetworkProcess; do
  [ -x "$ROOT/$LIB/webkit2gtk-4.1/$helper" ] || { echo "no $helper" >&2; exit 1; }
done
grep -q "././$LIB/webkit2gtk-4.1" "$ROOT"/$LIB/libwebkit2gtk-4.1.so.0 || { echo "helper path not patched" >&2; exit 1; }

bad=0
for f in $(find "$ROOT" -type f -name '*.so*') "$ROOT/$LIB/webkit2gtk-4.1/WebKitWebProcess" "$ROOT/$LIB/webkit2gtk-4.1/WebKitNetworkProcess"; do
  head -c 4 "$f" | grep -q 'ELF' || continue
  out=$(LD_LIBRARY_PATH="$ROOT/$LIB" ldd "$f" 2>&1 || true)
  if echo "$out" | grep -q 'not found'; then
    echo "$f:" >&2
    echo "$out" | grep 'not found' >&2
    bad=1
  fi
  # a library the host supplied instead of the bundle (other than glibc) means a mix
  host=$(echo "$out" | awk '/=> \//{print $3}' | grep -v "^$ROOT/" |
    grep -Ev '/(ld-linux-x86-64|libc|libm|libdl|libpthread|librt|libresolv|libutil|libanl|libnsl|libmvec)\.so' || true)
  if [ -n "$host" ]; then
    echo "$f takes from the host: $host" >&2
    bad=1
  fi
done
[ "$bad" -eq 0 ] || exit 1
echo "ok: $(find "$ROOT" -type f | wc -l) files, $(du -sh "$ROOT" | cut -f1)"
