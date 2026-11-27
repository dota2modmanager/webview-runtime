# Plugging the WebView2 installer into Dota 2 Mod Manager (Windows)

People whose Windows lost its WebView2 - "debloated" or "optimized" builds, Edge removers,
EdgeUpdate blocked - see Mod Manager stop at start with *Could not find the webview runtime*, and
reinstalling Microsoft's runtime often does not help: the installer believes it is still there.
`webview2-setup.exe` puts a WebView2 of the app's own next to its data, and the app starts on it.

## What the installer is

* `webview2-setup.exe`, about 2 MB, no installer of its own, nothing installed system-wide, no
  administrator rights, no registry.
* It downloads Microsoft's **WebView2 Fixed Version** runtime (158.0.4370.45, 438 MB, from
  Microsoft's server), checks it against the size and SHA-256 built into it (`runtime.json`),
  unpacks it and grants the renderer's sandbox read access to the folder.
* It needs nothing a stripped Windows may lack: no EdgeUpdate, no expand.exe, no icacls, no
  certificate store (TLS has its own roots), no WebView2 (its window is drawn with GDI, in the
  app's colours).
* Its window: progress, Cancel (also the cross, Esc), Retry after a failure; movable by any part,
  resizable, Russian or English after the system's language.

## Calling it

```
webview2-setup.exe --base "<app data>\webview-runtime" --result "<temp>\webview2-setup.json"
```

| Exit code | Meaning |
|---|---|
| 0 | the engine is there (installed now, or already) |
| 1 | it failed; the window showed why, `--result` has `"error"` |
| 2 | the person cancelled |
| 3 | bad arguments |

`--result` file: `{"ok":true,"folder":"...","version":"158.0.4370.45"}` or
`{"ok":false,"error":"...","cancelled":false}`. `--quiet` runs without a window. `--lang ru|en`
overrides the language.

The folder is also kept in `<base>\current.json`:

```json
{ "platform": "windows-x64", "version": "158.0.4370.45",
  "folder": "C:\\Users\\...\\webview-runtime\\windows-x64-158.0.4370.45",
  "check": "msedgewebview2.exe" }
```

## What Mod Manager does

1. **At start, after it drops the `WEBVIEW2_*` variables** (`src-tauri/src/main.rs`, release builds):
   if `<base>\current.json` exists and `folder\check` is a file, set
   `WEBVIEW2_BROWSER_EXECUTABLE_FOLDER=<folder>`. It is the app's own folder, set by the app
   itself after the cleanup, so the reason for the cleanup (another program pointing the window at
   its own browser) does not apply.

   When to use it, simplest first:
   * always when it is there (the app then always runs on its own engine), or
   * only when the system's runtime is missing, or the last start did not reach the window (a
     flag file written before the window, removed by the page's first message).

2. **When the start fails for want of WebView2** (`src-tauri/src/startup_failure.rs`, the
   "webview" branch): instead of only the download link, run `webview2-setup.exe` (shipped beside
   the app, or downloaded from this repository's release) and wait. On exit code 0, start the app
   again; on 2, close; on 1, show the message as today.

A sketch of step 1:

```rust
#[cfg(windows)]
fn own_webview2(data_dir: &std::path::Path) {
    #[derive(serde::Deserialize)]
    struct Current { folder: std::path::PathBuf, check: String }
    let file = data_dir.join("webview-runtime").join("current.json");
    let Ok(bytes) = std::fs::read(file) else { return };
    let Ok(c) = serde_json::from_slice::<Current>(&bytes) else { return };
    if c.folder.join(&c.check).is_file() {
        std::env::set_var("WEBVIEW2_BROWSER_EXECUTABLE_FOLDER", &c.folder);
    }
}
```

## Checked

* This repository's tests (cab reader, manifest, store, decision rule) and clippy.
* On Windows 10 22H2: a full run (download, check, unpack in 7 s, sandbox access), cancel in the
  middle (exit 2, nothing left behind), and a WebView2 window through wry on the installed folder
  answering as 158.0.4370.45 where the system's is 158.0.4370.33 (`examples/selftest-win.rs`).
* Not yet: a Windows without WebView2 at all (the test VM did not finish installing: Hyper-V makes
  VirtualBox slow on that machine), and Mod Manager itself on it, which needs step 1.
