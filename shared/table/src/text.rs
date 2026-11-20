//! CSV and TSV as text: which encoding the bytes are in, reading them, and writing them back the same way.
//! Reading and writing go through the same tables (encoding_rs), so a file that went in comes out byte for byte.

use encoding_rs::Encoding;
use serde::Serialize;

#[derive(Serialize)]
pub struct Decoded {
    pub text: String,
    /// "utf-8", "gbk", "big5", "shift_jis", "utf-16le", … as the page names them
    pub enc: String,
    pub bom: bool,
    /// Writing the text back in `enc` gives exactly the bytes we read.
    pub lossless: bool,
    /// The encoding was a guess, not a certainty: no BOM and not valid UTF-8.
    pub guessed: bool,
}

fn label(e: &'static Encoding) -> String {
    // the page's names: the WHATWG ones in lower case, with GBK for the GB18030 family as Windows names it
    match e.name() {
        "gb18030" | "GBK" => "gbk".into(),
        "Shift_JIS" => "shift_jis".into(),
        other => other.to_ascii_lowercase(),
    }
}

fn by_label(enc: &str) -> Option<&'static Encoding> {
    match enc {
        "utf-16le" => Some(encoding_rs::UTF_16LE),
        "utf-16be" => Some(encoding_rs::UTF_16BE),
        other => Encoding::for_label(other.as_bytes()),
    }
}

fn bom_of(bytes: &[u8]) -> Option<(&'static str, usize)> {
    if bytes.starts_with(&[0xEF, 0xBB, 0xBF]) {
        Some(("utf-8", 3))
    } else if bytes.starts_with(&[0xFF, 0xFE]) {
        Some(("utf-16le", 2))
    } else if bytes.starts_with(&[0xFE, 0xFF]) {
        Some(("utf-16be", 2))
    } else {
        None
    }
}

/// The encoding these bytes are most likely in: a BOM, valid UTF-8, or chardetng's guess (as Firefox guesses).
fn detect(bytes: &[u8]) -> (String, bool, bool) {
    if let Some((enc, _)) = bom_of(bytes) {
        return (enc.into(), true, false);
    }
    if std::str::from_utf8(bytes).is_ok() {
        return ("utf-8".into(), false, false);
    }
    let mut d = chardetng::EncodingDetector::new(chardetng::Iso2022JpDetection::Deny);
    d.feed(bytes, true);
    (label(d.guess(None, chardetng::Utf8Detection::Allow)), false, true)
}

/// The bytes read as `enc` (or as detected, when `enc` is None).
pub fn decode(bytes: &[u8], enc: Option<&str>) -> Decoded {
    let (detected, has_bom, guessed) = detect(bytes);
    let enc = enc.map(str::to_string).unwrap_or(detected);
    let encoding = by_label(&enc).unwrap_or(encoding_rs::UTF_8);
    // a BOM is kept only when it belongs to the encoding being read
    let skip = match bom_of(bytes) {
        Some((b, n)) if b == enc => n,
        _ => 0,
    };
    let bom = has_bom && skip > 0;
    let (text, had_errors) = encoding.decode_without_bom_handling(&bytes[skip..]);
    let text = text.into_owned();
    let lossless = !had_errors && encode(&text, &enc, bom).is_ok_and(|b| b == bytes);
    Decoded { text, enc, bom, lossless, guessed: guessed && skip == 0 }
}

/// The text in `enc`, with the BOM if asked. Fails if a character has no place in that encoding.
pub fn encode(text: &str, enc: &str, bom: bool) -> Result<Vec<u8>, ()> {
    let mut out = Vec::with_capacity(text.len() + 3);
    match enc {
        "utf-8" => {
            if bom {
                out.extend_from_slice(&[0xEF, 0xBB, 0xBF]);
            }
            out.extend_from_slice(text.as_bytes());
        }
        "utf-16le" | "utf-16be" => {
            let le = enc == "utf-16le";
            if bom {
                out.extend_from_slice(if le { &[0xFF, 0xFE] } else { &[0xFE, 0xFF] });
            }
            for u in text.encode_utf16() {
                out.extend_from_slice(&if le { u.to_le_bytes() } else { u.to_be_bytes() });
            }
        }
        other => {
            let encoding = by_label(other).ok_or(())?;
            let mut encoder = encoding.new_encoder();
            let mut buf =
                vec![0u8; encoder.max_buffer_length_from_utf8_without_replacement(text.len()).ok_or(())? + 16];
            let (result, _, written) = encoder.encode_from_utf8_without_replacement(text, &mut buf, true);
            if !matches!(result, encoding_rs::EncoderResult::InputEmpty) {
                return Err(());
            }
            out.extend_from_slice(&buf[..written]);
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gbk_is_found_and_comes_back_byte_for_byte() {
        let (bytes, _, _) = encoding_rs::GBK
            .encode("姓名,部门,入职日期\n张伟,市场部,2019-03-01\n李娜,财务部,2020-07-15\n王芳,人事部,2021-11-30\n");
        let d = decode(&bytes, None);
        assert_eq!(d.enc, "gbk");
        assert!(d.text.starts_with("姓名,部门"));
        assert!(d.lossless);
        assert_eq!(encode(&d.text, "gbk", false).unwrap(), bytes.into_owned());
    }

    #[test]
    fn utf8_bom_and_utf16_are_kept() {
        let d = decode(b"\xEF\xBB\xBFa,b\r\n1,2\r\n", None);
        assert_eq!((d.enc.as_str(), d.bom, d.lossless, d.text.as_str()), ("utf-8", true, true, "a,b\r\n1,2\r\n"));
        let mut u16 = vec![0xFF, 0xFE];
        for u in "名,值\n".encode_utf16() {
            u16.extend_from_slice(&u.to_le_bytes());
        }
        let d = decode(&u16, None);
        assert_eq!((d.enc.as_str(), d.bom, d.lossless, d.text.as_str()), ("utf-16le", true, true, "名,值\n"));
    }

    #[test]
    fn a_character_gbk_has_no_place_for_fails() {
        assert!(encode("emoji 😀", "gbk", false).is_err());
        assert!(encode("中文", "gbk", false).is_ok());
    }
}
