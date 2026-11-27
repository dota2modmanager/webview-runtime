//! Noticing a start whose window never came up.
//!
//! A broken engine often does not fail with an error the app can catch: WebView2 hangs on a
//! half-removed runtime, WebKitGTK dies with a segfault in the GPU driver. So the app counts
//! instead. [`Health::begin`] counts a start as failed before the window is made; the page's
//! first message calls [`Health::ready`], which clears it. A start that dies in between leaves
//! the count up, and the next start sees it.

use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct Health {
    file: PathBuf,
    before: u32,
}

/// After this many failed starts in a row on our own engine, it runs on the software renderer.
pub const SOFTWARE_AFTER: u32 = 2;

impl Health {
    /// Reads the count the previous starts left in `file`.
    pub fn open(file: &Path) -> Health {
        let before = std::fs::read_to_string(file).ok().and_then(|t| t.trim().parse().ok()).unwrap_or(0);
        Health { file: file.to_path_buf(), before }
    }

    /// Starts that failed in a row before this one.
    pub fn failed_starts(&self) -> u32 {
        self.before
    }

    pub fn last_start_failed(&self) -> bool {
        self.before > 0
    }

    /// Whether the window should be drawn without the GPU: our own engine failed too.
    pub fn software_rendering(&self) -> bool {
        self.before >= SOFTWARE_AFTER
    }

    /// Counts this start as failed until [`Health::ready`] says otherwise.
    pub fn begin(&self) -> std::io::Result<()> {
        self.write(self.before.saturating_add(1))
    }

    /// The window's page is up: this start did not fail.
    pub fn ready(&self) -> std::io::Result<()> {
        self.write(0)
    }

    fn write(&self, n: u32) -> std::io::Result<()> {
        if let Some(dir) = self.file.parent() {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::write(&self.file, n.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_start_that_dies_before_its_page_is_seen_by_the_next() {
        let file = std::env::temp_dir().join(format!("wvr-health-{}", std::process::id()));
        let _ = std::fs::remove_file(&file);
        let first = Health::open(&file);
        assert!(!first.last_start_failed());
        first.begin().unwrap();
        // no ready(): the window never came up
        let second = Health::open(&file);
        assert!(second.last_start_failed());
        assert!(!second.software_rendering());
        second.begin().unwrap();
        let third = Health::open(&file);
        assert!(third.software_rendering());
        third.begin().unwrap();
        third.ready().unwrap();
        assert!(!Health::open(&file).last_start_failed());
        let _ = std::fs::remove_file(&file);
    }
}
