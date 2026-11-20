//! Serves one file to the page's media element a stretch at a time; it knows nothing about the window or the open file.

use std::borrow::Cow;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;
use tauri::http::Response;

/// The most the player gets in one answer: it asks again for the next stretch.
const STRETCH: u64 = 8 << 20;

pub fn refused(status: u16) -> Response<Cow<'static, [u8]>> {
    Response::builder().status(status).header("Access-Control-Allow-Origin", "*").body(Cow::Borrowed(&[][..])).unwrap()
}

/// `range` is the request's Range header, if it had one.
pub fn respond(file: &Path, mime: &str, range: Option<&str>) -> Response<Cow<'static, [u8]>> {
    let Ok(mut f) = File::open(file) else { return refused(404) };
    let Ok(len) = f.metadata().map(|m| m.len()) else { return refused(404) };
    let (start, last) = range
        .and_then(|r| r.strip_prefix("bytes="))
        .and_then(|r| r.split(',').next())
        .and_then(|r| r.split_once('-'))
        .map(|(a, b)| (a.trim().parse::<u64>().unwrap_or(0), b.trim().parse::<u64>().ok()))
        .unwrap_or((0, None));
    if len == 0 || start >= len {
        return Response::builder()
            .status(416)
            .header("Access-Control-Allow-Origin", "*")
            .header("Content-Range", format!("bytes */{len}"))
            .body(Cow::Borrowed(&[][..]))
            .unwrap();
    }
    let last = last.unwrap_or(len - 1).min(len - 1).min(start + STRETCH - 1);
    let mut body = vec![0u8; (last - start + 1) as usize];
    if f.seek(SeekFrom::Start(start)).and_then(|_| f.read_exact(&mut body)).is_err() {
        return refused(500);
    }
    Response::builder()
        .status(206)
        .header("Access-Control-Allow-Origin", "*")
        .header("Accept-Ranges", "bytes")
        .header("Content-Type", mime)
        .header("Content-Length", body.len())
        .header("Content-Range", format!("bytes {start}-{last}/{len}"))
        .body(Cow::Owned(body))
        .unwrap()
}
