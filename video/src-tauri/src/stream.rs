//! Serves one file to `<video>` a stretch at a time. Knows nothing about Video's window or which film is open.

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

/// The Mac: a film repackaged for the player (`dotany-repack`), served as `respond` serves a file, its samples read from
/// the original `file` as they are asked for.
#[cfg(target_os = "macos")]
pub fn respond_repacked(
    file: &Path,
    film: &dotany_repack::Repacked,
    range: Option<&str>,
) -> Response<Cow<'static, [u8]>> {
    let Ok(mut f) = File::open(file) else { return refused(404) };
    let len = film.size();
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
    match film.read_at(&mut f, start, &mut body) {
        Ok(n) if n == body.len() => {}
        _ => return refused(500),
    }
    Response::builder()
        .status(206)
        .header("Access-Control-Allow-Origin", "*")
        .header("Accept-Ranges", "bytes")
        .header("Content-Type", film.mime())
        .header("Content-Length", body.len())
        .header("Content-Range", format!("bytes {start}-{last}/{len}"))
        .body(Cow::Owned(body))
        .unwrap()
}

/// The Mac: a WebM or MKV served as `respond` serves it, with each stretch in `hidden` (start, end: whole elements of
/// the segment) seen as a Void element of the same length, so nothing after it moves. The file isn't touched.
#[cfg(target_os = "macos")]
pub fn respond_hiding(
    file: &Path,
    mime: &str,
    range: Option<&str>,
    hidden: &[(u64, u64)],
) -> Response<Cow<'static, [u8]>> {
    let mut answer = respond(file, mime, range);
    let start = answer
        .headers()
        .get("Content-Range")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("bytes "))
        .and_then(|v| v.split_once('-'))
        .and_then(|(a, _)| a.parse::<u64>().ok());
    if let (Some(start), Cow::Owned(body)) = (start, answer.body_mut()) {
        for &(at, end) in hidden {
            void_over(body, start, at, end);
        }
    }
    answer
}

/// Writes, where `body` (which starts at `start` in the file) holds it, the head of a Void element that spans
/// `at..end`: its id and its size, the size as wide as fits (at most eight bytes).
#[cfg(target_os = "macos")]
fn void_over(body: &mut [u8], start: u64, at: u64, end: u64) {
    let total = end.saturating_sub(at);
    if total < 2 {
        return;
    }
    let wide = (total - 1).min(8);
    let size = total - 1 - wide;
    let mut head = vec![0xECu8];
    for i in (0..wide).rev() {
        head.push((size >> (8 * i)) as u8);
    }
    head[1] |= 0x80 >> (wide - 1);
    for (i, b) in head.iter().enumerate() {
        let pos = at + i as u64;
        if pos >= start && pos - start < body.len() as u64 {
            body[(pos - start) as usize] = *b;
        }
    }
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::void_over;

    #[test]
    fn a_hidden_element_reads_as_a_void_of_the_same_length() {
        // an Attachments element of 20 bytes at 3, seen whole and in two pieces
        let mut whole = vec![0u8; 30];
        void_over(&mut whole, 0, 3, 23);
        assert_eq!(&whole[3..12], &[0xEC, 0x01, 0, 0, 0, 0, 0, 0, 11]);
        let (mut a, mut b) = (vec![0u8; 7], vec![0u8; 23]);
        void_over(&mut a, 0, 3, 23);
        void_over(&mut b, 7, 3, 23);
        assert_eq!([a, b].concat(), whole);
        // too short for eight bytes of size
        let mut small = vec![0u8; 6];
        void_over(&mut small, 0, 0, 6);
        assert_eq!(small, [0xEC, 0x08, 0, 0, 0, 0]);
    }
}
