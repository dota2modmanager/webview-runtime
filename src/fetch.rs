//! Downloading an engine archive: every byte counted and hashed on the way to disk, so a file
//! that is not exactly the one in the manifest never reaches the unpacker.

use std::fs::File;
use std::io::{Read, Write};
use std::path::Path;

use sha2::{Digest, Sha256};

/// Set from another thread (the installer's Cancel button) to stop a download at the next block.
pub static CANCEL: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

use crate::{Error, Runtime};

/// Copies `from` into `to`, refusing anything longer than `size` and checking length and SHA-256
/// at the end. `progress` gets (bytes so far, size).
pub fn copy_checked(
    mut from: impl Read,
    to: &mut impl Write,
    size: u64,
    sha256: &str,
    progress: &mut dyn FnMut(u64, u64),
) -> Result<(), Error> {
    let mut hash = Sha256::new();
    let mut done = 0u64;
    let mut buf = vec![0u8; 1 << 16];
    loop {
        if CANCEL.load(std::sync::atomic::Ordering::Relaxed) {
            return Err(Error::Cancelled);
        }
        let n = from.read(&mut buf).map_err(|e| Error::Download(e.to_string()))?;
        if n == 0 {
            break;
        }
        done += n as u64;
        if done > size {
            return Err(Error::Mismatch { what: "size", want: size.to_string(), got: format!("more than {size}") });
        }
        hash.update(&buf[..n]);
        to.write_all(&buf[..n])?;
        progress(done, size);
    }
    if done != size {
        return Err(Error::Mismatch { what: "size", want: size.to_string(), got: done.to_string() });
    }
    let got = hex::encode(hash.finalize());
    if got != sha256 {
        return Err(Error::Mismatch { what: "sha256", want: sha256.into(), got });
    }
    to.flush()?;
    Ok(())
}

/// Hashes a file already on disk the same way, for an archive a person put there by hand.
pub fn check_file(path: &Path, runtime: &Runtime) -> Result<(), Error> {
    copy_checked(File::open(path)?, &mut std::io::sink(), runtime.size, &runtime.sha256, &mut |_, _| {})
}

/// Fetches `runtime`'s archive into `dest`, trying each url in order. The first good copy wins;
/// the error of the last url is the one returned.
pub fn download(runtime: &Runtime, dest: &Path, progress: &mut dyn FnMut(u64, u64)) -> Result<(), Error> {
    let mut last = Error::Download("no url".into());
    for url in &runtime.urls {
        match download_one(url, runtime, dest, progress) {
            Ok(()) => return Ok(()),
            Err(Error::Cancelled) => {
                let _ = std::fs::remove_file(dest);
                return Err(Error::Cancelled);
            }
            Err(e) => {
                let _ = std::fs::remove_file(dest);
                last = Error::Download(format!("{url}: {e}"));
            }
        }
    }
    Err(last)
}

fn download_one(url: &str, runtime: &Runtime, dest: &Path, progress: &mut dyn FnMut(u64, u64)) -> Result<(), Error> {
    let response = ureq::get(url)
        .header("User-Agent", concat!("webview-runtime/", env!("CARGO_PKG_VERSION")))
        .call()
        .map_err(|e| Error::Download(e.to_string()))?;
    // one byte past the size is enough to tell a longer file
    let body = response.into_body().into_with_config().limit(runtime.size + 1).reader();
    let mut file = File::create(dest)?;
    copy_checked(body, &mut file, runtime.size, &runtime.sha256, progress)?;
    file.sync_all()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sha(data: &[u8]) -> String {
        hex::encode(Sha256::digest(data))
    }

    #[test]
    fn the_right_bytes_pass() {
        let data = b"engine".repeat(50_000);
        let mut out = Vec::new();
        let mut seen = 0;
        copy_checked(&data[..], &mut out, data.len() as u64, &sha(&data), &mut |d, _| seen = d).unwrap();
        assert_eq!(out, data);
        assert_eq!(seen, data.len() as u64);
    }

    #[test]
    fn a_changed_byte_fails() {
        let data = b"engine".to_vec();
        let mut other = data.clone();
        other[0] = b'E';
        let err = copy_checked(&other[..], &mut Vec::new(), 6, &sha(&data), &mut |_, _| {}).unwrap_err();
        assert!(matches!(err, Error::Mismatch { what: "sha256", .. }));
    }

    #[test]
    fn short_and_long_files_fail() {
        let data = b"engine";
        let short = copy_checked(&data[..5], &mut Vec::new(), 6, &sha(data), &mut |_, _| {});
        assert!(matches!(short, Err(Error::Mismatch { what: "size", .. })));
        let long = copy_checked(&b"engine!"[..], &mut Vec::new(), 6, &sha(data), &mut |_, _| {});
        assert!(matches!(long, Err(Error::Mismatch { what: "size", .. })));
    }
}
