//! The read-only long scroll: a file of any size, indexed in the background and read a few lines at a time.
//! The index keeps one offset per `STEP` lines, so a 500 MB file with ten million lines needs about 80 KB.

use crate::encoding::TextEncoding;
use regex::{Regex, RegexBuilder};
use serde::Serialize;
use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant};

pub const STEP: u64 = 1024;
/// A single line longer than this is shown cut short; the file itself is never touched.
pub const MAX_LINE_BYTES: usize = 64 * 1024;
const CHUNK: usize = 1024 * 1024;
const MAX_MATCHES: usize = 100_000;

#[derive(Clone, Copy, PartialEq, Debug, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Level {
    All,
    Warn,
    Error,
}

pub struct LongDoc {
    pub path: PathBuf,
    encoding: RwLock<TextEncoding>,
    /// Offset of line `i * STEP`.
    index: RwLock<Vec<u64>>,
    lines: AtomicU64,
    scanned: AtomicU64,
    size: AtomicU64,
    done: AtomicBool,
    last_newline_end: AtomicU64,
    filter: RwLock<Option<Arc<Vec<(u64, u64)>>>>,
    filter_done: AtomicBool,
    filter_gen: AtomicU64,
    find: Mutex<FindState>,
    find_gen: AtomicU64,
    stopped: AtomicBool,
}

#[derive(Default)]
struct FindState {
    regex: Option<Regex>,
    /// (view row, line, nth match in that line)
    matches: Vec<(u64, u64, u32)>,
    done: bool,
    capped: bool,
}

#[derive(Serialize, Clone, Debug)]
pub struct Progress {
    pub lines: u64,
    pub scanned: u64,
    pub size: u64,
    pub done: bool,
    pub rows: u64,
    pub filter_done: bool,
}

#[derive(Serialize, Clone, Debug)]
pub struct Line {
    pub no: u64,
    pub text: String,
    pub cut: bool,
    /// UTF-16 ranges of find matches in this line.
    pub marks: Vec<(u32, u32)>,
}

#[derive(Serialize, Clone, Debug)]
pub struct FindStatus {
    /// The page's number for the search this answers: an answer for an older search is dropped there.
    pub seq: u64,
    pub count: usize,
    pub done: bool,
    pub capped: bool,
    pub valid: bool,
}

#[derive(Serialize, Clone, Debug)]
pub struct FindHit {
    pub index: usize,
    pub row: u64,
    pub line: u64,
    pub nth: u32,
}

/// The offset just past each line break in a block of bytes, in this encoding.
fn each_break(enc: TextEncoding, base: u64, buf: &[u8], mut found: impl FnMut(u64)) {
    if enc.is_utf16() {
        let little = enc.encoding == encoding_rs::UTF_16LE;
        for at in memchr::memchr_iter(b'\n', buf) {
            let abs = base + at as u64;
            if little {
                if abs % 2 == 0 && buf.get(at + 1) == Some(&0) {
                    found(abs + 2);
                }
            } else if abs % 2 == 1 && at > 0 && buf[at - 1] == 0 {
                found(abs + 1);
            }
        }
    } else {
        for at in memchr::memchr_iter(b'\n', buf) {
            found(base + at as u64 + 1);
        }
    }
}

impl LongDoc {
    pub fn new(path: &Path, encoding: TextEncoding, size: u64) -> Arc<LongDoc> {
        let start = encoding.bom_bytes().len() as u64;
        Arc::new(LongDoc {
            path: path.to_path_buf(),
            encoding: RwLock::new(encoding),
            index: RwLock::new(vec![start]),
            lines: AtomicU64::new(1),
            scanned: AtomicU64::new(start),
            size: AtomicU64::new(size),
            done: AtomicBool::new(false),
            last_newline_end: AtomicU64::new(start),
            filter: RwLock::new(None),
            filter_done: AtomicBool::new(true),
            filter_gen: AtomicU64::new(0),
            find: Mutex::new(FindState::default()),
            find_gen: AtomicU64::new(0),
            stopped: AtomicBool::new(false),
        })
    }

    pub fn stop(&self) {
        self.stopped.store(true, Ordering::Relaxed);
        self.find_gen.fetch_add(1, Ordering::Relaxed);
        self.filter_gen.fetch_add(1, Ordering::Relaxed);
    }

    pub fn encoding(&self) -> TextEncoding {
        *self.encoding.read().unwrap()
    }

    /// Reads from where the last scan stopped to the end of the file; also how a growing log is followed.
    pub fn scan(&self, mut tick: impl FnMut(&Progress)) -> io::Result<()> {
        let enc = self.encoding();
        let mut file = File::open(&self.path)?;
        self.size.store(file.metadata()?.len(), Ordering::Relaxed);
        let mut pos = self.scanned.load(Ordering::Relaxed);
        file.seek(SeekFrom::Start(pos))?;
        let mut buf = vec![0u8; CHUNK];
        let mut last = Instant::now();
        // UTF-16 line breaks must not be split between two reads
        let keep_even = enc.is_utf16();
        let mut carry = 0usize;
        loop {
            if self.stopped.load(Ordering::Relaxed) {
                return Ok(());
            }
            let n = file.read(&mut buf[carry..])?;
            if n == 0 {
                break;
            }
            let have = carry + n;
            let usable = if keep_even { have & !1 } else { have };
            let mut lines = self.lines.load(Ordering::Relaxed);
            let mut end = self.last_newline_end.load(Ordering::Relaxed);
            let mut new_offsets: Vec<u64> = Vec::new();
            each_break(enc, pos, &buf[..usable], |offset| {
                if lines % STEP == 0 {
                    new_offsets.push(offset);
                }
                lines += 1;
                end = offset;
            });
            if !new_offsets.is_empty() {
                self.index.write().unwrap().extend_from_slice(&new_offsets);
            }
            self.lines.store(lines, Ordering::Relaxed);
            self.last_newline_end.store(end, Ordering::Relaxed);
            pos += usable as u64;
            self.scanned.store(pos, Ordering::Relaxed);
            carry = have - usable;
            if carry > 0 {
                buf[0] = buf[usable];
            }
            if last.elapsed() > Duration::from_millis(120) {
                last = Instant::now();
                tick(&self.progress());
            }
        }
        self.done.store(true, Ordering::Relaxed);
        tick(&self.progress());
        Ok(())
    }

    /// Lines are counted by their breaks, so a file ending in a break would show one empty line more than it has.
    fn line_count(&self) -> u64 {
        let lines = self.lines.load(Ordering::Relaxed);
        let done = self.done.load(Ordering::Relaxed);
        if done && lines > 1 && self.last_newline_end.load(Ordering::Relaxed) == self.scanned.load(Ordering::Relaxed) {
            lines - 1
        } else {
            lines
        }
    }

    pub fn rows(&self) -> u64 {
        match &*self.filter.read().unwrap() {
            Some(list) => list.len() as u64,
            None => self.line_count(),
        }
    }

    pub fn progress(&self) -> Progress {
        Progress {
            lines: self.line_count(),
            scanned: self.scanned.load(Ordering::Relaxed),
            size: self.size.load(Ordering::Relaxed),
            done: self.done.load(Ordering::Relaxed),
            rows: self.rows(),
            filter_done: self.filter_done.load(Ordering::Relaxed),
        }
    }

    fn line_offset_block(&self, line: u64) -> (u64, u64) {
        let index = self.index.read().unwrap();
        let block = ((line / STEP) as usize).min(index.len() - 1);
        (index[block], block as u64 * STEP)
    }

    fn clean(&self, raw: &[u8], enc: TextEncoding) -> String {
        let (text, _) = enc.encoding.decode_without_bom_handling(raw);
        let trimmed = text.strip_suffix('\n').unwrap_or(&text);
        let trimmed = trimmed.strip_suffix('\r').unwrap_or(trimmed);
        trimmed.to_string()
    }

    /// Reads consecutive lines from `offset`; `first` is the line number at that offset.
    fn read_lines(
        &self,
        offset: u64,
        first: u64,
        skip: u64,
        want: u64,
        enc: TextEncoding,
        out: &mut dyn FnMut(u64, &[u8], bool) -> bool,
    ) -> io::Result<()> {
        let mut file = File::open(&self.path)?;
        file.seek(SeekFrom::Start(offset))?;
        let mut buf = vec![0u8; 256 * 1024];
        let mut line_no = first;
        let (end_at, mut given) = (first + skip + want, 0u64);
        let mut cur: Vec<u8> = Vec::new();
        let mut cut = false;
        let mut pos = offset;
        let mut filled = 0usize;
        loop {
            let n = file.read(&mut buf[filled..])?;
            if n == 0 && filled == 0 {
                break;
            }
            let have = filled + n;
            let usable = if enc.is_utf16() && n != 0 { have & !1 } else { have };
            let mut start = 0usize;
            let mut breaks: Vec<usize> = Vec::new();
            each_break(enc, pos, &buf[..usable], |o| breaks.push((o - pos) as usize));
            for stop in breaks {
                if line_no >= first + skip {
                    let take = stop - start;
                    let room = MAX_LINE_BYTES.saturating_sub(cur.len());
                    if take > room {
                        cur.extend_from_slice(&buf[start..start + room]);
                        cut = true;
                    } else {
                        cur.extend_from_slice(&buf[start..stop]);
                    }
                    if !out(line_no, &cur, cut) {
                        return Ok(());
                    }
                    given += 1;
                }
                cur.clear();
                cut = false;
                start = stop;
                line_no += 1;
                if line_no >= end_at {
                    return Ok(());
                }
            }
            if line_no >= first + skip {
                let room = MAX_LINE_BYTES.saturating_sub(cur.len());
                let take = usable - start;
                if take > room {
                    cur.extend_from_slice(&buf[start..start + room]);
                    cut = true;
                } else {
                    cur.extend_from_slice(&buf[start..usable]);
                }
            }
            pos += usable as u64;
            filled = have - usable;
            if filled > 0 {
                buf[0] = buf[usable];
            }
            if n == 0 {
                break;
            }
        }
        if (!cur.is_empty() || (given == 0 && line_no < end_at)) && line_no >= first + skip {
            out(line_no, &cur, cut);
        }
        Ok(())
    }

    fn marks(&self, regex: Option<&Regex>, text: &str) -> Vec<(u32, u32)> {
        let Some(re) = regex else { return Vec::new() };
        re.find_iter(text)
            .filter(|m| !m.is_empty())
            .map(|m| (text[..m.start()].encode_utf16().count() as u32, text[..m.end()].encode_utf16().count() as u32))
            .collect()
    }

    /// `start` counts view rows: lines, or the lines that pass the filter while one is on.
    pub fn fetch(&self, start: u64, count: u64) -> io::Result<Vec<Line>> {
        let enc = self.encoding();
        let regex = self.find.lock().unwrap().regex.clone();
        let mut result = Vec::with_capacity(count as usize);
        let filter = self.filter.read().unwrap().clone();
        if let Some(list) = filter {
            for &(line, offset) in list.iter().skip(start as usize).take(count as usize) {
                self.read_lines(offset, line, 0, 1, enc, &mut |no, raw, cut| {
                    let mut text = self.clean(raw, enc);
                    if cut {
                        text.push('…');
                    }
                    let marks = self.marks(regex.as_ref(), &text);
                    result.push(Line { no, text, cut, marks });
                    false
                })?;
            }
            return Ok(result);
        }
        let total = self.line_count();
        if start >= total {
            return Ok(result);
        }
        let want = count.min(total - start);
        let (offset, first) = self.line_offset_block(start);
        self.read_lines(offset, first, start - first, want, enc, &mut |no, raw, cut| {
            let mut text = self.clean(raw, enc);
            if cut {
                text.push('…');
            }
            let marks = self.marks(regex.as_ref(), &text);
            result.push(Line { no, text, cut, marks });
            true
        })?;
        Ok(result)
    }

    /// Every line in the view, from the start of the file; stops early when `out` returns false.
    fn each_line(&self, stop: &dyn Fn() -> bool, out: &mut dyn FnMut(u64, u64, &[u8]) -> bool) -> io::Result<()> {
        let enc = self.encoding();
        let mut file = File::open(&self.path)?;
        let mut pos = enc.bom_bytes().len() as u64;
        file.seek(SeekFrom::Start(pos))?;
        let mut buf = vec![0u8; CHUNK];
        let mut filled = 0usize;
        let (mut line_no, mut line_at) = (0u64, pos);
        let mut spill: Vec<u8> = Vec::new();
        loop {
            if stop() {
                return Ok(());
            }
            let n = file.read(&mut buf[filled..])?;
            let have = filled + n;
            let usable = if enc.is_utf16() && n != 0 { have & !1 } else { have };
            let mut start = 0usize;
            let mut breaks: Vec<usize> = Vec::new();
            each_break(enc, pos, &buf[..usable], |o| breaks.push((o - pos) as usize));
            for at in breaks {
                let keep_going = if spill.is_empty() {
                    out(line_no, line_at, &buf[start..at])
                } else {
                    if spill.len() < MAX_LINE_BYTES {
                        spill.extend_from_slice(&buf[start..at]);
                    }
                    let go = out(line_no, line_at, &spill);
                    spill.clear();
                    go
                };
                if !keep_going {
                    return Ok(());
                }
                line_no += 1;
                line_at = pos + at as u64;
                start = at;
            }
            if spill.len() < MAX_LINE_BYTES {
                spill.extend_from_slice(&buf[start..usable]);
            }
            pos += usable as u64;
            filled = have - usable;
            if filled > 0 {
                buf[0] = buf[usable];
            }
            if n == 0 {
                break;
            }
        }
        if !spill.is_empty() {
            out(line_no, line_at, &spill);
        }
        Ok(())
    }

    /// Builds the list of lines at this level or above. Runs on its own thread.
    pub fn apply_filter(&self, level: Level, mut tick: impl FnMut(&Progress)) -> io::Result<()> {
        let gen = self.filter_gen.fetch_add(1, Ordering::SeqCst) + 1;
        if level == Level::All {
            *self.filter.write().unwrap() = None;
            self.filter_done.store(true, Ordering::Relaxed);
            tick(&self.progress());
            return Ok(());
        }
        self.filter_done.store(false, Ordering::Relaxed);
        *self.filter.write().unwrap() = Some(Arc::new(Vec::new()));
        let enc = self.encoding();
        let mut found: Vec<(u64, u64)> = Vec::new();
        let mut last = Instant::now();
        let stale = || self.filter_gen.load(Ordering::SeqCst) != gen || self.stopped.load(Ordering::Relaxed);
        self.each_line(&stale, &mut |line, offset, raw| {
            let pass = if enc.is_utf16() || !enc.encoding.is_ascii_compatible() {
                let (t, _) = enc.encoding.decode_without_bom_handling(raw);
                level_passes(t.as_bytes(), level)
            } else {
                level_passes(raw, level)
            };
            if pass {
                found.push((line, offset));
            }
            if last.elapsed() > Duration::from_millis(120) {
                last = Instant::now();
                *self.filter.write().unwrap() = Some(Arc::new(found.clone()));
                tick(&self.progress());
            }
            true
        })?;
        if stale() {
            return Ok(());
        }
        *self.filter.write().unwrap() = Some(Arc::new(found));
        self.filter_done.store(true, Ordering::Relaxed);
        tick(&self.progress());
        Ok(())
    }

    /// The view row of a line: its own number, or its place among the filtered lines.
    fn row_of(&self, line: u64) -> Option<u64> {
        match &*self.filter.read().unwrap() {
            Some(list) => list.binary_search_by_key(&line, |x| x.0).ok().map(|i| i as u64),
            None => Some(line),
        }
    }

    /// Counts matches in the background. An empty query clears the search. `seq` grows with each search the page starts:
    /// threads can start out of order while someone types, and an older search must never replace a newer one. So the
    /// shared state is touched only under its lock after checking that this search is still the newest (a newer one
    /// raises `find_gen` before it takes the lock), and every answer carries `seq`, so the page drops an older answer
    /// that arrives late: answers from two threads can reach the page in either order.
    pub fn start_find(
        &self,
        seq: u64,
        query: &str,
        match_case: bool,
        whole_word: bool,
        is_regex: bool,
        mut tick: impl FnMut(FindStatus),
    ) -> io::Result<()> {
        if self.find_gen.fetch_max(seq, Ordering::SeqCst) > seq {
            return Ok(());
        }
        let gen = seq;
        let stale = || self.find_gen.load(Ordering::SeqCst) != gen || self.stopped.load(Ordering::Relaxed);
        // the shared state, locked, only while this search is still the newest
        let current = || {
            let st = self.find.lock().unwrap();
            if stale() { None } else { Some(st) }
        };
        let status = |count, done, capped, valid| FindStatus { seq, count, done, capped, valid };
        if query.is_empty() {
            let Some(mut st) = current() else { return Ok(()) };
            *st = FindState::default();
            drop(st);
            tick(status(0, true, false, true));
            return Ok(());
        }
        let source = if is_regex { query.to_string() } else { regex::escape(query) };
        let source = if whole_word { format!(r"\b(?:{source})\b") } else { source };
        let Ok(regex) = RegexBuilder::new(&source).case_insensitive(!match_case).size_limit(1 << 22).build() else {
            let Some(mut st) = current() else { return Ok(()) };
            *st = FindState::default();
            drop(st);
            tick(status(0, true, false, false));
            return Ok(());
        };
        {
            let Some(mut st) = current() else { return Ok(()) };
            *st = FindState { regex: Some(regex.clone()), ..Default::default() };
        }
        let enc = self.encoding();
        let filtered = self.filter.read().unwrap().is_some();
        let mut batch: Vec<(u64, u64, u32)> = Vec::new();
        let mut last = Instant::now();
        let mut full = false;
        self.each_line(&stale, &mut |line, _, raw| {
            let (text, _) = enc.encoding.decode_without_bom_handling(raw);
            let hits = regex.find_iter(&text).filter(|m| !m.is_empty()).count();
            if hits > 0 {
                let row = if filtered { self.row_of(line) } else { Some(line) };
                if let Some(row) = row {
                    for nth in 0..hits {
                        batch.push((row, line, nth as u32));
                    }
                }
            }
            if last.elapsed() > Duration::from_millis(100) || batch.len() > 20_000 {
                last = Instant::now();
                // a newer search may have started since the last line: its state is not this one's to add to
                let Some(mut st) = current() else { return false };
                st.matches.append(&mut batch);
                full = st.matches.len() >= MAX_MATCHES;
                if full {
                    st.capped = true;
                }
                let now = status(st.matches.len(), false, st.capped, true);
                drop(st);
                tick(now);
            }
            !full
        })?;
        let Some(mut st) = current() else { return Ok(()) };
        st.matches.append(&mut batch);
        st.done = true;
        let now = status(st.matches.len(), true, st.capped, true);
        drop(st);
        tick(now);
        Ok(())
    }

    /// The next (or previous) match from a view row, wrapping round.
    pub fn find_step(&self, from_row: u64, forward: bool, current: Option<usize>) -> Option<FindHit> {
        let st = self.find.lock().unwrap();
        if st.matches.is_empty() {
            return None;
        }
        let len = st.matches.len();
        let index = match current {
            Some(i) if forward => (i + 1) % len,
            Some(i) => (i + len - 1) % len,
            None => {
                let at = st.matches.partition_point(|m| m.0 < from_row);
                if forward { at % len } else { (at + len - 1) % len }
            }
        };
        let m = st.matches[index];
        Some(FindHit { index, row: m.0, line: m.1, nth: m.2 })
    }

    /// The whole text for printing, when it is small enough to hold.
    pub fn read_all(&self) -> io::Result<String> {
        let enc = self.encoding();
        let mut bytes = Vec::new();
        File::open(&self.path)?.read_to_end(&mut bytes)?;
        let (text, _) = crate::encoding::decode(&bytes, enc);
        Ok(crate::document::normalize_eol(&text))
    }
}

/// `\b(WARN|ERROR)\b` / `\bERROR\b`, without a regex for each of ten million lines.
pub fn level_passes(line: &[u8], level: Level) -> bool {
    let has = |word: &[u8]| {
        memchr::memmem::find_iter(line, word).any(|at| {
            let before = at == 0 || !is_word(line[at - 1]);
            let after = at + word.len() >= line.len() || !is_word(line[at + word.len()]);
            before && after
        })
    };
    match level {
        Level::All => true,
        Level::Warn => has(b"WARN") || has(b"ERROR"),
        Level::Error => has(b"ERROR"),
    }
}

fn is_word(b: u8) -> bool {
    b == b'_' || b.is_ascii_alphanumeric()
}
