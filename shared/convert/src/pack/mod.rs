//! Archives: zip, 7z and tar both ways; read only: tar in gzip, bzip2, xz or zstd, one file in any of those, rar, lzh,
//! cpio, ar, deb and rpm. No archive is ever written as RAR.
//! Reading is DotAny Archive's own, shared: `source` is dotany-unpack's (`shared/unpack`), which lists and streams what
//! an archive holds, undoes gzip, bzip2, xz and zstd, and reads RAR, LHA, cpio and ar through libarchive (built there,
//! those readers only). `write`, beside this module, is a copy of Archive's `pack.rs`, which writes a new archive from a
//! list of entries; it lacks what Archive's own copy has since added (names composed on the Mac: a conversion takes its
//! names from the archive it reads, never from a disk, and writes them as that archive has them). A conversion is Archive's "save as another format": every entry, in the source's own order, one pass even
//! through a solid 7z, with its time. Names an old zip keeps in a local code page are read the way Archive's page guesses
//! them.

#![allow(dead_code)]

mod write;

use crate::out::Outputs;
use crate::{Options, Pack, Reason, Step};
use dotany_unpack::source::{self, Entry, Kind, OpenError, Source};
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use write::{Format, Level, Node, PackError};

fn level(p: Pack) -> Level {
    match p {
        Pack::Fast => Level::Fast,
        Pack::Normal => Level::Std,
        Pack::Small => Level::Tight,
    }
}

fn format(target: &str) -> Format {
    match target {
        "7z" => Format::SevenZ,
        "tar" => Format::Tar,
        _ => Format::Zip,
    }
}

fn unb64(s: &str) -> Vec<u8> {
    let val = |c: u8| match c {
        b'A'..=b'Z' => Some(c - b'A'),
        b'a'..=b'z' => Some(c - b'a' + 26),
        b'0'..=b'9' => Some(c - b'0' + 52),
        b'+' => Some(62),
        b'/' => Some(63),
        _ => None,
    };
    let mut out = Vec::new();
    for chunk in s.as_bytes().chunks(4) {
        let v: Vec<u8> = chunk.iter().filter_map(|c| val(*c)).collect();
        let n = v.iter().enumerate().fold(0u32, |a, (i, x)| a | (*x as u32) << (18 - 6 * i));
        for k in 0..v.len().saturating_sub(1) {
            out.push((n >> (16 - 8 * k)) as u8);
        }
    }
    out
}

/// Every name the archive holds without saying its encoding, read in the one encoding that suits them all: the guess
/// Archive's page makes (chardetng over all of them), so Chinese and Japanese zips made on Windows keep their names.
fn names(entries: &[Entry]) -> Vec<String> {
    let raws: Vec<Option<Vec<u8>>> = entries.iter().map(|e| e.r.as_deref().map(unb64)).collect();
    let all: Vec<u8> = raws.iter().flatten().flat_map(|r| r.iter().copied().chain(std::iter::once(b'\n'))).collect();
    let enc = if all.is_empty() {
        encoding_rs::UTF_8
    } else {
        let mut d = chardetng::EncodingDetector::new(chardetng::Iso2022JpDetection::Deny);
        d.feed(&all, true);
        d.guess(None, chardetng::Utf8Detection::Allow)
    };
    entries
        .iter()
        .zip(raws)
        .map(|(e, raw)| {
            e.n.clone()
                .unwrap_or_else(|| raw.map(|r| enc.decode_without_bom_handling(&r).0.into_owned()).unwrap_or_default())
        })
        .collect()
}

/// "YYYY-MM-DD HH:MM:SS" in local time, for ZIP's DOS time, from seconds since 1970, as Archive's page writes it.
fn local_time(u: i64) -> Option<String> {
    #[cfg(windows)]
    {
        use windows_sys::Win32::Foundation::{FILETIME, SYSTEMTIME};
        use windows_sys::Win32::System::Time::{FileTimeToSystemTime, SystemTimeToTzSpecificLocalTime};
        let t = (u as i128 + 11_644_473_600) * 10_000_000;
        if t < 0 {
            return None;
        }
        let ft = FILETIME { dwLowDateTime: t as u32, dwHighDateTime: (t >> 32) as u32 };
        // SAFETY: plain structs passed by pointer to the documented conversions
        unsafe {
            let mut utc: SYSTEMTIME = std::mem::zeroed();
            let mut local: SYSTEMTIME = std::mem::zeroed();
            if FileTimeToSystemTime(&ft, &mut utc) == 0
                || SystemTimeToTzSpecificLocalTime(std::ptr::null(), &utc, &mut local) == 0
            {
                return None;
            }
            Some(format!(
                "{:04}-{:02}-{:02} {:02}:{:02}:{:02}",
                local.wYear, local.wMonth, local.wDay, local.wHour, local.wMinute, local.wSecond
            ))
        }
    }
    #[cfg(target_os = "macos")]
    {
        let t = libc::time_t::try_from(u).ok()?;
        // SAFETY: a plain struct filled by the C library's own conversion, with the system's time zone
        unsafe {
            let mut local: libc::tm = std::mem::zeroed();
            if libc::localtime_r(&t, &mut local).is_null() {
                return None;
            }
            Some(format!(
                "{:04}-{:02}-{:02} {:02}:{:02}:{:02}",
                local.tm_year + 1900,
                local.tm_mon + 1,
                local.tm_mday,
                local.tm_hour,
                local.tm_min,
                local.tm_sec
            ))
        }
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        let _ = u;
        None
    }
}

fn open(p: &Path) -> Result<Source, Reason> {
    match Source::open(p, None) {
        Ok(s) => Ok(s),
        Err(OpenError::Locked) | Err(OpenError::WrongPassword) => Err(Reason::Locked),
        Err(OpenError::Damaged) => Err(Reason::Damaged),
    }
}

pub fn convert(p: &Path, target: &str, opts: &Options, out: &mut Outputs, step: Step) -> Result<(), Reason> {
    crate::open(p)?;
    let mut src = std::panic::catch_unwind(|| open(p)).unwrap_or(Err(Reason::Damaged))?;
    if src.entries.iter().any(|e| e.l) {
        return Err(Reason::Locked);
    }
    let names = names(&src.entries);
    let nodes: Vec<Node> = src
        .entries
        .iter()
        .zip(&names)
        .map(|(e, n)| Node {
            path: n.clone(),
            dir: e.d,
            i: if e.d { None } else { Some(e.i) },
            add: None,
            u: e.u,
            t: e.t.clone().or_else(|| e.u.and_then(local_time)),
        })
        .collect();
    let total: u64 = src.entries.iter().map(|e| e.s).sum::<u64>().max(1);
    let done = AtomicU64::new(0);
    let stop = AtomicBool::new(false);
    let f = out.file(target)?;
    drop(f);
    let temp = out.last_temp().ok_or(Reason::Unwritable)?.to_path_buf();
    let now = |_: &str| step(0.98 * done.load(Ordering::Relaxed) as f32 / total as f32);
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        write::write_archive(
            &temp,
            format(target),
            level(opts.pack),
            None,
            None,
            Some(&mut src),
            &nodes,
            &done,
            &now,
            &stop,
        )
    }));
    match result {
        Ok(Ok(())) => {
            // the new archive holds what the old one did: a downloaded archive's mark goes with it, so what is later
            // unpacked from it is marked too (moving the part file to its name keeps the mark)
            dotany_unpack::mark::Mark::of(p).put(&temp);
            step(1.0);
            Ok(())
        }
        Ok(Err(PackError::Password)) => Err(Reason::Locked),
        Ok(Err(PackError::Io(e)))
            if e.kind() == std::io::ErrorKind::InvalidData || e.kind() == std::io::ErrorKind::UnexpectedEof =>
        {
            Err(Reason::Damaged)
        }
        Ok(Err(PackError::Io(e)))
            if e.raw_os_error() == Some(112) || e.kind() == std::io::ErrorKind::PermissionDenied =>
        {
            Err(Reason::Unwritable)
        }
        // a read that failed partway through the source is the source, not the folder
        Ok(Err(PackError::Io(_))) | Ok(Err(PackError::Damaged(_))) | Err(_) => Err(Reason::Damaged),
        Ok(Err(PackError::Cancelled)) => Err(Reason::Damaged),
    }
}

pub fn estimate(p: &Path, size: u64, target: &str, opts: &Options) -> u64 {
    // what each level makes of the source's size, as a share of it
    let lvl = match opts.pack {
        Pack::Fast => 1.02,
        Pack::Normal => 0.96,
        Pack::Small => 0.9,
    };
    let kind = source::sniff_kind(p);
    let listed = match kind {
        // anything compressed would have to be read whole to be listed; its size is near enough
        Some(Kind::Tar(_)) | Some(Kind::Single(_)) | Some(Kind::Cpio(Some(_))) | Some(Kind::Rpm) | None => None,
        _ => Source::open(p, None).ok(),
    };
    let (plain, count) = match &listed {
        Some(s) => (s.entries.iter().map(|e| e.s).sum::<u64>(), s.entries.len() as u64),
        None if kind == Some(Kind::Tar(None)) => (size, size / 4096),
        None => ((size as f64 * 2.5) as u64, size / 4096),
    };
    match target {
        // a TAR holds every file as it is, in 512-byte blocks, a header for each
        "tar" => (plain + count * 1024 + 1024).div_ceil(10240) * 10240,
        "7z" => match kind {
            Some(Kind::Tar(None)) => (size as f64 * 0.55 * lvl) as u64,
            _ => (size as f64 * lvl * 0.86) as u64,
        },
        _ => match kind {
            Some(Kind::Tar(None)) => (size as f64 * 0.62 * lvl) as u64,
            // each entry has a local header and a central one, about 100 bytes and its name twice
            _ => (size as f64 * lvl) as u64 + count * 100,
        },
    }
}
