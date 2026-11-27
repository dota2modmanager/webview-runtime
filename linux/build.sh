#!/bin/sh
# Builds the Linux engine: WebKitGTK 4.1 and everything it links, as one folder that runs on any
# x86-64 distribution with glibc at least as new as the base image's.
#
# Run as root in a clean ubuntu:24.04 container (the same base as the app's AppImage, so the
# glibc floor stays 2.39):
#
#   docker run --rm -v "$PWD:/w" -w /w ubuntu:24.04 sh linux/build.sh dist
#
# What goes in, and why it differs from the AppImage's copy:
#   * the whole closure of WebKitGTK, its helper processes, GTK's and GDK's modules, GIO's
#     modules, GStreamer's plugins and Mesa (drivers, GBM, EGL and GLX vendors), taken with ldd
#     from the files themselves. Only glibc stays the host's. The AppImage instead mixes its
#     libraries with the host's (mm-core host_env), and a host GPU driver built against newer
#     libraries than the image's is what drew black windows and crashed GTK on rolling
#     distributions (#761, #797). Here Mesa comes with the libraries it was built against.
#   * libwebkit2gtk looks for WebKitWebProcess and WebKitNetworkProcess at a path compiled in
#     (/usr/lib/x86_64-linux-gnu/webkit2gtk-4.1). Its "/usr" is rewritten to "././" (same length),
#     so the helpers are found under the working folder, which the app sets to this folder.
#     WEBKIT_EXEC_PATH would be the clean way, but release builds of WebKit ignore it.
#   * caches that hold absolute paths are written with @ROOT@ and listed in fixups.txt; the
#     installer writes the folder they were unpacked to (src/store.rs).
set -eu

OUT=${1:-dist}
LIB=lib/x86_64-linux-gnu
STAGE=$(mktemp -d)/webkit
mkdir -p "$STAGE/$LIB" "$STAGE/share" "$OUT"

export DEBIAN_FRONTEND=noninteractive
apt-get update -qq
apt-get install -y -qq --no-install-recommends \
  libwebkit2gtk-4.1-0 \
  gstreamer1.0-plugins-base gstreamer1.0-plugins-good gstreamer1.0-gl \
  libgl1-mesa-dri libegl-mesa0 libglx-mesa0 libgbm1 mesa-libgallium \
  libgdk-pixbuf2.0-bin librsvg2-common libgtk-3-bin \
  gsettings-desktop-schemas shared-mime-info hicolor-icon-theme adwaita-icon-theme \
  file binutils ca-certificates >/dev/null

VERSION=$(dpkg-query -W -f='${Version}' libwebkit2gtk-4.1-0 | sed 's/-.*//')
echo "WebKitGTK $VERSION"

# --- the files the closure starts from ---------------------------------------------------
copy() { # copy a file or folder from the system into the same place under the stage
  for src in "$@"; do
    [ -e "$src" ] || continue
    dst="$STAGE/${src#/usr/}"
    mkdir -p "$(dirname "$dst")"
    cp -a "$src" "$dst"
  done
}
copy /usr/$LIB/libwebkit2gtk-4.1.so.0* /usr/$LIB/libjavascriptcoregtk-4.1.so.0* \
  /usr/$LIB/webkit2gtk-4.1 \
  /usr/$LIB/gdk-pixbuf-2.0 /usr/$LIB/gio /usr/$LIB/gtk-3.0 \
  /usr/$LIB/gstreamer-1.0 /usr/$LIB/gstreamer1.0 \
  /usr/$LIB/dri /usr/$LIB/gbm \
  /usr/$LIB/libEGL_mesa.so.0* /usr/$LIB/libGLX_mesa.so.0* \
  /usr/share/glvnd /usr/share/glib-2.0 /usr/share/mime \
  /usr/share/icons/hicolor /usr/share/icons/Adwaita /usr/share/X11/xkb

# --- every library those files need, except glibc -----------------------------------------
GLIBC='^(ld-linux-x86-64|libc|libm|libdl|libpthread|librt|libresolv|libutil|libanl|libnsl|libmvec|libBrokenLocale)\.so'
pass=0
while :; do
  pass=$((pass + 1))
  before=$(find "$STAGE" -type f | wc -l)
  find "$STAGE" -type f | while read -r f; do
    file -b "$f" | grep -q '^ELF 64-bit' || continue
    LD_LIBRARY_PATH="$STAGE/$LIB" ldd "$f" 2>/dev/null | awk '/=> \//{print $3}'
  done | sort -u | while read -r dep; do
    base=$(basename "$dep")
    echo "$base" | grep -Eq "$GLIBC" && continue
    [ -e "$STAGE/$LIB/$base" ] && continue
    real=$(readlink -f "$dep")
    cp -a "$real" "$STAGE/$LIB/"
    [ "$(basename "$real")" = "$base" ] || ln -sf "$(basename "$real")" "$STAGE/$LIB/$base"
  done
  after=$(find "$STAGE" -type f | wc -l)
  [ "$after" -eq "$before" ] && break
  [ "$pass" -lt 20 ] || { echo "the library closure did not settle" >&2; exit 1; }
done

missing=$(find "$STAGE" -type f | while read -r f; do
  file -b "$f" | grep -q '^ELF 64-bit' || continue
  LD_LIBRARY_PATH="$STAGE/$LIB" ldd "$f" 2>/dev/null | grep 'not found' | sed "s|^|$f: |"
done)
[ -z "$missing" ] || { echo "$missing" >&2; exit 1; }

# --- helpers under the working folder --------------------------------------------------------
for f in "$STAGE"/$LIB/libwebkit2gtk-4.1.so.0.*; do
  [ -L "$f" ] && continue
  grep -c "/usr/$LIB/webkit2gtk-4.1" "$f" >/dev/null || { echo "no helper path in $f" >&2; exit 1; }
  sed -i "s|/usr/$LIB/webkit2gtk-4.1|././$LIB/webkit2gtk-4.1|g" "$f"
done

# --- caches with the bundle's own paths ---------------------------------------------------------
GDK="$STAGE/$LIB/gdk-pixbuf-2.0/2.10.0"
LD_LIBRARY_PATH="$STAGE/$LIB" GDK_PIXBUF_MODULEDIR="$GDK/loaders" \
  /usr/$LIB/gdk-pixbuf-2.0/gdk-pixbuf-query-loaders >"$GDK/loaders.cache"
sed -i "s|$STAGE|@ROOT@|g" "$GDK/loaders.cache"

IM="$STAGE/$LIB/gtk-3.0/3.0.0"
if [ -d "$IM/immodules" ]; then
  LD_LIBRARY_PATH="$STAGE/$LIB" gtk-query-immodules-3.0 "$IM"/immodules/*.so >"$IM/immodules.cache"
  sed -i "s|$STAGE|@ROOT@|g" "$IM/immodules.cache"
fi

gio-querymodules "$STAGE/$LIB/gio/modules" 2>/dev/null || true
glib-compile-schemas "$STAGE/share/glib-2.0/schemas"
update-mime-database "$STAGE/share/mime" >/dev/null 2>&1 || true

printf '%s\n' "# files that name this folder as @ROOT@ (src/store.rs writes it in)" \
  "$LIB/gdk-pixbuf-2.0/2.10.0/loaders.cache" >"$STAGE/fixups.txt"
[ -f "$IM/immodules.cache" ] && echo "$LIB/gtk-3.0/3.0.0/immodules.cache" >>"$STAGE/fixups.txt"

# --- licenses: everything shipped keeps its copyright files --------------------------------------
mkdir -p "$STAGE/share/doc"
for so in "$STAGE"/$LIB/*.so*; do
  pkg=$(dpkg -S "$(basename "$so")" 2>/dev/null | head -1 | cut -d: -f1) || continue
  [ -n "$pkg" ] && [ -f "/usr/share/doc/$pkg/copyright" ] && mkdir -p "$STAGE/share/doc/$pkg" &&
    cp "/usr/share/doc/$pkg/copyright" "$STAGE/share/doc/$pkg/"
done
dpkg-query -W -f='${Package} ${Version}\n' >"$STAGE/share/doc/PACKAGES.txt"

# --- the archive and its entry -------------------------------------------------------------------
NAME="webkitgtk-linux-x64-$VERSION"
tar -C "$(dirname "$STAGE")" --sort=name --owner=0 --group=0 --numeric-owner --mtime='@0' \
  -czf "$OUT/$NAME.tar.gz" webkit
SIZE=$(stat -c %s "$OUT/$NAME.tar.gz")
SHA=$(sha256sum "$OUT/$NAME.tar.gz" | cut -d' ' -f1)
cat >"$OUT/runtime-linux-x64.json" <<EOF
{
  "platform": "linux-x64",
  "version": "$VERSION",
  "archive": "tar.gz",
  "size": $SIZE,
  "sha256": "$SHA",
  "urls": ["${RELEASE_URL_BASE:-https://example.invalid}/$NAME.tar.gz"],
  "root": "webkit",
  "check": "$LIB/libwebkit2gtk-4.1.so.0"
}
EOF
echo "$OUT/$NAME.tar.gz $SIZE bytes"
