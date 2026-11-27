//! Unpacking a checked archive into a fresh folder.

use std::fs::File;
use std::path::Path;

use crate::manifest::Archive;
use crate::Error;

/// Unpacks `archive` (already checked against the manifest) into `into`, which must be empty.
pub fn unpack(kind: Archive, archive: &Path, into: &Path) -> Result<(), Error> {
    std::fs::create_dir_all(into)?;
    match kind {
        Archive::TarGz => tar_gz(archive, into),
        Archive::Cab => cab(archive, into),
    }
}

/// tar's `unpack` refuses entries that leave the folder: absolute paths, `..`, and writing through
/// a symlink that points out. The bundle's own symlinks (`libfoo.so.1 -> libfoo.so.1.2`) stay.
fn tar_gz(archive: &Path, into: &Path) -> Result<(), Error> {
    let gz = flate2::read::GzDecoder::new(File::open(archive)?);
    let mut tar = tar::Archive::new(gz);
    tar.set_preserve_permissions(true);
    tar.set_overwrite(false);
    tar.unpack(into).map_err(|e| Error::Unpack(e.to_string()))
}

/// Microsoft's cabinet, through the `expand` that every Windows since XP carries in System32.
/// Called by full path, so a program named expand elsewhere on PATH is never the one run.
#[cfg(windows)]
fn cab(archive: &Path, into: &Path) -> Result<(), Error> {
    let system = std::env::var_os("SystemRoot").map(std::path::PathBuf::from).unwrap_or_else(|| "C:\\Windows".into());
    let out = std::process::Command::new(system.join("System32").join("expand.exe"))
        .arg(archive)
        .arg("-F:*")
        .arg(into)
        .output()?;
    if !out.status.success() {
        let text = String::from_utf8_lossy(&out.stdout).into_owned() + &String::from_utf8_lossy(&out.stderr);
        return Err(Error::Unpack(format!("expand.exe exited with {}: {}", out.status, text.trim())));
    }
    Ok(())
}

#[cfg(not(windows))]
fn cab(_: &Path, _: &Path) -> Result<(), Error> {
    Err(Error::Unpack("a cabinet is unpacked only on Windows".into()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("wvr-unpack-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn tar_gz_of(entries: &[(&str, &[u8])], path: &Path) {
        let gz = flate2::write::GzEncoder::new(File::create(path).unwrap(), flate2::Compression::fast());
        let mut b = tar::Builder::new(gz);
        for (name, data) in entries {
            let mut h = tar::Header::new_gnu();
            h.set_size(data.len() as u64);
            h.set_mode(0o755);
            // set_path refuses `..`; the raw name field is what a hostile archive would carry
            let raw = &mut h.as_old_mut().name;
            raw[..name.len()].copy_from_slice(name.as_bytes());
            h.set_cksum();
            b.append(&h, *data).unwrap();
        }
        b.into_inner().unwrap().finish().unwrap();
    }

    #[test]
    fn a_tar_gz_unpacks() {
        let dir = temp("good");
        let archive = dir.join("a.tar.gz");
        tar_gz_of(&[("lib/libwebkit2gtk-4.1.so.0", b"elf"), ("run", b"#!/bin/sh")], &archive);
        unpack(Archive::TarGz, &archive, &dir.join("out")).unwrap();
        assert_eq!(std::fs::read(dir.join("out/lib/libwebkit2gtk-4.1.so.0")).unwrap(), b"elf");
    }

    #[test]
    fn an_entry_that_climbs_out_is_not_written_outside() {
        let dir = temp("climb");
        let archive = dir.join("a.tar.gz");
        tar_gz_of(&[("../escaped", b"x")], &archive);
        let _ = unpack(Archive::TarGz, &archive, &dir.join("out"));
        assert!(!dir.join("escaped").exists());
    }
}
