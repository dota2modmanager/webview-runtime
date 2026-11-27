//! A Microsoft cabinet reader that decompresses each folder once, front to back.
//!
//! Microsoft's WebView2 Fixed Version cab is one MSZIP folder of 438 MB holding 258 files. A
//! reader that opens each file on its own (the `cab` crate does) decompresses the folder from its
//! start for every file: the installer sat in "unpacking" for minutes. Here the data blocks are
//! read once, in order, and each decompressed byte goes to the file whose range it falls in.
//!
//! Format: [MS-CAB]. Supported: one cabinet (no spanning), folders stored or MSZIP. MSZIP blocks
//! are raw deflate streams of at most 32 KiB after a "CK" mark, each allowed to refer back into
//! the 32 KiB before it, so the last 32 KiB of output is kept as the history of the next block.
//! Checksums are not checked: the whole file's SHA-256 is, before this runs.

use std::fs::File;
use std::io::{BufReader, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use miniz_oxide::inflate::core::{decompress, inflate_flags, DecompressorOxide};
use miniz_oxide::inflate::TINFLStatus;

const HISTORY: usize = 32 * 1024;
const BLOCK: usize = 32 * 1024;

struct Folder {
    data_start: u32,
    blocks: u16,
    compression: u16,
}

struct Entry {
    name: String,
    size: u32,
    offset: u32,
    folder: u16,
}

fn bad(why: impl Into<String>) -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::InvalidData, why.into())
}

fn u8_(r: &mut impl Read) -> std::io::Result<u8> {
    let mut b = [0u8; 1];
    r.read_exact(&mut b)?;
    Ok(b[0])
}

fn u16_(r: &mut impl Read) -> std::io::Result<u16> {
    let mut b = [0u8; 2];
    r.read_exact(&mut b)?;
    Ok(u16::from_le_bytes(b))
}

fn u32_(r: &mut impl Read) -> std::io::Result<u32> {
    let mut b = [0u8; 4];
    r.read_exact(&mut b)?;
    Ok(u32::from_le_bytes(b))
}

fn skip(r: &mut impl Seek, n: i64) -> std::io::Result<()> {
    r.seek(SeekFrom::Current(n)).map(|_| ())
}

fn cstring(r: &mut impl Read) -> std::io::Result<String> {
    let mut out = Vec::new();
    loop {
        match u8_(r)? {
            0 => break,
            b => out.push(b),
        }
        if out.len() > 1024 {
            return Err(bad("a file name longer than 1024 bytes"));
        }
    }
    Ok(String::from_utf8_lossy(&out).into_owned())
}

/// Unpacks every file of the cabinet at `path` into `into`. `place` turns a name inside the
/// cabinet into a path under `into`, or refuses it.
pub fn extract(path: &Path, into: &Path, place: impl Fn(&str) -> Option<PathBuf>) -> std::io::Result<()> {
    let mut r = BufReader::with_capacity(1 << 20, File::open(path)?);
    let mut sig = [0u8; 4];
    r.read_exact(&mut sig)?;
    if &sig != b"MSCF" {
        return Err(bad("not a cabinet"));
    }
    skip(&mut r, 4)?;
    let _size = u32_(&mut r)?;
    skip(&mut r, 4)?;
    let files_at = u32_(&mut r)?;
    skip(&mut r, 4 + 2)?; // reserved, version
    let folders_n = u16_(&mut r)?;
    let files_n = u16_(&mut r)?;
    let flags = u16_(&mut r)?;
    skip(&mut r, 4)?; // set id, cabinet index
    if flags & 0x3 != 0 {
        return Err(bad("a cabinet spanning several files"));
    }
    let (mut folder_reserve, mut data_reserve) = (0u8, 0u8);
    if flags & 0x4 != 0 {
        let header_reserve = u16_(&mut r)?;
        folder_reserve = u8_(&mut r)?;
        data_reserve = u8_(&mut r)?;
        skip(&mut r, header_reserve as i64)?;
    }
    let mut folders = Vec::with_capacity(folders_n as usize);
    for _ in 0..folders_n {
        let data_start = u32_(&mut r)?;
        let blocks = u16_(&mut r)?;
        let compression = u16_(&mut r)? & 0xF;
        skip(&mut r, folder_reserve as i64)?;
        if compression > 1 {
            return Err(bad(format!("compression {compression} (only stored and MSZIP are read)")));
        }
        folders.push(Folder { data_start, blocks, compression });
    }
    r.seek(SeekFrom::Start(files_at as u64))?;
    let mut entries = Vec::with_capacity(files_n as usize);
    for _ in 0..files_n {
        let size = u32_(&mut r)?;
        let offset = u32_(&mut r)?;
        let folder = u16_(&mut r)?;
        skip(&mut r, 6)?; // date, time, attributes
        let name = cstring(&mut r)?;
        if folder as usize >= folders.len() {
            return Err(bad(format!("{name}: in a folder the cabinet does not have")));
        }
        entries.push(Entry { name, size, offset, folder });
    }

    for (index, folder) in folders.iter().enumerate() {
        let mut mine: Vec<&Entry> = entries.iter().filter(|e| e.folder as usize == index).collect();
        mine.sort_by_key(|e| e.offset);
        let mut targets = Vec::with_capacity(mine.len());
        for e in &mine {
            let rel = place(&e.name).ok_or_else(|| bad(format!("{} leaves the folder", e.name)))?;
            targets.push(into.join(rel));
        }
        let mut sink = Sink::new(&mine, targets)?;
        r.seek(SeekFrom::Start(folder.data_start as u64))?;
        let mut history: Vec<u8> = Vec::with_capacity(HISTORY);
        let mut out = vec![0u8; HISTORY + BLOCK];
        let mut packed = Vec::with_capacity(BLOCK + 6144);
        for _ in 0..folder.blocks {
            skip(&mut r, 4)?; // checksum
            let packed_len = u16_(&mut r)? as usize;
            let plain_len = u16_(&mut r)? as usize;
            skip(&mut r, data_reserve as i64)?;
            packed.resize(packed_len, 0);
            r.read_exact(&mut packed)?;
            if folder.compression == 0 {
                sink.write(&packed)?;
                continue;
            }
            if packed.len() < 2 || &packed[..2] != b"CK" {
                return Err(bad("an MSZIP block without its CK mark"));
            }
            // the history first, then this block, which may copy from it
            let h = history.len();
            out[..h].copy_from_slice(&history);
            let mut state = DecompressorOxide::new();
            let flags = inflate_flags::TINFL_FLAG_USING_NON_WRAPPING_OUTPUT_BUF;
            let (status, _, written) = decompress(&mut state, &packed[2..], &mut out[..h + BLOCK], h, flags);
            if status != TINFLStatus::Done || written != plain_len {
                return Err(bad(format!("an MSZIP block did not inflate ({status:?}, {written} of {plain_len} bytes)")));
            }
            sink.write(&out[h..h + written])?;
            let end = h + written;
            history.clear();
            history.extend_from_slice(&out[end.saturating_sub(HISTORY)..end]);
        }
        sink.finish()?;
    }
    Ok(())
}

/// Routes a folder's decompressed stream into its files, which are sorted by offset.
struct Sink {
    files: Vec<(u64, u64, PathBuf)>,
    current: usize,
    open: Option<File>,
    at: u64,
}

impl Sink {
    fn new(entries: &[&Entry], paths: Vec<PathBuf>) -> std::io::Result<Sink> {
        let files = entries.iter().zip(paths).map(|(e, p)| (e.offset as u64, e.size as u64, p)).collect();
        let mut s = Sink { files, current: 0, open: None, at: 0 };
        s.open_next_empty()?;
        Ok(s)
    }

    /// Files of size 0 are created as soon as the stream reaches them.
    fn open_next_empty(&mut self) -> std::io::Result<()> {
        while let Some((start, size, path)) = self.files.get(self.current) {
            if self.open.is_some() || self.at < *start {
                break;
            }
            if let Some(dir) = path.parent() {
                std::fs::create_dir_all(dir)?;
            }
            let f = File::create(path)?;
            if *size == 0 {
                self.current += 1;
                continue;
            }
            self.open = Some(f);
        }
        Ok(())
    }

    fn write(&mut self, mut data: &[u8]) -> std::io::Result<()> {
        while !data.is_empty() {
            self.open_next_empty()?;
            let Some((start, size, _)) = self.files.get(self.current).map(|(a, b, c)| (*a, *b, c.clone())) else {
                // bytes past the last file belong to nothing
                self.at += data.len() as u64;
                return Ok(());
            };
            if self.at < start {
                let gap = ((start - self.at) as usize).min(data.len());
                self.at += gap as u64;
                data = &data[gap..];
                continue;
            }
            let left = (start + size - self.at) as usize;
            let n = left.min(data.len());
            self.open.as_mut().expect("the file in range is open").write_all(&data[..n])?;
            self.at += n as u64;
            data = &data[n..];
            if n == left {
                self.open = None;
                self.current += 1;
            }
        }
        Ok(())
    }

    fn finish(mut self) -> std::io::Result<()> {
        self.open_next_empty()?;
        if self.current < self.files.len() {
            return Err(bad(format!("the folder ended inside {}", self.files[self.current].2.display())));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("wvr-cab-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// Bytes that compress, yet differ enough that back-references cross block borders.
    fn data(n: usize, seed: u32) -> Vec<u8> {
        let mut x = seed;
        (0..n)
            .map(|i| {
                x = x.wrapping_mul(1_103_515_245).wrapping_add(12345);
                if i % 7 == 0 { (x >> 16) as u8 } else { b"WebView2 Fixed Version "[i % 23] }
            })
            .collect()
    }

    fn build(path: &Path, compression: cab::CompressionType, files: &[(&str, Vec<u8>)]) {
        let mut b = cab::CabinetBuilder::new();
        {
            let folder = b.add_folder(compression);
            for (name, _) in files {
                folder.add_file(*name);
            }
        }
        let mut w = b.build(File::create(path).unwrap()).unwrap();
        let mut i = 0;
        while let Some(mut fw) = w.next_file().unwrap() {
            fw.write_all(&files[i].1).unwrap();
            i += 1;
        }
        w.finish().unwrap();
    }

    fn plain(name: &str) -> Option<PathBuf> {
        Some(name.split('\\').collect())
    }

    #[test]
    fn mszip_files_across_many_blocks_come_out_whole() {
        let dir = temp("mszip");
        let files = vec![
            ("a\\first.dll", data(100_000, 1)),
            ("empty.txt", Vec::new()),
            ("a\\b\\second.bin", data(250_000, 2)),
            ("tiny", b"x".to_vec()),
            ("last.pak", data(70_000, 3)),
        ];
        let cab = dir.join("t.cab");
        build(&cab, cab::CompressionType::MsZip, &files);
        extract(&cab, &dir.join("out"), plain).unwrap();
        for (name, bytes) in &files {
            let got = std::fs::read(dir.join("out").join(plain(name).unwrap())).unwrap();
            assert_eq!(&got, bytes, "{name}");
        }
    }

    #[test]
    fn stored_files_come_out_whole() {
        let dir = temp("stored");
        let files = vec![("one", data(40_000, 4)), ("two", data(9, 5))];
        let cab = dir.join("t.cab");
        build(&cab, cab::CompressionType::None, &files);
        extract(&cab, &dir.join("out"), plain).unwrap();
        assert_eq!(std::fs::read(dir.join("out/one")).unwrap(), files[0].1);
        assert_eq!(std::fs::read(dir.join("out/two")).unwrap(), files[1].1);
    }

    #[test]
    fn a_name_the_caller_refuses_stops_it() {
        let dir = temp("refuse");
        let cab = dir.join("t.cab");
        build(&cab, cab::CompressionType::MsZip, &[("..\\x", b"no".to_vec())]);
        assert!(extract(&cab, &dir.join("out"), |_| None).is_err());
        assert!(!dir.join("x").exists());
    }
}
