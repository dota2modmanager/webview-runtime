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
}

#[cfg(target_os = "linux")]
mod imp {
    /// The same folders the app's AppImage start looks in (mm-core host_env).
    const LIB_DIRS: &[&str] = &["/usr/lib/x86_64-linux-gnu", "/lib/x86_64-linux-gnu", "/usr/lib64", "/usr/lib"];

    pub fn engine_present() -> bool {
        LIB_DIRS.iter().any(|d| std::path::Path::new(d).join("libwebkit2gtk-4.1.so.0").exists())
    }
}

#[cfg(not(any(windows, target_os = "linux")))]
mod imp {
    pub fn engine_present() -> bool {
        true
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
}
