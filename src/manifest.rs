//! `runtime.json`: which engine archives exist, and exactly what each must be.

use serde::{Deserialize, Serialize};

use crate::Error;

/// The format this crate reads. A manifest of another format is refused, not guessed at.
pub const FORMAT: u32 = 1;
/// Nothing we ship comes near this; a manifest that says more is wrong, not big.
pub const MAX_SIZE: u64 = 1 << 30;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Manifest {
    pub format: u32,
    #[serde(default)]
    pub runtimes: Vec<Runtime>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Runtime {
    pub platform: Platform,
    /// The engine's own version: WebView2 `142.0.3595.53`, WebKitGTK `2.50.1`.
    pub version: String,
    pub archive: Archive,
    pub size: u64,
    /// Lowercase hex SHA-256 of the archive.
    pub sha256: String,
    /// Tried in order: our release first, the vendor's own link after it.
    pub urls: Vec<String>,
    /// The folder inside the archive that is the engine (WebView2's cab holds one folder named
    /// after the version); empty when the archive's root is the engine.
    #[serde(default)]
    pub root: String,
    /// A file that must be there after unpacking, relative to the engine folder.
    pub check: String,
    /// Linux: the oldest host glibc the bundle runs on with the host's own loader (`2.35`). An
    /// older one, none (musl), or NixOS: the bundle runs on the glibc it carries.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub glibc: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Platform {
    #[serde(rename = "windows-x64")]
    WindowsX64,
    #[serde(rename = "linux-x64")]
    LinuxX64,
}

impl Platform {
    pub fn this() -> Option<Platform> {
        match (std::env::consts::OS, std::env::consts::ARCH) {
            ("windows", "x86_64") => Some(Platform::WindowsX64),
            ("linux", "x86_64") => Some(Platform::LinuxX64),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Platform::WindowsX64 => "windows-x64",
            Platform::LinuxX64 => "linux-x64",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Archive {
    /// Microsoft's cabinet, as Microsoft publishes it.
    #[serde(rename = "cab")]
    Cab,
    #[serde(rename = "tar.gz")]
    TarGz,
}

impl Manifest {
    pub fn parse(text: &str) -> Result<Manifest, Error> {
        let m: Manifest = serde_json::from_str(text).map_err(|e| Error::Manifest(e.to_string()))?;
        if m.format != FORMAT {
            return Err(Error::Manifest(format!("format {} is not {FORMAT}", m.format)));
        }
        for r in &m.runtimes {
            r.validate()?;
        }
        Ok(m)
    }

    pub fn for_platform(&self, p: Platform) -> Option<&Runtime> {
        self.runtimes.iter().find(|r| r.platform == p)
    }

    pub fn for_this_platform(&self) -> Option<&Runtime> {
        Platform::this().and_then(|p| self.for_platform(p))
    }
}

impl Runtime {
    fn validate(&self) -> Result<(), Error> {
        let bad = |why: String| Err(Error::Manifest(format!("{} {}: {why}", self.platform.name(), self.version)));
        if self.sha256.len() != 64 || !self.sha256.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)) {
            return bad("sha256 is not 64 lowercase hex digits".into());
        }
        if self.size == 0 || self.size > MAX_SIZE {
            return bad(format!("size {} is out of range", self.size));
        }
        if self.urls.is_empty() || self.urls.iter().any(|u| !u.starts_with("https://")) {
            return bad("every url must be https".into());
        }
        // the version names a folder of ours, the root and check name paths inside one
        if !plain(&self.version) || !self.root.split('/').all(|s| s.is_empty() || plain(s)) {
            return bad("version or root is not a plain name".into());
        }
        if self.check.is_empty() || !self.check.split('/').all(plain) {
            return bad("check is not a plain relative path".into());
        }
        if !self.glibc.is_empty() && crate::system::parse_version(&self.glibc).is_none() {
            return bad(format!("glibc {} is not a version", self.glibc));
        }
        Ok(())
    }

    /// The folder this engine lives in under the app's `base`.
    pub fn folder_name(&self) -> String {
        format!("{}-{}", self.platform.name(), self.version)
    }
}

fn plain(s: &str) -> bool {
    !s.is_empty() && s != "." && s != ".." && s.chars().all(|c| c.is_ascii_alphanumeric() || "._-+".contains(c))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn runtime() -> serde_json::Value {
        serde_json::json!({
            "platform": "windows-x64", "version": "142.0.3595.53", "archive": "cab",
            "size": 180_000_000u64, "sha256": "ab".repeat(32),
            "urls": ["https://example.org/a.cab"],
            "root": "Microsoft.WebView2.FixedVersionRuntime.142.0.3595.53.x64",
            "check": "msedgewebview2.exe"
        })
    }

    fn parse(r: serde_json::Value) -> Result<Manifest, Error> {
        Manifest::parse(&serde_json::json!({ "format": 1, "runtimes": [r] }).to_string())
    }

    #[test]
    fn a_good_entry_parses() {
        let m = parse(runtime()).unwrap();
        assert_eq!(m.for_platform(Platform::WindowsX64).unwrap().folder_name(), "windows-x64-142.0.3595.53");
        assert!(m.for_platform(Platform::LinuxX64).is_none());
    }

    #[test]
    fn paths_that_leave_the_folder_are_refused() {
        for (key, value) in [("version", "../x"), ("root", "a/../b"), ("check", "/etc/passwd"), ("check", "a\\b")] {
            let mut r = runtime();
            r[key] = value.into();
            assert!(parse(r).is_err(), "{key} = {value}");
        }
    }

    #[test]
    fn plain_http_and_bad_hashes_are_refused() {
        let mut r = runtime();
        r["urls"] = serde_json::json!(["http://example.org/a.cab"]);
        assert!(parse(r).is_err());
        let mut r = runtime();
        r["sha256"] = "AB".repeat(32).into();
        assert!(parse(r).is_err());
        let mut r = runtime();
        r["size"] = 0.into();
        assert!(parse(r).is_err());
    }

    #[test]
    fn another_format_is_refused() {
        assert!(Manifest::parse(r#"{"format":2,"runtimes":[]}"#).is_err());
    }
}
