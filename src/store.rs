//! The engines on disk: `<base>/<platform>-<version>/`, each with a `.complete` mark written last.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::{fetch, unpack, Error, Launch, Platform, Runtime};

/// Written after everything else: a folder without it is a broken or unfinished install.
const MARK: &str = ".complete";
/// In a Linux bundle: the text files that name the bundle's own folder as `@ROOT@`.
const FIXUPS: &str = "fixups.txt";
const ROOT_TOKEN: &str = "@ROOT@";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Installed {
    pub dir: PathBuf,
    pub runtime: Runtime,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
struct Mark {
    sha256: String,
    version: String,
    /// Where the bundle's paths were written for; a moved folder is installed again.
    dir: String,
}

/// The engine `runtime` names, if it is here, whole, and was unpacked in this very folder.
pub fn find(base: &Path, runtime: &Runtime) -> Option<Installed> {
    let dir = base.join(runtime.folder_name());
    let mark: Mark = serde_json::from_slice(&std::fs::read(dir.join(MARK)).ok()?).ok()?;
    let same_engine = mark.sha256 == runtime.sha256 && mark.version == runtime.version;
    // the same folder however it was spelled: D:/x and D:\x, a symlink, a relative base
    let same_folder = match (std::fs::canonicalize(&mark.dir), std::fs::canonicalize(&dir)) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    };
    (same_engine && same_folder && dir.join(&runtime.check).is_file()).then(|| Installed { dir, runtime: runtime.clone() })
}

/// Downloads, checks and unpacks `runtime` into `base`, then removes older engines of the same
/// platform. Works in `.part` folders beside the final one and renames at the end, so a start
/// that dies halfway leaves nothing [`find`] would take.
pub fn install(base: &Path, runtime: &Runtime, progress: &mut dyn FnMut(u64, u64)) -> Result<Installed, Error> {
    std::fs::create_dir_all(base)?;
    let name = runtime.folder_name();
    let archive = base.join(format!("{name}.download"));
    fetch::download(runtime, &archive, progress)?;
    let result = install_from(base, runtime, &archive);
    let _ = std::fs::remove_file(&archive);
    result
}

/// [`install`] from an archive already on disk (checked again here).
pub fn install_from(base: &Path, runtime: &Runtime, archive: &Path) -> Result<Installed, Error> {
    fetch::check_file(archive, runtime)?;
    let name = runtime.folder_name();
    let part = base.join(format!(".part-{name}"));
    let dir = base.join(&name);
    remove(&part)?;
    unpack::unpack(runtime.archive, archive, &part)?;
    let engine = if runtime.root.is_empty() { part.clone() } else { part.join(&runtime.root) };
    if !engine.join(&runtime.check).is_file() {
        remove(&part)?;
        return Err(Error::Unpack(format!("{} is not in the archive", runtime.check)));
    }
    remove(&dir)?;
    std::fs::rename(&engine, &dir)?;
    remove(&part)?;
    if runtime.platform == Platform::LinuxX64 {
        write_root(&dir)?;
    }
    #[cfg(windows)]
    if runtime.platform == Platform::WindowsX64 {
        grant_sandbox(&dir)?;
    }
    let mark = Mark { sha256: runtime.sha256.clone(), version: runtime.version.clone(), dir: dir.display().to_string() };
    std::fs::write(dir.join(MARK), serde_json::to_vec_pretty(&mark).expect("a mark serializes"))?;
    remove_older(base, runtime, &name);
    let installed = find(base, runtime).ok_or_else(|| Error::Unpack("the unpacked engine was not found again".into()))?;
    write_current(base, &installed)?;
    Ok(installed)
}

/// `<base>/current.json`: the engine a caller that does not link this crate should use (Dota 2
/// Mod Manager reads it, INTEGRATION.md). Written last, through a temporary file.
pub const CURRENT: &str = "current.json";

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Current {
    pub platform: String,
    pub version: String,
    /// The engine folder: WEBVIEW2_BROWSER_EXECUTABLE_FOLDER on Windows.
    pub folder: String,
    /// The file that must be there, relative to `folder` (msedgewebview2.exe).
    pub check: String,
}

fn write_current(base: &Path, installed: &Installed) -> Result<(), Error> {
    let current = Current {
        platform: installed.runtime.platform.name().into(),
        version: installed.runtime.version.clone(),
        folder: installed.dir.display().to_string(),
        check: installed.runtime.check.clone(),
    };
    let tmp = base.join(format!("{CURRENT}.tmp"));
    std::fs::write(&tmp, serde_json::to_vec_pretty(&current).expect("a pointer serializes"))?;
    std::fs::rename(&tmp, base.join(CURRENT))?;
    Ok(())
}

/// The engine `<base>/current.json` names, if its file is really there.
pub fn current(base: &Path) -> Option<Current> {
    let c: Current = serde_json::from_slice(&std::fs::read(base.join(CURRENT)).ok()?).ok()?;
    Path::new(&c.folder).join(&c.check).is_file().then_some(c)
}

fn remove(path: &Path) -> std::io::Result<()> {
    match std::fs::remove_dir_all(path) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e),
        _ => Ok(()),
    }
}

fn remove_older(base: &Path, runtime: &Runtime, keep: &str) {
    let prefix = format!("{}-", runtime.platform.name());
    let Ok(entries) = std::fs::read_dir(base) else { return };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with(&prefix) && name != keep && entry.path().is_dir() {
            // an engine still in use by a running app cannot be removed on Windows; next time
            let _ = std::fs::remove_dir_all(entry.path());
        }
    }
}

/// GDK's loader cache, GTK's module cache and the like name absolute paths; the bundle ships them
/// with `@ROOT@` and lists them in `fixups.txt`.
fn write_root(dir: &Path) -> Result<(), Error> {
    let Ok(list) = std::fs::read_to_string(dir.join(FIXUPS)) else { return Ok(()) };
    let root = dir.display().to_string();
    for rel in list.lines().map(str::trim).filter(|l| !l.is_empty() && !l.starts_with('#')) {
        if rel.split('/').any(|s| s == ".." || s.is_empty()) {
            return Err(Error::Unpack(format!("fixups.txt names {rel}")));
        }
        let file = dir.join(rel);
        let text = std::fs::read_to_string(&file)?;
        std::fs::write(&file, text.replace(ROOT_TOKEN, &root))?;
    }
    Ok(())
}

/// The renderer runs in an AppContainer sandbox, which may read only folders that grant
/// "ALL APPLICATION PACKAGES" (S-1-15-2-1) and "ALL RESTRICTED APPLICATION PACKAGES"
/// (S-1-15-2-2). Program Files does; the user's AppData does not, and WebView2's renderer then dies
/// at once (Microsoft's notes on the Fixed Version runtime). Read and execute, inherited by every
/// file and folder below, set through the Win32 API rather than icacls, which a stripped Windows
/// may lack.
#[cfg(windows)]
fn grant_sandbox(dir: &Path) -> Result<(), Error> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Foundation::{LocalFree, ERROR_SUCCESS};
    use windows_sys::Win32::Security::Authorization::{
        ConvertStringSidToSidW, GetNamedSecurityInfoW, SetEntriesInAclW, SetNamedSecurityInfoW, EXPLICIT_ACCESS_W, GRANT_ACCESS,
        NO_MULTIPLE_TRUSTEE, SE_FILE_OBJECT, TRUSTEE_IS_SID, TRUSTEE_IS_WELL_KNOWN_GROUP, TRUSTEE_W,
    };
    use windows_sys::Win32::Security::{ACL, DACL_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR, PSID, SUB_CONTAINERS_AND_OBJECTS_INHERIT};

    // FILE_GENERIC_READ | FILE_GENERIC_EXECUTE, what icacls calls (RX)
    const READ_EXECUTE: u32 = 0x0012_0089 | 0x0012_00A0;
    let wide = |s: &std::ffi::OsStr| s.encode_wide().chain(Some(0)).collect::<Vec<u16>>();
    let fail = |what: &str, code: u32| Error::Unpack(format!("granting the sandbox read access: {what} failed ({code})"));

    let path = wide(dir.as_os_str());
    let mut sids: Vec<PSID> = Vec::new();
    for s in ["S-1-15-2-1", "S-1-15-2-2"] {
        let text = wide(std::ffi::OsStr::new(s));
        let mut sid: PSID = std::ptr::null_mut();
        // SAFETY: text is NUL-terminated; sid is freed with LocalFree below
        if unsafe { ConvertStringSidToSidW(text.as_ptr(), &mut sid) } == 0 {
            return Err(fail("ConvertStringSidToSidW", 0));
        }
        sids.push(sid);
    }
    let entries: Vec<EXPLICIT_ACCESS_W> = sids
        .iter()
        .map(|&sid| EXPLICIT_ACCESS_W {
            grfAccessPermissions: READ_EXECUTE,
            grfAccessMode: GRANT_ACCESS,
            grfInheritance: SUB_CONTAINERS_AND_OBJECTS_INHERIT,
            Trustee: TRUSTEE_W {
                pMultipleTrustee: std::ptr::null_mut(),
                MultipleTrusteeOperation: NO_MULTIPLE_TRUSTEE,
                TrusteeForm: TRUSTEE_IS_SID,
                TrusteeType: TRUSTEE_IS_WELL_KNOWN_GROUP,
                ptstrName: sid as *mut u16,
            },
        })
        .collect();

    let mut old: *mut ACL = std::ptr::null_mut();
    let mut sd: PSECURITY_DESCRIPTOR = std::ptr::null_mut();
    let mut new: *mut ACL = std::ptr::null_mut();
    // SAFETY: path is NUL-terminated; old points into sd, which is freed after use; new is
    // allocated by SetEntriesInAclW and freed with LocalFree
    let result = unsafe {
        let rc = GetNamedSecurityInfoW(
            path.as_ptr(), SE_FILE_OBJECT, DACL_SECURITY_INFORMATION,
            std::ptr::null_mut(), std::ptr::null_mut(), &mut old, std::ptr::null_mut(), &mut sd,
        );
        if rc != ERROR_SUCCESS {
            Err(fail("GetNamedSecurityInfoW", rc))
        } else {
            let rc = SetEntriesInAclW(entries.len() as u32, entries.as_ptr(), old, &mut new);
            if rc != ERROR_SUCCESS {
                Err(fail("SetEntriesInAclW", rc))
            } else {
                // a folder's new inheritable entries reach the files already in it
                let rc = SetNamedSecurityInfoW(
                    path.as_ptr(), SE_FILE_OBJECT, DACL_SECURITY_INFORMATION,
                    std::ptr::null_mut(), std::ptr::null_mut(), new, std::ptr::null(),
                );
                if rc != ERROR_SUCCESS { Err(fail("SetNamedSecurityInfoW", rc)) } else { Ok(()) }
            }
        }
    };
    // SAFETY: each was allocated by the API that handed it out, and is freed once
    unsafe {
        if !new.is_null() {
            LocalFree(new as _);
        }
        if !sd.is_null() {
            LocalFree(sd as _);
        }
        for sid in sids {
            LocalFree(sid as _);
        }
    }
    result
}

/// The environment of a Linux bundle (`linux/build.sh` lays it out). Everything comes from the
/// bundle; only the display server and the person's fonts and settings come from the host, and
/// glibc too unless `own_glibc` ([`crate::system::needs_own_glibc`]).
///
/// With `own_glibc` the program runs through the bundle's loader (`Launch::wrap`), and the
/// bundle's executables find it through `D2MM_GLIBC_LOADER` (build.sh's wrappers). The library
/// path is then the loader's `--library-path`, not LD_LIBRARY_PATH: a host program started from
/// the app (a musl `xdg-open`, Nix's tools) must not be pointed at glibc builds of its libraries.
pub fn linux_launch(dir: &Path, software: bool, own_glibc: bool) -> Launch {
    let d = |rel: &str| dir.join(rel).display().to_string();
    let lib = "lib/x86_64-linux-gnu";
    let host_data = std::env::var("XDG_DATA_DIRS").ok().filter(|v| !v.is_empty());
    let mut vars: Vec<(String, String)> = vec![
        ("GDK_PIXBUF_MODULE_FILE".into(), d(&format!("{lib}/gdk-pixbuf-2.0/2.10.0/loaders.cache"))),
        ("GIO_MODULE_DIR".into(), d(&format!("{lib}/gio/modules"))),
        ("GTK_PATH".into(), d(&format!("{lib}/gtk-3.0"))),
        ("GTK_EXE_PREFIX".into(), d("")),
        ("GTK_DATA_PREFIX".into(), d("")),
        ("GTK_IM_MODULE_FILE".into(), d(&format!("{lib}/gtk-3.0/3.0.0/immodules.cache"))),
        ("GSETTINGS_SCHEMA_DIR".into(), d("share/glib-2.0/schemas")),
        ("GST_PLUGIN_SYSTEM_PATH_1_0".into(), d(&format!("{lib}/gstreamer-1.0"))),
        ("GST_PLUGIN_SCANNER_1_0".into(), d(&format!("{lib}/gstreamer1.0/gstreamer-1.0/gst-plugin-scanner"))),
        ("GST_REGISTRY_1_0".into(), d("gst-registry.bin")),
        ("LIBGL_DRIVERS_PATH".into(), d(&format!("{lib}/dri"))),
        ("GBM_BACKENDS_PATH".into(), d(&format!("{lib}/gbm"))),
        ("__EGL_VENDOR_LIBRARY_DIRS".into(), d("share/glvnd/egl_vendor.d")),
        // the host's font folders with the bundle's own rules, and DejaVu for hosts with none
        ("FONTCONFIG_FILE".into(), d("etc/fonts/fonts.conf")),
        ("XDG_DATA_DIRS".into(), format!("{}:{}", d("share"), host_data.unwrap_or_else(|| "/usr/local/share:/usr/share".into()))),
        // No WEBKIT_DISABLE_DMABUF_RENDERER: with WebKitGTK 2.52 the old path it selects crashed
        // Dota 2 Mod Manager's window on Mint 22.3 and EndeavourOS (2026-11-27), and the DMA-BUF
        // renderer falls back to shared memory by itself where the GPU cannot share buffers.
    ];
    let mut wrap = Vec::new();
    if own_glibc {
        let libs = format!("{}:{}", d("glibc"), d(lib));
        let loader = d("glibc/ld-linux-x86-64.so.2");
        vars.push(("D2MM_GLIBC_LOADER".into(), loader.clone()));
        vars.push(("D2MM_GLIBC_LIBRARY_PATH".into(), libs.clone()));
        // iconv's converters and the one locale the bundle carries: the host's are musl's or Nix's
        vars.push(("GCONV_PATH".into(), d("glibc/gconv")));
        vars.push(("LOCPATH".into(), d("glibc/locale")));
        vars.push(("LC_ALL".into(), "C.UTF-8".into()));
        wrap = vec![loader, "--library-path".into(), libs];
    } else {
        vars.push(("LD_LIBRARY_PATH".into(), d(lib)));
    }
    if software {
        vars.push(("LIBGL_ALWAYS_SOFTWARE".into(), "1".into()));
        vars.push(("WEBKIT_DISABLE_COMPOSITING_MODE".into(), "1".into()));
    }
    // libwebkit2gtk is patched to find its helper processes at ././/lib/... (linux/build.sh)
    Launch { vars, cwd: Some(dir.to_path_buf()), wrap }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::Archive;
    use sha2::{Digest, Sha256};

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("wvr-store-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn bundle(path: &Path, files: &[(&str, &str)]) -> Runtime {
        let gz = flate2::write::GzEncoder::new(std::fs::File::create(path).unwrap(), flate2::Compression::fast());
        let mut b = tar::Builder::new(gz);
        for (name, text) in files {
            let mut h = tar::Header::new_gnu();
            h.set_size(text.len() as u64);
            h.set_mode(0o644);
            b.append_data(&mut h, name, text.as_bytes()).unwrap();
        }
        b.into_inner().unwrap().finish().unwrap();
        let data = std::fs::read(path).unwrap();
        Runtime {
            platform: Platform::LinuxX64,
            version: "2.50.1".into(),
            archive: Archive::TarGz,
            size: data.len() as u64,
            sha256: hex::encode(Sha256::digest(&data)),
            urls: vec!["https://example.org/a.tar.gz".into()],
            root: "webkit".into(),
            check: "lib/x86_64-linux-gnu/libwebkit2gtk-4.1.so.0".into(),
            glibc: "2.35".into(),
        }
    }

    #[test]
    fn an_install_is_found_and_names_its_own_folder() {
        let base = temp("ok");
        let archive = base.join("in.tar.gz");
        let rt = bundle(&archive, &[
            ("webkit/lib/x86_64-linux-gnu/libwebkit2gtk-4.1.so.0", "elf"),
            ("webkit/loaders.cache", "\"@ROOT@/lib/libpixbufloader-png.so\""),
            ("webkit/fixups.txt", "# paths\nloaders.cache\n"),
        ]);
        // an older engine of the same platform goes; a half-done one is ignored
        std::fs::create_dir_all(base.join("linux-x64-2.48.0")).unwrap();
        assert!(find(&base, &rt).is_none());
        let got = install_from(&base, &rt, &archive).unwrap();
        assert_eq!(got.dir, base.join("linux-x64-2.50.1"));
        let cache = std::fs::read_to_string(got.dir.join("loaders.cache")).unwrap();
        assert_eq!(cache, format!("\"{}/lib/libpixbufloader-png.so\"", got.dir.display()));
        assert!(!base.join("linux-x64-2.48.0").exists());
        assert!(!base.join(".part-linux-x64-2.50.1").exists());
        assert_eq!(find(&base, &rt), Some(got.clone()));
        let c = current(&base).expect("current.json names the engine");
        assert_eq!((c.version.as_str(), c.folder), ("2.50.1", got.dir.display().to_string()));
        // the same base spelled another way finds it too
        let respelled = base.join(".").join("..").join(base.file_name().unwrap());
        assert!(find(&respelled, &rt).is_some());
    }

    #[test]
    fn a_folder_without_its_mark_or_check_file_is_not_an_engine() {
        let base = temp("broken");
        let archive = base.join("in.tar.gz");
        let rt = bundle(&archive, &[("webkit/lib/x86_64-linux-gnu/libwebkit2gtk-4.1.so.0", "elf")]);
        let got = install_from(&base, &rt, &archive).unwrap();
        std::fs::remove_file(got.dir.join(&rt.check)).unwrap();
        assert!(find(&base, &rt).is_none());
        std::fs::remove_file(got.dir.join(MARK)).unwrap();
        assert!(find(&base, &rt).is_none());
    }

    #[test]
    fn an_archive_without_the_engine_is_refused() {
        let base = temp("empty");
        let archive = base.join("in.tar.gz");
        let rt = bundle(&archive, &[("webkit/readme", "nothing here")]);
        assert!(matches!(install_from(&base, &rt, &archive), Err(Error::Unpack(_))));
        assert!(!base.join(rt.folder_name()).exists());
    }

    #[test]
    fn a_changed_archive_is_refused_before_unpacking() {
        let base = temp("changed");
        let archive = base.join("in.tar.gz");
        let mut rt = bundle(&archive, &[("webkit/lib/x86_64-linux-gnu/libwebkit2gtk-4.1.so.0", "elf")]);
        rt.sha256 = "0".repeat(64);
        assert!(matches!(install_from(&base, &rt, &archive), Err(Error::Mismatch { .. })));
        assert!(!base.join(format!(".part-{}", rt.folder_name())).exists());
    }

    #[test]
    fn software_rendering_is_added_only_when_asked() {
        let dir = Path::new("/home/u/.local/share/d2mm/webview-runtime/linux-x64-2.50.1");
        let plain = linux_launch(dir, false, false);
        assert_eq!(plain.cwd.as_deref(), Some(dir));
        assert!(plain.wrap.is_empty());
        assert!(plain.vars.iter().any(|(k, v)| k == "LD_LIBRARY_PATH" && v.ends_with("lib/x86_64-linux-gnu")));
        assert!(!plain.vars.iter().any(|(k, _)| k == "LIBGL_ALWAYS_SOFTWARE"));
        assert!(!plain.vars.iter().any(|(k, _)| k == "WEBKIT_DISABLE_DMABUF_RENDERER"), "it crashes WebKitGTK 2.52 windows");
        assert!(linux_launch(dir, true, false).vars.iter().any(|(k, _)| k == "LIBGL_ALWAYS_SOFTWARE"));
    }

    #[test]
    fn own_glibc_runs_through_the_bundle_s_loader_without_ld_library_path() {
        let dir = Path::new("/home/u/.local/share/d2mm/webview-runtime/linux-x64-2.50.1");
        let own = linux_launch(dir, false, true);
        assert!(own.wrap[0].ends_with("ld-linux-x86-64.so.2"));
        assert_eq!(own.wrap[1], "--library-path");
        assert!(own.wrap[2].contains("glibc") && own.wrap[2].contains("lib/x86_64-linux-gnu"));
        assert!(!own.vars.iter().any(|(k, _)| k == "LD_LIBRARY_PATH"), "host programs started from the app keep their libraries");
        assert!(own.vars.iter().any(|(k, v)| k == "D2MM_GLIBC_LOADER" && *v == own.wrap[0]));
    }
}
