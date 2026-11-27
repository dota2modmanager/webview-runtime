#!/bin/sh
# Builds the Linux engine: WebKitGTK 4.1 and everything it links, as one folder that runs on any
# x86-64 distribution with glibc at least as new as the base image's.
#
# Run as root in a clean ubuntu:24.04 container: the stack Linux Mint 22 and Ubuntu 24.04 run on
# themselves (WebKitGTK 2.50, Mesa 25). The 22.04 base this started on had Mesa 23.2, and Dota 2
# Mod Manager crashed in WebKit's GL setup on Mint 22.3 in a VM, where the host's own 24.04 stack
# drew (2026-11-27). The base decides the oldest glibc the bundle runs on with the host's loader,
# 2.39: Ubuntu 24.04+, Mint 22+, Debian 13, Kali, Arch, Manjaro, EndeavourOS, Fedora 40+,
# openSUSE Tumbleweed. Older hosts (Ubuntu 22.04, Debian 12, Mint 21) run it on its own glibc.
#
# Hosts whose glibc cannot run it (musl: Alpine, Void musl; NixOS; glibc older than the floor) run
# it on the glibc it carries in glibc/: the program starts through glibc/ld-linux-x86-64.so.2, and
# every executable in the bundle is a small sh wrapper that does the same for itself when
# D2MM_GLIBC_LOADER is set (src/store.rs linux_launch). WebKit starts its helper processes by path,
# so the wrapper is what lets them load on a host without /lib64/ld-linux-x86-64.so.2.
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
#     (/usr/lib/x86_64-linux-gnu/webkit2gtk-4.1). Its "/usr/" is rewritten to "././/" (same length),
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
  libgl1-mesa-dri libegl-mesa0 libglx-mesa0 libgbm1 \
  libgdk-pixbuf2.0-bin librsvg2-common libgtk-3-bin \
  gsettings-desktop-schemas shared-mime-info hicolor-icon-theme adwaita-icon-theme \
  fonts-dejavu-core fontconfig-config \
  libayatana-appindicator3-1 libgles2 libopengl0 \
  file binutils ca-certificates \
  gcc libc6-dev pkg-config libwebkit2gtk-4.1-dev >/dev/null
# Mesa's Gallium drivers are their own package since 24.04 (in -dri before)
apt-get install -y -qq --no-install-recommends mesa-libgallium >/dev/null 2>&1 || true

VERSION=$(dpkg-query -W -f='${Version}' libwebkit2gtk-4.1-0 | sed 's/-.*//')
echo "WebKitGTK $VERSION"

# the self-test (linux/selftest.c): draws a page with this very WebKitGTK and checks its pixels
mkdir -p "$STAGE/bin"
# shellcheck disable=SC2046 # pkg-config's flags are separate words on purpose
gcc -O2 -Wall -o "$STAGE/bin/wvr-selftest" "$(dirname "$0")/selftest.c" $(pkg-config --cflags --libs webkit2gtk-4.1)

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
  /usr/share/icons/hicolor /usr/share/icons/Adwaita /usr/share/X11/xkb \
  /usr/share/fonts/truetype/dejavu
# loaded with dlopen, so no ldd finds them: the tray icon of Tauri apps (libappindicator-sys).
# The host's copy is built against the host's GLib, which may be newer than the bundle's (Arch:
# libdbusmenu-glib wants g_once_init_leave_pointer, GLib 2.80), and Mod Manager 4.7.1 then panics
# at start, so the bundle carries its own.
copy /usr/$LIB/libayatana-appindicator3.so.1* /usr/$LIB/libayatana-indicator3.so.7* \
  /usr/$LIB/libayatana-ido3-0.4.so.0* /usr/$LIB/libdbusmenu-glib.so.4* /usr/$LIB/libdbusmenu-gtk3.so.4*
[ -e "$STAGE/$LIB/libayatana-appindicator3.so.1" ] || { echo "no libayatana-appindicator3" >&2; exit 1; }
# also dlopened: libepoxy loads GLES and OpenGL by name when WebKit makes its first GL context.
# Without them in the bundle it found none on X11 ("No provider of glTexParameteri") and Mod
# Manager's window stayed black in Mint and EndeavourOS, while the self-test's snapshot, drawn
# without the screen's GL, came out right.
copy /usr/$LIB/libGLESv2.so.2* /usr/$LIB/libOpenGL.so.0*
[ -e "$STAGE/$LIB/libGLESv2.so.2" ] || { echo "no libGLESv2" >&2; exit 1; }

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
for f in "$STAGE/$LIB"/libwebkit2gtk-4.1.so.0.*; do
  [ -L "$f" ] && continue
  grep -c "/usr/$LIB/webkit2gtk-4.1" "$f" >/dev/null || { echo "no helper path in $f" >&2; exit 1; }
  # "/usr/" and "././/" are both five bytes: the file must not change size, or every offset
  # after the path moves and the loader crashes on the library (it did, with "././")
  before=$(stat -c %s "$f")
  sed -i "s|/usr/$LIB/webkit2gtk-4.1|././/$LIB/webkit2gtk-4.1|g" "$f"
  [ "$(stat -c %s "$f")" -eq "$before" ] || { echo "patching changed the size of $f" >&2; exit 1; }
done

# Debian and Ubuntu keep GTK's and GLib's helper tools out of PATH, in the libraries' folders
tool() { # tool NAME: the first copy found on PATH or in those folders
  # libgtk-3-0t64: 24.04 renamed the package (64-bit time_t) and the folder with it
  for t in "$(command -v "$1" 2>/dev/null)" "/usr/$LIB/libgtk-3-0t64/$1" "/usr/$LIB/libgtk-3-0/$1"     "/usr/$LIB/glib-2.0/$1" "/usr/lib/glib-2.0/$1"; do
    if [ -n "$t" ] && [ -x "$t" ]; then echo "$t"; return 0; fi
  done
  echo "no $1" >&2
  return 1
}

# --- glibc of its own, for hosts that cannot run the bundle on theirs -----------------------------
mkdir -p "$STAGE/glibc/locale"
for so in ld-linux-x86-64.so.2 libc.so.6 libm.so.6 libmvec.so.1 libpthread.so.0 libdl.so.2 librt.so.1 \
  libresolv.so.2 libutil.so.1 libanl.so.1 libnsl.so.1 libBrokenLocale.so.1 libnss_files.so.2 libnss_dns.so.2; do
  if [ -e "/usr/$LIB/$so" ]; then cp -L "/usr/$LIB/$so" "$STAGE/glibc/$so"; fi
done
[ -x "$STAGE/glibc/ld-linux-x86-64.so.2" ] || { echo "no loader" >&2; exit 1; }
cp -a "/usr/$LIB/gconv" "$STAGE/glibc/gconv"
cp -a /usr/lib/locale/C.utf8 "$STAGE/glibc/locale/C.utf8"

# every executable becomes a wrapper that starts it through the bundle's loader when asked
# Libraries are never wrapped, even runnable ones: 24.04's libcap.so.2 has an interpreter too,
# and wrapping it left a shell script where the loader wanted an ELF ("invalid ELF header").
find "$STAGE" -type f ! -path "$STAGE/glibc/*" ! -name '*.so' ! -name '*.so.*' | while read -r f; do
  file -b "$f" | grep -q 'interpreter /lib64/ld-linux-x86-64.so.2' || continue
  mv "$f" "$f.real"
  cat >"$f" <<'WRAP'
#!/bin/sh
# Starts the program beside this file (".real"), through the bundle's own glibc where the app
# runs the bundle on it (webview-runtime: D2MM_GLIBC_LOADER), else on the host's.
if [ -n "${D2MM_GLIBC_LOADER:-}" ]; then
  exec "$D2MM_GLIBC_LOADER" --library-path "$D2MM_GLIBC_LIBRARY_PATH" --argv0 "$0" "$0.real" "$@"
fi
exec "$0.real" "$@"
WRAP
  chmod 755 "$f"
done

# fonts: the host's font folders with the bundle's own rules (the host's conf.d is written for its
# own, often newer, fontconfig, and the bundle's 2.13 warns at every line it does not know), and
# DejaVu for a host that has no fonts at all
mkdir -p "$STAGE/etc/fonts/conf.d"
for c in /etc/fonts/conf.d/*.conf; do cp -L "$c" "$STAGE/etc/fonts/conf.d/"; done
cat >"$STAGE/etc/fonts/fonts.conf" <<'FONTS'
<?xml version="1.0"?>
<!DOCTYPE fontconfig SYSTEM "urn:fontconfig:fonts.dtd">
<fontconfig>
  <include ignore_missing="yes">@ROOT@/etc/fonts/conf.d</include>
  <dir>/usr/share/fonts</dir>
  <dir>/usr/local/share/fonts</dir>
  <dir prefix="xdg">fonts</dir>
  <dir>/run/current-system/sw/share/X11/fonts</dir>
  <dir>@ROOT@/share/fonts</dir>
  <cachedir prefix="xdg">fontconfig-d2mm</cachedir>
</fontconfig>
FONTS

# --- caches with the bundle's own paths ---------------------------------------------------------
GDK="$STAGE/$LIB/gdk-pixbuf-2.0/2.10.0"
LD_LIBRARY_PATH="$STAGE/$LIB" GDK_PIXBUF_MODULEDIR="$GDK/loaders" \
  /usr/$LIB/gdk-pixbuf-2.0/gdk-pixbuf-query-loaders >"$GDK/loaders.cache"
sed -i "s|$STAGE|@ROOT@|g" "$GDK/loaders.cache"

IM="$STAGE/$LIB/gtk-3.0/3.0.0"
if [ -d "$IM/immodules" ]; then
  # a variable first: "$(tool ...)" as a command hides tool's failure behind "Permission denied"
  QUERY_IM=$(tool gtk-query-immodules-3.0)
  LD_LIBRARY_PATH="$STAGE/$LIB" "$QUERY_IM" "$IM"/immodules/*.so >"$IM/immodules.cache"
  sed -i "s|$STAGE|@ROOT@|g" "$IM/immodules.cache"
fi

QUERY_GIO=$(tool gio-querymodules)
COMPILE_SCHEMAS=$(tool glib-compile-schemas)
"$QUERY_GIO" "$STAGE/$LIB/gio/modules"
"$COMPILE_SCHEMAS" "$STAGE/share/glib-2.0/schemas"
update-mime-database "$STAGE/share/mime" >/dev/null 2>&1 || true

printf '%s\n' "# files that name this folder as @ROOT@ (src/store.rs writes it in)" \
  "$LIB/gdk-pixbuf-2.0/2.10.0/loaders.cache" "etc/fonts/fonts.conf" >"$STAGE/fixups.txt"
[ -f "$IM/immodules.cache" ] && echo "$LIB/gtk-3.0/3.0.0/immodules.cache" >>"$STAGE/fixups.txt"

# --- licenses: everything shipped keeps its copyright files --------------------------------------
mkdir -p "$STAGE/share/doc"
for so in "$STAGE/$LIB"/*.so*; do
  pkg=$(dpkg -S "$(basename "$so")" 2>/dev/null | head -1 | cut -d: -f1) || continue
  if [ -n "$pkg" ] && [ -f "/usr/share/doc/$pkg/copyright" ]; then
    mkdir -p "$STAGE/share/doc/$pkg"
    cp "/usr/share/doc/$pkg/copyright" "$STAGE/share/doc/$pkg/"
  fi
done
dpkg-query -W -f='${Package} ${Version}\n' >"$STAGE/share/doc/PACKAGES.txt"

# --- the oldest glibc it runs on: the newest GLIBC_ symbol version anything in it asks for -----
FLOOR=$(find "$STAGE" -type f | while read -r f; do
  if file -b "$f" | grep -q '^ELF 64-bit'; then objdump -T "$f" 2>/dev/null; fi
done | grep -o 'GLIBC_[0-9][0-9.]*' | sed 's/GLIBC_//' | sort -uV | tail -1)
echo "$FLOOR" >"$STAGE/share/doc/GLIBC.txt"
echo "needs glibc $FLOOR or newer"

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
  "check": "$LIB/libwebkit2gtk-4.1.so.0",
  "glibc": "$FLOOR"
}
EOF
echo "$OUT/$NAME.tar.gz $SIZE bytes"
