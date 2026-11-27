#!/bin/sh
# Checks the Linux engine on a distribution other than the one it was built on:
#   * the host's glibc is at least the bundle's floor (share/doc/GLIBC.txt);
#   * every ELF in the bundle finds every library it needs, and none but glibc's from the host;
#   * the helper processes are where the patched libwebkit2gtk looks for them, and they load:
#     started with no arguments they must get past the dynamic loader.
#
#   docker run --rm -v "$PWD:/w" -w /w archlinux sh linux/smoke.sh dist/webkitgtk-linux-x64-*.tar.gz
set -eu
ARCHIVE=$1
DIR=$(mktemp -d)
tar -C "$DIR" -xzf "$ARCHIVE"
ROOT="$DIR/webkit"
LIB=lib/x86_64-linux-gnu
HELPERS="$ROOT/$LIB/webkit2gtk-4.1"

name=$( (. /etc/os-release && echo "$PRETTY_NAME") 2>/dev/null || echo unknown)
host_glibc=$(ldd --version 2>&1 | head -1 | grep -o '[0-9][0-9]*\.[0-9][0-9]*$')
floor=$(cat "$ROOT/share/doc/GLIBC.txt")
echo "$name: glibc $host_glibc, bundle needs $floor"
if [ "$(printf '%s\n%s\n' "$floor" "$host_glibc" | sort -V | head -1)" != "$floor" ]; then
  echo "glibc $host_glibc is older than $floor" >&2
  exit 1
fi

for helper in WebKitWebProcess WebKitNetworkProcess; do
  [ -x "$HELPERS/$helper" ] || { echo "no $helper" >&2; exit 1; }
done
grep -q "././$LIB/webkit2gtk-4.1" "$ROOT/$LIB/libwebkit2gtk-4.1.so.0" || { echo "helper path not patched" >&2; exit 1; }

bad=0
for f in $(find "$ROOT" -type f -name '*.so*') "$HELPERS/WebKitWebProcess" "$HELPERS/WebKitNetworkProcess"; do
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

for helper in WebKitWebProcess WebKitNetworkProcess; do
  err=$(cd "$ROOT" && LD_LIBRARY_PATH="$ROOT/$LIB" timeout 10 "$HELPERS/$helper" 2>&1 || true)
  if echo "$err" | grep -Eq 'error while loading shared libraries|version .GLIBC_|symbol lookup error'; then
    echo "$helper does not load: $err" >&2
    bad=1
  fi
done

[ "$bad" -eq 0 ] || exit 1
echo "ok: $(find "$ROOT" -type f | wc -l) files, $(du -sh "$ROOT" | cut -f1)"
