//! A WebView engine of the app's own, for a machine whose own one is missing or broken.
//!
//! The window of Dota 2 Mod Manager is a web page, drawn by the engine the system has: WebView2
//! on Windows, WebKitGTK on Linux. When that engine is missing, half-removed ("debloat" scripts,
//! Edge removers), or on Linux built against libraries that do not match the GPU driver, the app
//! cannot open its window at all. This crate gives the app a second engine that does not depend on
//! the system:
//!
//! * Windows: Microsoft's WebView2 **Fixed Version** runtime, unpacked into the app's data folder
//!   and used through `WEBVIEW2_BROWSER_EXECUTABLE_FOLDER`. Nothing is installed system-wide, no
//!   EdgeUpdate, no registry.
//! * Linux: WebKitGTK 4.1 with everything it links (GTK, GLib, GStreamer, ICU, Mesa) and glibc
//!   itself, built on one base system (`linux/build.sh`) and run as a whole. The host's glibc is
//!   used where it is new enough; on musl (Alpine, Void), NixOS, or an old glibc the bundle runs
//!   through the loader it carries.
//!
//! The archives are pinned in [`MANIFEST`] by size and SHA-256, so the manifest the app was built
//! with decides what may be run; a download that differs is thrown away.
//!
//! [`choose`] says which engine a start should use, [`install`] fetches the app's own, [`env_for`]
//! is what to start the window with, and [`Health`] notices a start that never reached its page.

pub mod cabfile;
pub mod fetch;
pub mod health;
pub mod manifest;
pub mod store;
pub mod system;
pub mod unpack;

use std::path::{Path, PathBuf};

pub use health::Health;
pub use manifest::{Manifest, Platform, Runtime};
pub use store::Installed;

/// The manifest this build trusts: the archives, their sizes and hashes. Release CI rewrites it
/// (`.github/workflows/release.yml`); the app embeds it, so a new engine comes with a new app.
pub const MANIFEST: &str = include_str!("../runtime.json");

/// Asks for the app's own engine whatever the system has: `1` (own), `0` (system's only).
pub const FORCE_VAR: &str = "D2MM_OWN_WEBVIEW";

/// Which engine a start uses, and why: the why goes into the app's log.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Choice {
    /// The system's engine is there and the last start reached its page.
    System,
    /// The app's own engine, already unpacked here.
    Own { dir: PathBuf, why: Why },
    /// The app's own engine is needed and not here yet: [`install`] it, then start with it.
    InstallOwn { why: Why },
    /// The system has none, and no engine of the app's own is known for this platform.
    Nothing,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Why {
    /// No system engine was found.
    NoSystem,
    /// The system engine is there, but the start before this one never got its page up.
    SystemFailed,
    /// [`FORCE_VAR`] or the app's setting asked for it.
    Asked,
}

/// What decides a start. Kept apart from the machine so [`decide`] can be tested.
#[derive(Debug, Clone, Default)]
pub struct Facts {
    pub system_ok: bool,
    pub last_start_failed: bool,
    /// `Some(true)` own engine asked for, `Some(false)` the system's only.
    pub forced: Option<bool>,
    pub own_installed: Option<PathBuf>,
    pub own_known: bool,
}

/// The rule, on its own: the system's engine while it works, ours when it is missing or did not
/// get the last start's window up. A failed start on our own engine does not switch back: the
/// marker is cleared only by a start that reaches its page.
pub fn decide(f: &Facts) -> Choice {
    let why = match f.forced {
        Some(false) => return if f.system_ok { Choice::System } else { Choice::Nothing },
        Some(true) => Why::Asked,
        None if !f.system_ok => Why::NoSystem,
        None if f.last_start_failed => Why::SystemFailed,
        None => return Choice::System,
    };
    match (&f.own_installed, f.own_known) {
        (Some(dir), _) => Choice::Own { dir: dir.clone(), why },
        (None, true) => Choice::InstallOwn { why },
        (None, false) if f.system_ok => Choice::System,
        (None, false) => Choice::Nothing,
    }
}

/// [`decide`] on this machine. `base` is where the app keeps its own engines (its data folder's
/// `webview-runtime`), `forced` the app's setting, which [`FORCE_VAR`] overrides.
pub fn choose(base: &Path, health: &Health, forced: Option<bool>) -> Choice {
    let manifest = Manifest::parse(MANIFEST).ok();
    let runtime = manifest.as_ref().and_then(|m| m.for_this_platform());
    let forced = match std::env::var(FORCE_VAR).as_deref() {
        Ok("1") => Some(true),
        Ok("0") => Some(false),
        _ => forced,
    };
    decide(&Facts {
        system_ok: system::engine_present(),
        last_start_failed: health.last_start_failed(),
        forced,
        own_installed: runtime.and_then(|r| store::find(base, r)).map(|i| i.dir),
        own_known: runtime.is_some(),
    })
}

/// Fetches and unpacks this platform's engine from [`MANIFEST`] into `base`.
pub fn install(base: &Path, progress: &mut dyn FnMut(u64, u64)) -> Result<Installed, Error> {
    let manifest = Manifest::parse(MANIFEST)?;
    let runtime = manifest.for_this_platform().ok_or(Error::NoRuntime)?;
    if let Some(have) = store::find(base, runtime) {
        return Ok(have);
    }
    store::install(base, runtime, progress)
}

/// The environment to start the window's process with on the app's own engine, and on Linux the
/// working folder it needs (see `linux/build.sh`: WebKit finds its helper processes relative to
/// it; the caller keeps its own working folder in `D2MM_OWD`). `software` draws without the GPU,
/// for when our own engine failed too ([`Health::software_rendering`]).
pub fn env_for(installed: &Installed, software: bool) -> Launch {
    match installed.runtime.platform {
        Platform::WindowsX64 => {
            let mut vars = vec![("WEBVIEW2_BROWSER_EXECUTABLE_FOLDER".into(), installed.dir.display().to_string())];
            if software {
                vars.push(("WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS".into(), "--disable-gpu".into()));
            }
            Launch { vars, cwd: None, wrap: Vec::new() }
        }
        Platform::LinuxX64 => {
            let own_glibc = system::needs_own_glibc(&system::host(), &installed.runtime.glibc);
            store::linux_launch(&installed.dir, software, own_glibc)
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Launch {
    pub vars: Vec<(String, String)>,
    pub cwd: Option<PathBuf>,
    /// What goes before the program on its command line: the bundle's glibc loader and its
    /// library path when the host's glibc cannot run the bundle (musl, NixOS, too old). The
    /// program then sees the loader as /proc/self/exe; `D2MM_EXE` names the program itself.
    pub wrap: Vec<String>,
}

impl Launch {
    /// The command that starts `program` with `args` this way.
    pub fn command<A: AsRef<std::ffi::OsStr>>(&self, program: &std::ffi::OsStr, args: impl IntoIterator<Item = A>) -> std::process::Command {
        let mut cmd = match self.wrap.split_first() {
            None => std::process::Command::new(program),
            Some((loader, rest)) => {
                let mut c = std::process::Command::new(loader);
                c.args(rest).arg("--argv0").arg(program).arg(program);
                c.env("D2MM_EXE", program);
                c
            }
        };
        cmd.args(args).envs(self.vars.iter().map(|(k, v)| (k, v)));
        if let Some(cwd) = &self.cwd {
            if let Ok(owd) = std::env::current_dir() {
                cmd.env("D2MM_OWD", owd);
            }
            cmd.current_dir(cwd);
        }
        cmd
    }
}

#[derive(Debug)]
pub enum Error {
    NoRuntime,
    Manifest(String),
    Download(String),
    Mismatch { what: &'static str, want: String, got: String },
    Unpack(String),
    Cancelled,
    Io(std::io::Error),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::NoRuntime => write!(f, "no WebView engine of our own is known for this platform"),
            Error::Manifest(e) => write!(f, "runtime.json: {e}"),
            Error::Download(e) => write!(f, "download failed: {e}"),
            Error::Mismatch { what, want, got } => write!(f, "{what} differs: expected {want}, got {got}"),
            Error::Unpack(e) => write!(f, "unpacking failed: {e}"),
            Error::Cancelled => write!(f, "cancelled"),
            Error::Io(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for Error {}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Error::Io(e)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn facts() -> Facts {
        Facts { system_ok: true, own_known: true, ..Default::default() }
    }

    #[test]
    fn a_working_system_engine_is_used() {
        assert_eq!(decide(&facts()), Choice::System);
    }

    #[test]
    fn no_system_engine_installs_ours() {
        let f = Facts { system_ok: false, ..facts() };
        assert_eq!(decide(&f), Choice::InstallOwn { why: Why::NoSystem });
    }

    #[test]
    fn a_start_that_never_reached_its_page_switches_to_ours() {
        let dir = PathBuf::from("own");
        let f = Facts { last_start_failed: true, own_installed: Some(dir.clone()), ..facts() };
        assert_eq!(decide(&f), Choice::Own { dir, why: Why::SystemFailed });
    }

    #[test]
    fn asked_for_the_system_one_never_installs() {
        let f = Facts { system_ok: false, forced: Some(false), ..facts() };
        assert_eq!(decide(&f), Choice::Nothing);
        let f = Facts { last_start_failed: true, forced: Some(false), ..facts() };
        assert_eq!(decide(&f), Choice::System);
    }

    #[test]
    fn asked_for_ours_uses_ours_even_with_a_working_system() {
        assert_eq!(decide(&Facts { forced: Some(true), ..facts() }), Choice::InstallOwn { why: Why::Asked });
    }

    #[test]
    fn without_a_known_engine_the_system_one_still_runs() {
        let f = Facts { last_start_failed: true, own_known: false, ..facts() };
        assert_eq!(decide(&f), Choice::System);
        assert_eq!(decide(&Facts { system_ok: false, ..f }), Choice::Nothing);
    }

    #[test]
    fn the_embedded_manifest_parses() {
        Manifest::parse(MANIFEST).unwrap();
    }
}
