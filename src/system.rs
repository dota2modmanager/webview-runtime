//! Whether the system's own engine is there: its files, not just a registry value or a package
//! record that outlived them.

/// The system's engine is present on this machine (not that it works: [`crate::Health`] tells).
pub fn engine_present() -> bool {
    imp::engine_present()
}

/// EdgeUpdate's client id of the WebView2 runtime.
pub const WEBVIEW2_CLIENT: &str = "{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}";

/// A `pv` value that names an installed runtime. Microsoft writes 0.0.0.0 after an uninstall.
pub fn real_version(pv: &str) -> bool {
    let parts: Vec<&str> = pv.split('.').collect();
    parts.len() == 4 && parts.iter().all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit())) && pv != "0.0.0.0"
}

/// `2.35` as (2, 35); anything else None.
pub fn parse_version(v: &str) -> Option<(u32, u32)> {
    let (major, minor) = v.trim().split_once('.')?;
    Some((major.parse().ok()?, minor.parse().ok()?))
}

/// What the host offers a glibc program, as far as the bundle cares.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Host {
    /// The glibc version behind `/lib64/ld-linux-x86-64.so.2`, None where there is no such loader
    /// (musl: Alpine, Void musl) or it is not glibc's.
    pub glibc: Option<(u32, u32)>,
    /// NixOS: its loader at /lib64, where there is one, is nix-ld's stand-in, and its glibc lives
    /// in /nix/store under a version the bundle's libraries were not built against.
    pub nixos: bool,
}

/// Whether the bundle must run on the glibc it carries, through its own loader, rather than on
/// the host's: no glibc, NixOS, or a glibc older than the bundle's floor (Ubuntu 20.04, Debian 11).
pub fn needs_own_glibc(host: &Host, floor: &str) -> bool {
    let Some(have) = host.glibc else { return true };
    if host.nixos {
        return true;
    }
    match parse_version(floor) {
        Some(need) => have < need,
        None => false,
    }
}

/// This machine, for [`needs_own_glibc`]. Only meaningful on Linux.
pub fn host() -> Host {
    imp::host()
}

#[cfg(windows)]
mod imp {
    use super::{real_version, WEBVIEW2_CLIENT};
    use std::path::PathBuf;
    use windows_sys::Win32::System::Registry::{
        RegGetValueW, HKEY, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, RRF_RT_REG_SZ, RRF_SUBKEY_WOW6432KEY,
    };

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(Some(0)).collect()
    }

    fn pv(root: HKEY, key: &str, flags: u32) -> Option<String> {
        let key = wide(key);
        let name = wide("pv");
        let mut buf = vec![0u16; 64];
        let mut len = (buf.len() * 2) as u32;
        // SAFETY: the buffers are ours, NUL-terminated, and len is the buffer's size in bytes
        let rc = unsafe {
            RegGetValueW(root, key.as_ptr(), name.as_ptr(), RRF_RT_REG_SZ | flags, std::ptr::null_mut(), buf.as_mut_ptr().cast(), &mut len)
        };
        if rc != 0 {
            return None;
        }
        let chars = (len as usize / 2).saturating_sub(1);
        Some(String::from_utf16_lossy(&buf[..chars.min(buf.len())]))
    }

    pub fn engine_present() -> bool {
        let key = format!("SOFTWARE\\Microsoft\\EdgeUpdate\\Clients\\{WEBVIEW2_CLIENT}");
        let folders = [
            std::env::var_os("ProgramFiles(x86)").map(PathBuf::from),
            std::env::var_os("ProgramFiles").map(PathBuf::from),
            std::env::var_os("LOCALAPPDATA").map(PathBuf::from),
        ];
        [pv(HKEY_LOCAL_MACHINE, &key, RRF_SUBKEY_WOW6432KEY), pv(HKEY_LOCAL_MACHINE, &key, 0), pv(HKEY_CURRENT_USER, &key, 0)]
            .into_iter()
            .flatten()
            .filter(|v| real_version(v))
            .any(|v| {
                folders.iter().flatten().any(|f| {
                    f.join("Microsoft\\EdgeWebView\\Application").join(&v).join("msedgewebview2.exe").is_file()
                })
            })
    }

    pub fn host() -> super::Host {
        super::Host { glibc: None, nixos: false }
    }
}

#[cfg(target_os = "linux")]
mod imp {
    /// The same folders the app's AppImage start looks in (mm-core host_env).
    const LIB_DIRS: &[&str] = &["/usr/lib/x86_64-linux-gnu", "/lib/x86_64-linux-gnu", "/usr/lib64", "/usr/lib"];

    pub fn engine_present() -> bool {
        LIB_DIRS.iter().any(|d| std::path::Path::new(d).join("libwebkit2gtk-4.1.so.0").exists())
    }

    const LOADER: &str = "/lib64/ld-linux-x86-64.so.2";

    pub fn host() -> super::Host {
        let nixos = std::path::Path::new("/etc/NIXOS").exists();
        let glibc = std::path::Path::new(LOADER).exists().then(glibc_version).flatten();
        super::Host { glibc, nixos }
    }

    /// glibc 2.33 and newer answer `ld.so --version` ("... release version 2.39."); older ones,
    /// and a loader that is not glibc's, fall through to getconf.
    fn glibc_version() -> Option<(u32, u32)> {
        let run = |cmd: &str, args: &[&str]| {
            std::process::Command::new(cmd).args(args).output().ok().filter(|o| o.status.success()).map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
        };
        if let Some(v) = run(LOADER, &["--version"]).as_deref().and_then(super::version_in_ld_so) {
            return Some(v);
        }
        let text = run("getconf", &["GNU_LIBC_VERSION"])?;
        super::parse_version(text.trim().strip_prefix("glibc ")?)
    }
}

/// The version in the first line of `ld.so --version`.
pub fn version_in_ld_so(text: &str) -> Option<(u32, u32)> {
    let line = text.lines().next()?;
    if !line.contains("release version") {
        return None;
    }
    line.split_whitespace().last().and_then(|w| parse_version(w.trim_end_matches('.')))
}

#[cfg(not(any(windows, target_os = "linux")))]
mod imp {
    pub fn engine_present() -> bool {
        true
    }

    pub fn host() -> super::Host {
        super::Host { glibc: None, nixos: false }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_leftover_pv_is_not_a_runtime() {
        assert!(real_version("142.0.3595.53"));
        assert!(!real_version("0.0.0.0"));
        assert!(!real_version(""));
        assert!(!real_version("142.0"));
        assert!(!real_version("142.0.x.1"));
    }

    #[test]
    fn the_loader_s_version_line_is_read() {
        let text = "ld.so (Ubuntu GLIBC 2.39-0ubuntu8.6) stable release version 2.39.
Copyright (C) 2024";
        assert_eq!(version_in_ld_so(text), Some((2, 39)));
        let text = "ld.so (GNU libc) stable release version 2.41.
";
        assert_eq!(version_in_ld_so(text), Some((2, 41)));
        assert_eq!(version_in_ld_so("nix-ld 2.0.4"), None);
    }

    #[test]
    fn own_glibc_where_the_host_has_none_or_an_old_one_or_is_nixos() {
        let host = |glibc, nixos| Host { glibc, nixos };
        assert!(needs_own_glibc(&host(None, false), "2.35"), "Alpine, Void musl");
        assert!(needs_own_glibc(&host(Some((2, 39)), true), "2.35"), "NixOS with nix-ld");
        assert!(needs_own_glibc(&host(Some((2, 31)), false), "2.35"), "Ubuntu 20.04");
        assert!(!needs_own_glibc(&host(Some((2, 35)), false), "2.35"));
        assert!(!needs_own_glibc(&host(Some((2, 41)), false), "2.35"));
        assert!(!needs_own_glibc(&host(Some((2, 9)), false), ""), "no floor known: the host's");
        assert!(parse_version("2.10") > parse_version("2.9"));
    }
}
