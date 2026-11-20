//! Working out how a file's bytes are written, and writing text back the same way.
//!
//! A file only opens for editing when we are sure of its encoding AND re-encoding the decoded text
//! gives back the same bytes. Anything else opens read-only, so a wrong guess can never reach the disk.

use chardetng::EncodingDetector;
use encoding_rs::{BIG5, EUC_KR, Encoding, GB18030, GBK, SHIFT_JIS, UTF_8, UTF_16BE, UTF_16LE};

/// How much non-ASCII evidence a legacy guess needs before we trust it.
const MIN_EVIDENCE: usize = 40;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TextEncoding {
    pub encoding: &'static Encoding,
    pub bom: bool,
}

impl TextEncoding {
    pub const UTF8: TextEncoding = TextEncoding { encoding: UTF_8, bom: false };

    pub fn label(&self) -> String {
        if self.encoding == UTF_8 {
            return if self.bom { "UTF-8 BOM".into() } else { "UTF-8".into() };
        }
        if self.encoding == UTF_16LE {
            return "UTF-16 LE".into();
        }
        if self.encoding == UTF_16BE {
            return "UTF-16 BE".into();
        }
        self.encoding.name().into()
    }

    pub fn from_label(label: &str) -> Option<TextEncoding> {
        match label {
            "UTF-8" => Some(TextEncoding::UTF8),
            "UTF-8 BOM" => Some(TextEncoding { encoding: UTF_8, bom: true }),
            "UTF-16 LE" => Some(TextEncoding { encoding: UTF_16LE, bom: true }),
            "UTF-16 BE" => Some(TextEncoding { encoding: UTF_16BE, bom: true }),
            other => Encoding::for_label(other.as_bytes()).map(|encoding| TextEncoding { encoding, bom: false }),
        }
    }

    pub fn bom_bytes(&self) -> &'static [u8] {
        if !self.bom {
            return &[];
        }
        if self.encoding == UTF_8 {
            &[0xEF, 0xBB, 0xBF]
        } else if self.encoding == UTF_16LE {
            &[0xFF, 0xFE]
        } else if self.encoding == UTF_16BE {
            &[0xFE, 0xFF]
        } else {
            &[]
        }
    }

    pub fn is_utf16(&self) -> bool {
        self.encoding == UTF_16LE || self.encoding == UTF_16BE
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Detection {
    pub encoding: TextEncoding,
    /// True when we would let this file be edited straight away.
    pub certain: bool,
}

/// None means this is not text we can open. `complete` is false when `bytes` is only the start of the file.
pub fn detect(bytes: &[u8], complete: bool) -> Option<Detection> {
    if let Some(encoding) = bom_of(bytes) {
        return Some(Detection { encoding, certain: true });
    }
    if bytes.len() >= 4 && bytes[..4] == [0xFF, 0xFE, 0, 0] {
        return None;
    }
    let head = &bytes[..bytes.len().min(8192)];
    if head.contains(&0) {
        return utf16_without_bom(head)
            .map(|encoding| Detection { encoding: TextEncoding { encoding, bom: false }, certain: false });
    }
    if is_utf8(bytes, complete) {
        return Some(Detection { encoding: TextEncoding::UTF8, certain: true });
    }
    let encoding = guess(bytes, complete, None);
    let mut enc = TextEncoding { encoding, bom: false };
    let evidence = bytes.iter().filter(|b| **b >= 0x80).count();
    let (decoded, errors) = encoding.decode_without_bom_handling(bytes);
    let clean = !errors || !complete;
    let agreed = reads_like_cjk(&decoded);
    // GBK cannot write every sequence it can read; GB18030 can
    if complete
        && encoding == GBK
        && clean
        && !round_trips(bytes, enc)
        && round_trips(bytes, TextEncoding { encoding: GB18030, bom: false })
    {
        enc.encoding = GB18030;
    }
    let exact = !complete || round_trips(bytes, enc);
    let trusted = [GBK, GB18030, BIG5, SHIFT_JIS, EUC_KR].contains(&enc.encoding);
    Some(Detection { encoding: enc, certain: trusted && evidence >= MIN_EVIDENCE && agreed && clean && exact })
}

/// A wrong guess turns into rare symbols and stray letters; real text is almost all ordinary characters.
fn reads_like_cjk(text: &str) -> bool {
    let (mut all, mut good) = (0usize, 0usize);
    for c in text.chars().filter(|c| !c.is_ascii()) {
        all += 1;
        if matches!(c as u32, 0x3000..=0x30FF | 0x4E00..=0x9FFF | 0xAC00..=0xD7A3 | 0xFF00..=0xFFEF | 0x2010..=0x2027) {
            good += 1;
        }
    }
    all > 0 && good * 100 >= all * 97
}

fn guess(bytes: &[u8], complete: bool, tld: Option<&[u8]>) -> &'static Encoding {
    let mut detector = EncodingDetector::new();
    detector.feed(bytes, complete);
    if !complete {
        detector.feed(&[], true);
    }
    detector.guess(tld, false)
}

fn bom_of(bytes: &[u8]) -> Option<TextEncoding> {
    if bytes.starts_with(&[0xEF, 0xBB, 0xBF]) {
        return Some(TextEncoding { encoding: UTF_8, bom: true });
    }
    if bytes.len() >= 4 && bytes[..4] == [0xFF, 0xFE, 0, 0] {
        return None;
    }
    if bytes.starts_with(&[0xFF, 0xFE]) {
        return Some(TextEncoding { encoding: UTF_16LE, bom: true });
    }
    if bytes.starts_with(&[0xFE, 0xFF]) {
        return Some(TextEncoding { encoding: UTF_16BE, bom: true });
    }
    None
}

fn is_utf8(bytes: &[u8], complete: bool) -> bool {
    match std::str::from_utf8(bytes) {
        Ok(_) => true,
        // a sample may stop in the middle of a character
        Err(e) => !complete && e.error_len().is_none(),
    }
}

/// UTF-16 without a mark shows up as zero bytes on one side only.
fn utf16_without_bom(head: &[u8]) -> Option<&'static Encoding> {
    if head.len() < 4 {
        return None;
    }
    let (mut even, mut odd) = (0usize, 0usize);
    for (i, b) in head.iter().enumerate() {
        if *b == 0 {
            if i % 2 == 0 { even += 1 } else { odd += 1 }
        }
    }
    let pairs = head.len() / 2;
    if odd * 10 >= pairs * 3 && even * 100 <= pairs {
        Some(UTF_16LE)
    } else if even * 10 >= pairs * 3 && odd * 100 <= pairs {
        Some(UTF_16BE)
    } else {
        None
    }
}

pub fn body_of<'a>(bytes: &'a [u8], enc: TextEncoding) -> &'a [u8] {
    if enc.bom { &bytes[enc.bom_bytes().len().min(bytes.len())..] } else { bytes }
}

/// The text, and whether any bytes could not be read (in which case saving would lose them).
pub fn decode(bytes: &[u8], enc: TextEncoding) -> (String, bool) {
    let body = body_of(bytes, enc);
    let (text, had_errors) = enc.encoding.decode_without_bom_handling(body);
    (text.into_owned(), had_errors)
}

fn round_trips(bytes: &[u8], enc: TextEncoding) -> bool {
    let (text, had_errors) = decode(bytes, enc);
    if had_errors {
        return false;
    }
    matches!(encode(&text, enc), Ok(out) if out == bytes)
}

/// True when opening and saving these bytes unchanged in this encoding gives back the same bytes.
pub fn is_exact(bytes: &[u8], enc: TextEncoding) -> bool {
    round_trips(bytes, enc)
}

/// The bytes for this text, or every character the encoding cannot write. Never substitutes anything.
pub fn encode(text: &str, enc: TextEncoding) -> Result<Vec<u8>, Vec<char>> {
    let mut out = Vec::with_capacity(text.len() + 4);
    out.extend_from_slice(enc.bom_bytes());
    if enc.encoding == UTF_8 {
        out.extend_from_slice(text.as_bytes());
        return Ok(out);
    }
    if enc.is_utf16() {
        let little = enc.encoding == UTF_16LE;
        for unit in text.encode_utf16() {
            out.extend_from_slice(&if little { unit.to_le_bytes() } else { unit.to_be_bytes() });
        }
        return Ok(out);
    }
    let mut encoder = enc.encoding.new_encoder();
    let mut bad: Vec<char> = Vec::new();
    let mut rest = text;
    let mut buf = vec![0u8; 16 * 1024];
    loop {
        let (result, read, written) = encoder.encode_from_utf8_without_replacement(rest, &mut buf, true);
        out.extend_from_slice(&buf[..written]);
        rest = &rest[read..];
        match result {
            encoding_rs::EncoderResult::InputEmpty => break,
            encoding_rs::EncoderResult::OutputFull => continue,
            encoding_rs::EncoderResult::Unmappable(c) => {
                if !bad.contains(&c) {
                    bad.push(c);
                }
            }
        }
    }
    if bad.is_empty() { Ok(out) } else { Err(bad) }
}
