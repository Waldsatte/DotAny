//! EXIF is a small TIFF directory tree. Enough of it is read here for the info panel, and it is edited in place where a
//! picture changes, so that everything a camera wrote (maker notes included) survives without its offsets moving.

use serde::Serialize;

#[derive(Clone, Copy)]
struct Order(bool);
impl Order {
    fn u16(self, b: &[u8], at: usize) -> Option<u16> {
        let s = b.get(at..at + 2)?;
        Some(if self.0 { u16::from_le_bytes([s[0], s[1]]) } else { u16::from_be_bytes([s[0], s[1]]) })
    }
    fn u32(self, b: &[u8], at: usize) -> Option<u32> {
        let s = b.get(at..at + 4)?;
        Some(if self.0 {
            u32::from_le_bytes([s[0], s[1], s[2], s[3]])
        } else {
            u32::from_be_bytes([s[0], s[1], s[2], s[3]])
        })
    }
    fn put16(self, b: &mut [u8], at: usize, v: u16) {
        let s = if self.0 { v.to_le_bytes() } else { v.to_be_bytes() };
        b[at..at + 2].copy_from_slice(&s);
    }
    fn put32(self, b: &mut [u8], at: usize, v: u32) {
        let s = if self.0 { v.to_le_bytes() } else { v.to_be_bytes() };
        b[at..at + 4].copy_from_slice(&s);
    }
}

fn order_of(b: &[u8]) -> Option<Order> {
    match b.get(..4)? {
        [b'I', b'I', 42, 0] => Some(Order(true)),
        [b'M', b'M', 0, 42] => Some(Order(false)),
        _ => None,
    }
}

const TYPE_SIZE: [usize; 14] = [0, 1, 1, 2, 4, 8, 1, 1, 2, 4, 8, 4, 8, 4];
const MAX_ENTRIES: usize = 4096;

#[derive(Clone, Debug)]
struct Entry {
    tag: u16,
    typ: u16,
    count: u32,
    /// Where this entry's twelve bytes start.
    at: usize,
    /// Where its value is: inside the entry when it fits, elsewhere otherwise.
    value_at: usize,
    value_len: usize,
}

fn read_ifd(b: &[u8], o: Order, at: usize) -> Option<(Vec<Entry>, u32)> {
    let n = o.u16(b, at)? as usize;
    if n > MAX_ENTRIES {
        return None;
    }
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let e = at + 2 + i * 12;
        let tag = o.u16(b, e)?;
        let typ = o.u16(b, e + 2)?;
        let count = o.u32(b, e + 4)?;
        let size = *TYPE_SIZE.get(typ as usize).unwrap_or(&0);
        if size == 0 {
            continue;
        }
        let value_len = size.checked_mul(count as usize)?;
        let value_at = if value_len <= 4 { e + 8 } else { o.u32(b, e + 8)? as usize };
        if value_at.checked_add(value_len)? > b.len() {
            continue;
        }
        out.push(Entry { tag, typ, count, at: e, value_at, value_len });
    }
    let next = o.u32(b, at + 2 + n * 12).unwrap_or(0);
    Some((out, next))
}

fn number(b: &[u8], o: Order, e: &Entry) -> Option<u32> {
    match e.typ {
        3 => o.u16(b, e.value_at).map(u32::from),
        4 | 9 => o.u32(b, e.value_at),
        1 => b.get(e.value_at).map(|v| *v as u32),
        _ => None,
    }
}

fn find(list: &[Entry], tag: u16) -> Option<&Entry> {
    list.iter().find(|e| e.tag == tag)
}

fn text(b: &[u8], e: &Entry) -> Option<String> {
    let raw = b.get(e.value_at..e.value_at + e.value_len)?;
    let end = raw.iter().position(|c| *c == 0).unwrap_or(raw.len());
    let s = String::from_utf8_lossy(&raw[..end]).trim().to_string();
    if s.is_empty() { None } else { Some(s) }
}

fn rational(b: &[u8], o: Order, e: &Entry, i: usize) -> Option<f64> {
    if e.typ != 5 && e.typ != 10 {
        return None;
    }
    let at = e.value_at + i * 8;
    let (n, d) = (o.u32(b, at)?, o.u32(b, at + 4)?);
    if d == 0 {
        return None;
    }
    Some(if e.typ == 10 { n as i32 as f64 / d as i32 as f64 } else { n as f64 / d as f64 })
}

/// Used on whole TIFF files: does the first directory carry any of these tags?
pub(crate) fn first_ifd_has(b: &[u8], tags: &[u16]) -> bool {
    let Some(o) = order_of(b) else { return false };
    let Some(first) = o.u32(b, 4) else { return false };
    match read_ifd(b, o, first as usize) {
        Some((list, _)) => list.iter().any(|e| tags.contains(&e.tag)),
        None => false,
    }
}

/// Raw values of some tags in a TIFF file's first directory, for the things that aren't EXIF proper.
pub(crate) fn first_ifd_raw(b: &[u8], tags: &[u16]) -> Vec<(u16, Vec<u8>)> {
    let Some(o) = order_of(b) else { return Vec::new() };
    let Some(first) = o.u32(b, 4) else { return Vec::new() };
    let Some((list, _)) = read_ifd(b, o, first as usize) else { return Vec::new() };
    list.iter()
        .filter(|e| tags.contains(&e.tag))
        .map(|e| (e.tag, b[e.value_at..e.value_at + e.value_len].to_vec()))
        .collect()
}

/// The TIFF reader refuses palette pictures, though it can read their index numbers as if they were grey. A copy that
/// says so, when any page needs it; the colours are put back from the page's own colour table after reading.
pub fn palette_read_as_grey(b: &[u8]) -> Option<Vec<u8>> {
    let o = order_of(b)?;
    let mut at = o.u32(b, 4)? as usize;
    let mut copy: Option<Vec<u8>> = None;
    for _ in 0..10_000 {
        if at == 0 {
            break;
        }
        let Some((list, next)) = read_ifd(b, o, at) else { break };
        if let Some(e) = find(&list, 262) {
            if number(b, o, e) == Some(3) && e.typ == 3 {
                o.put16(copy.get_or_insert_with(|| b.to_vec()), e.value_at, 1);
            }
        }
        at = next as usize;
    }
    copy
}

pub fn orientation(blob: &[u8]) -> u8 {
    let read = || -> Option<u8> {
        let o = order_of(blob)?;
        let (list, _) = read_ifd(blob, o, o.u32(blob, 4)? as usize)?;
        let v = number(blob, o, find(&list, 0x0112)?)?;
        if (1..=8).contains(&v) { Some(v as u8) } else { None }
    };
    read().unwrap_or(1)
}

// ---------- the info panel ----------

#[derive(Serialize, Default, Clone, Debug, PartialEq)]
pub struct Info {
    pub time: Option<String>,
    pub camera: Option<String>,
    pub lens: Option<String>,
    pub settings: Option<String>,
    pub place: Option<String>,
}

fn shorten_time(s: &str) -> String {
    // YYYY:MM:DD hh:mm:ss -> YYYY-MM-DD hh:mm
    let mut t: String = s.trim().chars().take(16).collect();
    if t.len() >= 10 {
        t = t.replacen(':', "-", 2);
    }
    t
}

fn coordinate(b: &[u8], o: Order, list: &[Entry], val: u16, side: u16, neg: char, pos: char) -> Option<String> {
    let e = find(list, val)?;
    if e.count < 3 {
        return None;
    }
    let deg = rational(b, o, e, 0)? + rational(b, o, e, 1)? / 60.0 + rational(b, o, e, 2)? / 3600.0;
    let hemi = find(list, side).and_then(|h| text(b, h)).and_then(|t| t.chars().next()).unwrap_or(pos);
    let hemi = if hemi == neg { neg } else { pos };
    Some(format!("{deg:.4}° {hemi}"))
}

pub fn info(blob: &[u8]) -> Info {
    let read = || -> Option<Info> {
        let o = order_of(blob)?;
        let (ifd0, _) = read_ifd(blob, o, o.u32(blob, 4)? as usize)?;
        let sub = |tag: u16| {
            find(&ifd0, tag)
                .and_then(|e| number(blob, o, e))
                .and_then(|at| read_ifd(blob, o, at as usize))
                .map(|r| r.0)
                .unwrap_or_default()
        };
        let (ex, gps) = (sub(0x8769), sub(0x8825));
        let mut out = Info::default();
        let when = find(&ex, 0x9003).or_else(|| find(&ex, 0x9004)).or_else(|| find(&ifd0, 0x0132));
        out.time = when.and_then(|e| text(blob, e)).filter(|t| !t.starts_with("0000")).map(|t| shorten_time(&t));
        let make = find(&ifd0, 0x010F).and_then(|e| text(blob, e));
        let model = find(&ifd0, 0x0110).and_then(|e| text(blob, e));
        out.camera = match (make, model) {
            (Some(a), Some(b))
                if b.to_lowercase().starts_with(a.split_whitespace().next().unwrap_or("").to_lowercase().as_str()) =>
            {
                Some(b)
            }
            (Some(a), Some(b)) => Some(format!("{a} {b}")),
            (a, b) => a.or(b),
        };
        out.lens = find(&ex, 0xA434).and_then(|e| text(blob, e));
        let mut parts = Vec::new();
        if let Some(f) = find(&ex, 0x829D).and_then(|e| rational(blob, o, e, 0)) {
            parts.push(format!("f/{}", trim_num(f)));
        }
        if let Some(t) = find(&ex, 0x829A).and_then(|e| rational(blob, o, e, 0)) {
            if t > 0.0 {
                parts.push(if t < 1.0 {
                    format!("1/{} s", (1.0 / t).round() as u32)
                } else {
                    format!("{} s", trim_num(t))
                });
            }
        }
        if let Some(iso) = find(&ex, 0x8827).and_then(|e| number(blob, o, e)) {
            parts.push(format!("ISO {iso}"));
        }
        if !parts.is_empty() {
            out.settings = Some(parts.join(" · "));
        }
        let lat = coordinate(blob, o, &gps, 2, 1, 'S', 'N');
        let lon = coordinate(blob, o, &gps, 4, 3, 'W', 'E');
        if let (Some(a), Some(b)) = (lat, lon) {
            if !(a.starts_with("0.0000") && b.starts_with("0.0000")) {
                out.place = Some(format!("{a}, {b}"));
            }
        }
        Some(out)
    };
    read().unwrap_or_default()
}

fn trim_num(v: f64) -> String {
    let s = format!("{v:.1}");
    s.strip_suffix(".0").map(str::to_string).unwrap_or(s)
}

// ---------- changing it in place ----------

/// One more directory holding this entry, at the end of the block, with everything else unmoved. That is safe even for maker notes.
fn add_orientation(mut blob: Vec<u8>, o: Order, ifd_at: usize, value: u16) -> Option<Vec<u8>> {
    let n = o.u16(&blob, ifd_at)? as usize;
    let mut entries: Vec<[u8; 12]> = Vec::with_capacity(n + 1);
    for i in 0..n {
        let e = ifd_at + 2 + i * 12;
        let mut one = [0u8; 12];
        one.copy_from_slice(blob.get(e..e + 12)?);
        entries.push(one);
    }
    let next = o.u32(&blob, ifd_at + 2 + n * 12).unwrap_or(0);
    let mut fresh = [0u8; 12];
    o.put16(&mut fresh, 0, 0x0112);
    o.put16(&mut fresh, 2, 3);
    o.put32(&mut fresh, 4, 1);
    o.put16(&mut fresh, 8, value);
    let tag_of = |e: &[u8; 12]| o.u16(e, 0).unwrap_or(0);
    let place = entries.iter().position(|e| tag_of(e) > 0x0112).unwrap_or(entries.len());
    entries.insert(place, fresh);
    if blob.len() % 2 == 1 {
        blob.push(0);
    }
    let at = blob.len();
    blob.resize(at + 2 + entries.len() * 12 + 4, 0);
    o.put16(&mut blob, at, entries.len() as u16);
    for (i, e) in entries.iter().enumerate() {
        blob[at + 2 + i * 12..at + 14 + i * 12].copy_from_slice(e);
    }
    o.put32(&mut blob, at + 2 + entries.len() * 12, next);
    o.put32(&mut blob, 4, at as u32);
    Some(blob)
}

pub(crate) fn set_orientation(blob: &[u8], value: u8) -> Option<Vec<u8>> {
    let o = order_of(blob)?;
    let first = o.u32(blob, 4)? as usize;
    let (list, _) = read_ifd(blob, o, first)?;
    let mut out = blob.to_vec();
    match find(&list, 0x0112) {
        Some(e) if e.typ == 3 && e.count == 1 => {
            o.put16(&mut out, e.value_at, value as u16);
            Some(out)
        }
        Some(_) => None,
        None if value == 1 => Some(out),
        None => add_orientation(out, o, first, value as u16),
    }
}

/// The smallest block that says only which way up the picture is.
pub(crate) fn orientation_only(value: u8) -> Vec<u8> {
    let mut b = vec![b'I', b'I', 42, 0, 8, 0, 0, 0, 1, 0];
    b.extend_from_slice(&0x0112u16.to_le_bytes());
    b.extend_from_slice(&3u16.to_le_bytes());
    b.extend_from_slice(&1u32.to_le_bytes());
    b.extend_from_slice(&(value as u32).to_le_bytes());
    b.extend_from_slice(&0u32.to_le_bytes());
    b
}

/// After the pixels themselves changed: they are upright now, sizes are what they are now, and the little preview
/// inside the block, which still shows the old picture (even a part that was cropped away), is erased.
pub fn after_pixels_changed(blob: &[u8], w: u32, h: u32) -> Option<Vec<u8>> {
    let mut out = set_orientation(blob, 1)?;
    let o = order_of(&out)?;
    let first = o.u32(&out, 4)? as usize;
    let (ifd0, next) = read_ifd(&out, o, first)?;
    if let Some(ex) = find(&ifd0, 0x8769).and_then(|e| number(&out, o, e)) {
        if let Some((list, _)) = read_ifd(&out, o, ex as usize) {
            for (tag, v) in [(0xA002u16, w), (0xA003u16, h)] {
                if let Some(e) = find(&list, tag) {
                    match e.typ {
                        3 if v <= u16::MAX as u32 => o.put16(&mut out, e.value_at, v as u16),
                        4 => o.put32(&mut out, e.value_at, v),
                        _ => {}
                    }
                }
            }
        }
    }
    if next != 0 {
        if let Some((ifd1, _)) = read_ifd(&out, o, next as usize) {
            if let (Some(off), Some(len)) = (
                find(&ifd1, 0x0201).and_then(|e| number(&out, o, e)),
                find(&ifd1, 0x0202).and_then(|e| number(&out, o, e)),
            ) {
                let (a, z) = (off as usize, off as usize + len as usize);
                if z <= out.len() {
                    out[a..z].fill(0);
                }
            }
            for e in &ifd1 {
                if e.value_at != e.at + 8 {
                    let z = e.value_at + e.value_len;
                    if z <= out.len() {
                        out[e.value_at..z].fill(0);
                    }
                }
            }
            let table = next as usize;
            let z = (table + 2 + ifd1.len() * 12 + 4).min(out.len());
            out[table..z].fill(0);
        }
        // the pointer to it is the last thing in the first directory
        let n = o.u16(&out, first)? as usize;
        o.put32(&mut out, first + 2 + n * 12, 0);
    }
    let end = used_end(&out, o, first)?;
    out.truncate(end.max(8));
    Some(out)
}

/// The end of the last thing still referred to. Whatever the erased preview left beyond that goes.
fn used_end(b: &[u8], o: Order, first: usize) -> Option<usize> {
    let mut end = 0usize;
    let mut stack = vec![first];
    let mut seen = 0;
    while let Some(at) = stack.pop() {
        seen += 1;
        if seen > 16 {
            break;
        }
        let (list, _) = read_ifd(b, o, at)?;
        end = end.max(at + 2 + o.u16(b, at)? as usize * 12 + 4);
        for e in &list {
            end = end.max(e.value_at + e.value_len);
            if matches!(e.tag, 0x8769 | 0x8825 | 0xA005) {
                if let Some(sub) = number(b, o, e) {
                    stack.push(sub as usize);
                }
            }
        }
    }
    Some(end.min(b.len()))
}

// ---------- rebuilding: a TIFF file's own tags as an EXIF block, and an EXIF block's tags for a TIFF file ----------

/// A tag to write into a TIFF: its type, count, and value bytes in little-endian order.
#[derive(Clone, Debug)]
pub struct Tag {
    pub tag: u16,
    pub typ: u16,
    pub count: u32,
    pub value: Vec<u8>,
}

/// Tags of the first directory that describe the picture's content rather than its pixels' layout.
const DESCRIPTIVE: [u16; 7] = [0x010E, 0x010F, 0x0110, 0x0131, 0x0132, 0x013B, 0x8298];

fn little(b: &[u8], o: Order, e: &Entry) -> Vec<u8> {
    let raw = &b[e.value_at..e.value_at + e.value_len];
    let unit = TYPE_SIZE[e.typ as usize];
    if o.0 || unit == 1 {
        return raw.to_vec();
    }
    let mut v = raw.to_vec();
    // rationals are two longs each
    let swap = if e.typ == 5 || e.typ == 10 { 4 } else { unit };
    for c in v.chunks_mut(swap) {
        c.reverse();
    }
    v
}

#[derive(Default, Clone, Debug)]
pub struct Tree {
    pub ifd0: Vec<Tag>,
    pub exif: Vec<Tag>,
    pub gps: Vec<Tag>,
}

/// Reads a block or a whole TIFF file into plain lists. Maker notes are left out: their insides point at places in the old block.
pub fn tree(blob: &[u8]) -> Option<Tree> {
    let o = order_of(blob)?;
    let (ifd0, _) = read_ifd(blob, o, o.u32(blob, 4)? as usize)?;
    let tag_of = |e: &Entry| Tag { tag: e.tag, typ: e.typ, count: e.count, value: little(blob, o, e) };
    let sub = |tag: u16| {
        find(&ifd0, tag)
            .and_then(|e| number(blob, o, e))
            .and_then(|at| read_ifd(blob, o, at as usize))
            .map(|r| r.0)
            .unwrap_or_default()
    };
    Some(Tree {
        ifd0: ifd0.iter().filter(|e| DESCRIPTIVE.contains(&e.tag)).map(tag_of).collect(),
        exif: sub(0x8769).iter().filter(|e| !matches!(e.tag, 0x927C | 0xA005 | 0xA002 | 0xA003)).map(tag_of).collect(),
        gps: sub(0x8825).iter().map(tag_of).collect(),
    })
}

impl Tree {
    pub fn is_empty(&self) -> bool {
        self.ifd0.is_empty() && self.exif.is_empty() && self.gps.is_empty()
    }
}

/// Writes one directory at the current end of a little-endian file; large values go after the table.
pub fn write_ifd(out: &mut Vec<u8>, tags: &[Tag], next: u32) {
    let at = out.len();
    let mut sorted: Vec<&Tag> = tags.iter().collect();
    sorted.sort_by_key(|t| t.tag);
    let table = 2 + sorted.len() * 12 + 4;
    let mut extra = at + table;
    let mut body = Vec::new();
    out.extend_from_slice(&(sorted.len() as u16).to_le_bytes());
    for t in &sorted {
        out.extend_from_slice(&t.tag.to_le_bytes());
        out.extend_from_slice(&t.typ.to_le_bytes());
        out.extend_from_slice(&t.count.to_le_bytes());
        if t.value.len() <= 4 {
            let mut v = t.value.clone();
            v.resize(4, 0);
            out.extend_from_slice(&v);
        } else {
            out.extend_from_slice(&(extra as u32).to_le_bytes());
            body.extend_from_slice(&t.value);
            extra += t.value.len();
            if t.value.len() % 2 == 1 {
                body.push(0);
                extra += 1;
            }
        }
    }
    out.extend_from_slice(&next.to_le_bytes());
    out.extend_from_slice(&body);
}

/// Lays out sub-directories at the current end of the file and adds the pointers to them to `ifd0`.
pub fn place_subdirs(out: &mut Vec<u8>, tree: &Tree, ifd0: &mut Vec<Tag>) {
    for (pointer, list) in [(0x8769u16, &tree.exif), (0x8825u16, &tree.gps)] {
        if list.is_empty() {
            continue;
        }
        if out.len() % 2 == 1 {
            out.push(0);
        }
        let at = out.len() as u32;
        write_ifd(out, list, 0);
        ifd0.push(Tag { tag: pointer, typ: 4, count: 1, value: at.to_le_bytes().to_vec() });
    }
}

/// A self-contained EXIF block from a tree.
pub fn block(tree: &Tree) -> Vec<u8> {
    let mut out = vec![b'I', b'I', 42, 0, 0, 0, 0, 0];
    let mut ifd0 = tree.ifd0.clone();
    place_subdirs(&mut out, tree, &mut ifd0);
    if out.len() % 2 == 1 {
        out.push(0);
    }
    let at = out.len() as u32;
    write_ifd(&mut out, &ifd0, 0);
    out[4..8].copy_from_slice(&at.to_le_bytes());
    out
}
