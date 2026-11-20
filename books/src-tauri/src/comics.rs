//! Comics packed some other way than ZIP: CBT (a tar) and CB7 (a 7z). Their pictures are taken out here and handed to
//! the page as a plain, uncompressed ZIP, so the page reads every comic the one way it reads a CBZ. The file itself is
//! never changed: Save As copies the original bytes.
//!
//! As with a CBZ (src/readers.js, unzipBounded), a comic may unpack to at most 8 times its file (256 MB for any small
//! one, never over 2 GB); past that it isn't opened. A 7z with a password isn't supported, as a CBZ with one isn't.
//! CBR (RAR) and CBA (ACE) aren't read at all.

use std::io::{Cursor, Read, Write};

#[derive(Debug, PartialEq)]
pub enum Problem {
    /// Not a comic of this kind, cut short, locked, or larger than it may be.
    Damaged,
}

/// How much a comic of this many bytes may unpack to.
pub fn budget(file: usize) -> u64 {
    ((file as u64).saturating_mul(8)).clamp(256 << 20, 2 << 30)
}

/// A page of a comic: a picture, and not one of the Mac's or a hidden file's leftovers. The same test as `readCbz` in the page.
pub fn is_page(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    let picture = [".jpg", ".jpeg", ".png", ".webp", ".gif", ".bmp"].iter().any(|e| lower.ends_with(e));
    let hidden = lower.split(['/', '\\']).any(|part| part == "__macosx" || part.starts_with('.'));
    picture && !hidden
}

/// The pages, one after another, into a ZIP that only stores them.
fn store(pages: Vec<(String, Vec<u8>)>) -> Result<Vec<u8>, Problem> {
    if pages.is_empty() {
        return Err(Problem::Damaged);
    }
    let mut z = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let o =
        zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored).large_file(false);
    for (name, data) in pages {
        z.start_file(name, o).map_err(|_| Problem::Damaged)?;
        z.write_all(&data).map_err(|_| Problem::Damaged)?;
    }
    Ok(z.finish().map_err(|_| Problem::Damaged)?.into_inner())
}

// ---------- tar ----------

fn octal(field: &[u8]) -> Option<u64> {
    // GNU's base-256 for sizes past 8 GB
    if field.first().is_some_and(|b| b & 0x80 != 0) {
        return Some(field[1..].iter().fold(0u64, |v, b| v.saturating_mul(256).saturating_add(*b as u64)));
    }
    let text = std::str::from_utf8(field).ok()?.trim_matches(|c: char| c == '\0' || c == ' ');
    if text.is_empty() {
        return Some(0);
    }
    u64::from_str_radix(text, 8).ok()
}

fn cstr(field: &[u8]) -> String {
    let end = field.iter().position(|&b| b == 0).unwrap_or(field.len());
    String::from_utf8_lossy(&field[..end]).into_owned()
}

/// A tar's own check: the header's bytes added up, with the checksum field read as spaces.
fn header_ok(h: &[u8]) -> bool {
    let Some(want) = octal(&h[148..156]) else { return false };
    let sum: u64 = h.iter().enumerate().map(|(i, &b)| if (148..156).contains(&i) { 32 } else { b as u64 }).sum();
    sum == want
}

/// The pictures in a tar (POSIX ustar, with GNU long names and pax paths), in the file's order.
pub fn tar_pages(b: &[u8]) -> Result<Vec<u8>, Problem> {
    let limit = budget(b.len());
    let (mut at, mut total, mut pages) = (0usize, 0u64, Vec::new());
    let (mut long_name, mut pax_path): (Option<String>, Option<String>) = (None, None);
    let mut any = false;
    while at + 512 <= b.len() {
        let h = &b[at..at + 512];
        // two empty blocks end it; one is enough to stop
        if h.iter().all(|&x| x == 0) {
            break;
        }
        if !header_ok(h) {
            return Err(Problem::Damaged);
        }
        any = true;
        let size = octal(&h[124..136]).ok_or(Problem::Damaged)?;
        let data = at + 512;
        let end = data.checked_add(usize::try_from(size).map_err(|_| Problem::Damaged)?).ok_or(Problem::Damaged)?;
        if end > b.len() {
            return Err(Problem::Damaged);
        }
        let body = &b[data..end];
        match h[156] {
            b'L' => long_name = Some(cstr(body)),
            b'x' => {
                // pax records: "<length> path=<name>\n"
                for rec in String::from_utf8_lossy(body).split('\n') {
                    if let Some((_, kv)) = rec.split_once(' ') {
                        if let Some(p) = kv.strip_prefix("path=") {
                            pax_path = Some(p.to_string());
                        }
                    }
                }
            }
            b'0' | 0 | b'7' => {
                let name = pax_path.take().or_else(|| long_name.take()).unwrap_or_else(|| {
                    let prefix = if &h[257..262] == b"ustar" { cstr(&h[345..500]) } else { String::new() };
                    let base = cstr(&h[0..100]);
                    if prefix.is_empty() { base } else { format!("{prefix}/{base}") }
                });
                if is_page(&name) {
                    total += size;
                    if total > limit {
                        return Err(Problem::Damaged);
                    }
                    pages.push((name, body.to_vec()));
                }
            }
            _ => {
                long_name = None;
                pax_path = None;
            }
        }
        at = data + (size as usize).div_ceil(512) * 512;
    }
    if !any {
        return Err(Problem::Damaged);
    }
    store(pages)
}

// ---------- 7z ----------

/// The pictures in a 7z, in the archive's order. Solid archives are unpacked from the start, as 7z has to.
pub fn sevenz_pages(b: &[u8]) -> Result<Vec<u8>, Problem> {
    let limit = budget(b.len());
    let mut r = sevenz_rust2::ArchiveReader::new(Cursor::new(b), sevenz_rust2::Password::empty())
        .map_err(|_| Problem::Damaged)?;
    let (mut total, mut pages, mut over) = (0u64, Vec::new(), false);
    r.for_each_entries(|entry, reader| {
        if entry.is_directory() || !entry.has_stream() || !is_page(entry.name()) {
            // what isn't a page is still read past, as a solid block must be
            std::io::copy(reader, &mut std::io::sink())?;
            return Ok(true);
        }
        let room = limit.saturating_sub(total);
        let mut data = Vec::new();
        reader.take(room + 1).read_to_end(&mut data)?;
        total += data.len() as u64;
        if total > limit {
            over = true;
            return Ok(false);
        }
        pages.push((entry.name().to_string(), data));
        Ok(true)
    })
    .map_err(|_| Problem::Damaged)?;
    if over {
        return Err(Problem::Damaged);
    }
    store(pages)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tar_entry(name: &str, data: &[u8], kind: u8) -> Vec<u8> {
        let mut h = vec![0u8; 512];
        h[..name.len()].copy_from_slice(name.as_bytes());
        h[100..107].copy_from_slice(b"0000644");
        h[124..135].copy_from_slice(format!("{:011o}", data.len()).as_bytes());
        h[136..147].copy_from_slice(b"00000000000");
        h[156] = kind;
        h[257..263].copy_from_slice(b"ustar\0");
        h[263..265].copy_from_slice(b"00");
        let sum: u32 = h.iter().enumerate().map(|(i, &b)| if (148..156).contains(&i) { 32 } else { b as u32 }).sum();
        h[148..155].copy_from_slice(format!("{sum:06o}\0").as_bytes());
        h[155] = b' ';
        let mut out = h;
        out.extend_from_slice(data);
        out.resize(out.len().div_ceil(512) * 512, 0);
        out
    }
    fn names(zip_bytes: &[u8]) -> Vec<(String, Vec<u8>)> {
        let mut z = zip::ZipArchive::new(Cursor::new(zip_bytes)).unwrap();
        (0..z.len())
            .map(|i| {
                let mut f = z.by_index(i).unwrap();
                assert_eq!(f.compression(), zip::CompressionMethod::Stored);
                let mut v = Vec::new();
                f.read_to_end(&mut v).unwrap();
                (f.name().to_string(), v)
            })
            .collect()
    }

    #[test]
    fn a_tar_gives_its_pictures_and_nothing_else() {
        let mut t = Vec::new();
        t.extend(tar_entry("comic/", b"", b'5'));
        t.extend(tar_entry("comic/02.png", b"two", b'0'));
        t.extend(tar_entry("comic/01.jpg", &[7u8; 700], b'0'));
        t.extend(tar_entry("comic/ComicInfo.xml", b"<x/>", b'0'));
        t.extend(tar_entry("comic/.thumb.jpg", b"no", b'0'));
        // a GNU long name for the next file
        let long = format!("comic/{}.webp", "x".repeat(120));
        t.extend(tar_entry("././@LongLink", format!("{long}\0").as_bytes(), b'L'));
        t.extend(tar_entry("short-cut", b"three", b'0'));
        t.extend(vec![0u8; 1024]);
        let got = names(&tar_pages(&t).unwrap());
        assert_eq!(
            got.iter().map(|g| g.0.as_str()).collect::<Vec<_>>(),
            ["comic/02.png", "comic/01.jpg", long.as_str()]
        );
        assert_eq!(got[1].1, vec![7u8; 700]);
        assert_eq!(got[2].1, b"three");
    }

    #[test]
    fn a_tar_that_lies_is_refused() {
        let mut t = tar_entry("a.png", b"hello", b'0');
        t[0] = b'b';
        assert_eq!(tar_pages(&t).err(), Some(Problem::Damaged), "checksum");
        let t = tar_entry("a.png", &[1u8; 2000], b'0');
        assert_eq!(tar_pages(&t[..1024]).err(), Some(Problem::Damaged), "cut short");
        assert_eq!(tar_pages(b"not a tar at all").err(), Some(Problem::Damaged));
        let t = tar_entry("notes.txt", b"words", b'0');
        assert_eq!(tar_pages(&t).err(), Some(Problem::Damaged), "no pictures");
    }

    #[test]
    fn the_unpacking_budget() {
        assert_eq!(budget(1000), 256 << 20);
        assert_eq!(budget(100 << 20), 800 << 20);
        assert_eq!(budget(1 << 30), 2 << 30);
    }

    /// A 7z written by sevenz-rust2's own writer (a test-only switch, `compress`): LZMA2, the pages each in turn.
    fn sevenz(files: &[(&str, &[u8])]) -> Vec<u8> {
        let mut w = sevenz_rust2::ArchiveWriter::new(Cursor::new(Vec::new())).unwrap();
        for (name, data) in files {
            let e = sevenz_rust2::ArchiveEntry::new_file(name);
            w.push_archive_entry(e, Some(*data)).unwrap();
        }
        w.finish().unwrap().into_inner()
    }

    #[test]
    fn a_7z_gives_its_pictures() {
        let b = sevenz(&[("01.png", b"png!"), ("notes.txt", b"words words words"), ("02.jpg", &[9u8; 5000])]);
        let got = names(&sevenz_pages(&b).unwrap());
        assert_eq!(got, [("01.png".to_string(), b"png!".to_vec()), ("02.jpg".to_string(), vec![9u8; 5000])]);
        assert_eq!(sevenz_pages(b"7z\xbc\xaf\x27\x1c junk").err(), Some(Problem::Damaged));
        assert_eq!(sevenz_pages(&b[..b.len() / 2]).err(), Some(Problem::Damaged));
        assert_eq!(sevenz_pages(&sevenz(&[("readme.txt", b"no pictures")])).err(), Some(Problem::Damaged));
    }

    /// Writes a small 7z comic from a CBZ comic: its first three pages, written by sevenz-rust2. Run by hand to make it
    /// again: cargo test make_cb7_fixture -- --ignored. It does nothing where there is no CBZ comic to read.
    #[test]
    #[ignore]
    fn make_cb7_fixture() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../test/books");
        let Ok(cbz) = std::fs::read(dir.join("comic.cbz")) else {
            eprintln!("skipped: {} is not here", dir.join("comic.cbz").display());
            return;
        };
        // the first three pages are enough, and keep the file small
        let mut pages = names(&cbz);
        pages.truncate(3);
        let files: Vec<(&str, &[u8])> = pages.iter().map(|(n, d)| (n.as_str(), d.as_slice())).collect();
        std::fs::write(dir.join("comic.cb7"), sevenz(&files)).unwrap();
    }
}
