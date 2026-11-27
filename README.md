# webview-runtime

A WebView engine of Dota 2 Mod Manager's own, for machines where the system's engine is
missing or broken. The app keeps using the system's engine while it works; this one is fetched
and used only when the system has none, or the last start never got its window up.

| | Windows | Linux |
|---|---|---|
| System engine | Microsoft Edge WebView2 (Evergreen), shared, updated by EdgeUpdate | WebKitGTK 4.1 from the distribution, or the AppImage's copy mixed with the host's libraries |
| Ours | WebView2 **Fixed Version** runtime, Microsoft's own signed files, unchanged | WebKitGTK 4.1 **with its whole library stack**, Mesa and glibc included, built on Ubuntu 24.04 |
| Where it goes | `%LOCALAPPDATA%\dota2-mod-manager\webview-runtime\windows-x64-<version>` | `~/.local/share/dota2-mod-manager/webview-runtime/linux-x64-<version>` |
| Size | 438 MB packed (158.0.4370.45) | about 160 MB packed, 420 MB unpacked; Adwaita icons and Mesa's drivers are most of it |

## How it differs from the system's engine

**Windows.** The system's WebView2 is one copy for every program, installed through EdgeUpdate
and found through the registry. That is what breaks: debloat scripts and Edge removers delete
the files and leave the registry key, Microsoft writes `0.0.0.0` into it after an uninstall,
EdgeUpdate is blocked by policy or a firewall, and the installer then believes the runtime is
there. Ours is a fixed version unpacked into the app's own folder:

* it is not installed: no EdgeUpdate, no registry, no administrator rights, no system-wide change;
* nothing else can remove or update it: it changes only when the app ships a new manifest;
* the app points WebView2 at it with `WEBVIEW2_BROWSER_EXECUTABLE_FOLDER`, set by the app itself
  after it drops every `WEBVIEW2_*` variable inherited from outside (Mod Manager's `main.rs`);
* it does not update itself, so the app must ship a new one for security fixes: the release
  workflow takes Microsoft's newest Fixed Version, and each Mod Manager release pins one.

**Linux.** The system's engine is the distribution's WebKitGTK, which drags in the distribution's
GTK, GLib and GPU driver; the AppImage carries its own WebKitGTK but loads the host's GTK, GLib
and Mesa next to it (mm-core `host_env`). Mixing them is what drew black windows and crashed
GTK on rolling distributions (#761, #797), and a distribution without WebKitGTK 4.1 cannot run
the app at all. Ours is one consistent set:

* WebKitGTK, GTK, GLib, GStreamer, ICU and **Mesa**, all from the same base system; only glibc
  and the display server (X11 or Wayland) come from the host, so it does not matter which
  distribution, package manager or library versions the host has;
* the helper processes (`WebKitWebProcess`, `WebKitNetworkProcess`) are found inside the bundle
  (see `linux/build.sh` on how);
* after two failed starts on it, it runs with software
  rendering (`LIBGL_ALWAYS_SOFTWARE`, compositing off), which works on any machine, only slower;
* NVIDIA's proprietary driver is not in it: on such a machine ours draws with Mesa, in software.

### Which distributions

The bundle carries glibc too. On a host whose glibc is at least the build base's (Ubuntu 24.04:
2.39, written in `runtime.json` and `share/doc/GLIBC.txt`) it runs on the host's glibc; on any
other x86-64 Linux - an older glibc, musl, NixOS - it runs on its own, started through its own
loader, and every executable in it is a small wrapper that does the same for itself, so WebKit's
helper processes load too. Only the display server (X11 or Wayland) and the fonts come from the
host.

| Host | glibc | Checked |
|---|---|---|
| Ubuntu 24.04+, Mint 22+, Debian 13, Kali, Arch, Manjaro, EndeavourOS, Fedora 40+, Tumbleweed | the host's | CI containers with Xvfb; Ubuntu 24.04.5, Mint 22.3 and EndeavourOS desktops in VMs |
| Ubuntu 22.04, Mint 21, Debian 12, older | the bundle's | CI containers with Xvfb |
| Alpine, Void musl, NixOS and Nix hosts | the bundle's | CI containers with Xvfb |

The CI check (`linux/smoke.sh`, `.github/workflows/linux.yml`) unpacks the bundle in each
distribution's container, checks that every library resolves inside it, installs it with the
static launcher, and draws a page with `wvr-selftest` on Xvfb: the page's script must turn it
green, and libepoxy must find GL. `linux/vm` holds the scripts that ran the same check on the VMs'
own desktops and then started Dota 2 Mod Manager 4.7.1 on the bundle.

## When the app switches

`src/lib.rs` `decide` is the whole rule:

1. the system's engine is used while it is there and the last start reached its page;
2. no system engine, or the last start never reached its page (`Health`): ours, fetched if needed;
3. ours failed twice too: ours with software rendering;
4. `D2MM_OWN_WEBVIEW=1` forces ours, `=0` forbids it.

A start counts as failed until the page's first message calls `Health::ready`, because a broken
engine usually does not fail with an error: WebView2 hangs on a half-removed runtime, WebKitGTK
dies in the GPU driver.

## Trust

* `runtime.json` names each archive by size and SHA-256 and is compiled into the app, so only
  the archives the app was built with can be run; a download that differs is thrown away
  before it is unpacked.
* The Windows cab is Microsoft's file, unchanged. `windows/fetch-fixed.ps1` checks that every
  exe and dll in it carries Microsoft's valid Authenticode signature before an entry is made.
* Every url must be https; our release comes first and Microsoft's link second.
* The renderer's sandbox can read only folders that grant `ALL APPLICATION PACKAGES`; the
  installer grants it on our folder (icacls), as Microsoft's notes on Fixed Version require.

## Layout

```
src/            the crate the app links: decide, fetch, unpack, store, health, system
src/bin/        webview-runtime: status, install, run (for testing by hand and in CI)
windows/        fetch-fixed.ps1: Microsoft's cab to a runtime.json entry
linux/          build.sh: the bundle; smoke.sh: checks it on Fedora, Arch and Debian
runtime.json    the manifest the app embeds (empty until the first release)
```

## Using it from Mod Manager

```rust
let base = data_dir.join("webview-runtime");
let health = webview_runtime::Health::open(&base.join("health"));
match webview_runtime::choose(&base, &health, settings.own_webview) {
    Choice::System => {}
    // install() returns the engine already there, or fetches it (show a progress window)
    Choice::Own { .. } | Choice::InstallOwn { .. } => {
        let installed = webview_runtime::install(&base, &mut |done, size| progress(done, size))?;
        apply(webview_runtime::env_for(&installed, health.software_rendering()));
    }
    Choice::Nothing => { /* the startup_failure message, as today */ }
}
health.begin()?;
// ... the window; its page's first message calls health.ready()
```

On Windows the variables are set in the app's own process before the first window. On Linux
the app links libwebkit2gtk itself, so it starts itself again with the bundle's environment, the
way it already does for the AppImage (`host_libs_once`).

## Testing by hand

```
cargo test
cargo run -- status
cargo run -- install --manifest my.json --from Microsoft.WebView2.FixedVersionRuntime.<v>.x64.cab
cargo run -- run --manifest my.json -- "C:\path\to\Dota 2 Mod Manager.exe"
```

## Licenses

The crate is MIT. The WebView2 Fixed Version runtime is redistributed under Microsoft's WebView2
runtime terms, which allow shipping it with an app. The Linux bundle carries each Ubuntu
package's copyright file in `share/doc/<package>/` and the list of packages and versions.
