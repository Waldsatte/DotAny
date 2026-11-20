//! DOS text art: .nfo and .diz (release notes drawn with box and block characters) and .ans (the same with ANSI colour
//! codes). They are written in IBM code page 437, which `encoding_rs` doesn't have (the WHATWG list leaves it out), so its
//! 256 characters are here. View-only: Text shows them and saves them as UTF-8 plain text, never back.
//!
//! What a DOS viewer did, done here: the bytes stop at DOS's end of file, a Ctrl-Z (0x1A) that ends the file or comes
//! right before an artist's tool's record (SAUCE, with its comments COMNT), a record of its own that isn't part of the
//! picture and isn't read. A Ctrl-Z anywhere else is CP437's arrow → and is drawn as one. Control bytes that aren't line
//! endings or tabs are drawn as CP437's little pictures (☺ ♥ ►…), and an escape (0x1B) is kept for an .ans so the page can
//! read its colours. A file that is already Unicode (a UTF-8 mark, or valid UTF-8 with letters beyond ASCII) is read as
//! UTF-8: newer release notes are written that way.

/// Which kind of art a file name is: "nfo" (also .diz) or "ans".
pub fn kind_of(path: &std::path::Path) -> Option<&'static str> {
    let ext = path.extension()?.to_string_lossy().to_ascii_lowercase();
    match ext.as_str() {
        "nfo" | "diz" => Some("nfo"),
        "ans" => Some("ans"),
        _ => None,
    }
}

/// 0x00–0x1F and 0x7F as CP437 draws them.
const LOW: [char; 32] = [
    ' ', '☺', '☻', '♥', '♦', '♣', '♠', '•', '◘', '○', '◙', '♂', '♀', '♪', '♫', '☼', '►', '◄', '↕', '‼', '¶', '§', '▬',
    '↨', '↑', '↓', '→', '←', '∟', '↔', '▲', '▼',
];

/// 0x80–0xFF.
const HIGH: [char; 128] = [
    'Ç', 'ü', 'é', 'â', 'ä', 'à', 'å', 'ç', 'ê', 'ë', 'è', 'ï', 'î', 'ì', 'Ä', 'Å', //
    'É', 'æ', 'Æ', 'ô', 'ö', 'ò', 'û', 'ù', 'ÿ', 'Ö', 'Ü', '¢', '£', '¥', '₧', 'ƒ', //
    'á', 'í', 'ó', 'ú', 'ñ', 'Ñ', 'ª', 'º', '¿', '⌐', '¬', '½', '¼', '¡', '«', '»', //
    '░', '▒', '▓', '│', '┤', '╡', '╢', '╖', '╕', '╣', '║', '╗', '╝', '╜', '╛', '┐', //
    '└', '┴', '┬', '├', '─', '┼', '╞', '╟', '╚', '╔', '╩', '╦', '╠', '═', '╬', '╧', //
    '╨', '╤', '╥', '╙', '╘', '╒', '╓', '╫', '╪', '┘', '┌', '█', '▄', '▌', '▐', '▀', //
    'α', 'ß', 'Γ', 'π', 'Σ', 'σ', 'µ', 'τ', 'Φ', 'Θ', 'Ω', 'δ', '∞', 'φ', 'ε', '∩', //
    '≡', '±', '≥', '≤', '⌠', '⌡', '÷', '≈', '°', '∙', '·', '√', 'ⁿ', '²', '■', '\u{A0}',
];

/// One CP437 byte as Unicode. `escape`: keep 0x1B as itself (an .ans reads it), else draw it (←).
pub fn char_of(b: u8, escape: bool) -> char {
    match b {
        b'\t' | b'\n' | b'\r' => b as char,
        0x1B if escape => '\u{1B}',
        0x00..=0x1F => LOW[b as usize],
        0x7F => '⌂',
        0x20..=0x7E => b as char,
        _ => HIGH[(b - 0x80) as usize],
    }
}

/// The file's words. For "nfo" line endings become `\n`; for "ans" they stay, as carriage return and line feed move the
/// cursor differently there.
pub fn decode(bytes: &[u8], kind: &str) -> String {
    let ansi = kind == "ans";
    let bytes = &bytes[..end_of_file(bytes)];
    let text = if let Some(rest) = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]) {
        String::from_utf8_lossy(rest).into_owned()
    } else {
        match std::str::from_utf8(bytes) {
            Ok(s) if !s.is_ascii() => s.to_string(),
            _ => bytes.iter().map(|&b| char_of(b, ansi)).collect(),
        }
    };
    if ansi { text } else { text.replace("\r\n", "\n").replace('\r', "\n") }
}

/// Where the art ends: at the first Ctrl-Z followed by nothing (or only more Ctrl-Z and NUL padding) or by a SAUCE or
/// COMNT record; otherwise the whole file, its Ctrl-Zs being arrows.
fn end_of_file(bytes: &[u8]) -> usize {
    (0..bytes.len())
        .find(|&i| {
            let rest = &bytes[i + 1..];
            bytes[i] == 0x1A
                && (rest.starts_with(b"SAUCE")
                    || rest.starts_with(b"COMNT")
                    || rest.iter().all(|&b| b == 0x1A || b == 0))
        })
        .unwrap_or(bytes.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_byte_is_one_character() {
        let all: Vec<u8> = (0u8..=255).filter(|&b| b != 0x1A).collect();
        let s = decode(&all, "ans");
        assert_eq!(s.chars().count(), 255);
        // the box and block characters an .nfo is drawn with
        assert_eq!(decode(&[0xC9, 0xCD, 0xBB, 0xB0, 0xB1, 0xB2, 0xDB, 0xDC, 0xDF], "nfo"), "╔═╗░▒▓█▄▀");
        assert_eq!(decode(&[0x01, 0x03, 0x10, 0x7F, 0xFE, 0xFF], "nfo"), "☺♥►⌂■\u{A0}");
    }

    #[test]
    fn stops_at_dos_end_of_file() {
        let mut b = b"\xDB\xDB hi\r\n".to_vec();
        b.extend_from_slice(b"\x1ASAUCE00 title");
        assert_eq!(decode(&b, "nfo"), "██ hi\n");
        assert_eq!(decode(b"end\r\n\x1A\x1A\0\0", "nfo"), "end\n");
        assert_eq!(decode(b"x\x1ACOMNTa comment\x1ASAUCE00", "nfo"), "x");
    }

    #[test]
    fn an_arrow_in_the_middle_is_an_arrow() {
        assert_eq!(decode(b"\x18 \x19 \x1A \x1B\r\nnext\r\n", "nfo"), "↑ ↓ → ←\nnext\n");
        assert_eq!(decode(b"go \x1A there\x1ASAUCE00", "nfo"), "go → there");
    }

    #[test]
    fn escape_kept_only_for_ansi() {
        assert_eq!(decode(b"\x1b[31mA", "ans"), "\u{1b}[31mA");
        assert_eq!(decode(b"\x1b[31mA", "nfo"), "←[31mA");
        assert_eq!(decode(b"a\r\nb", "ans"), "a\r\nb");
    }

    #[test]
    fn unicode_files_stay_unicode() {
        assert_eq!(decode("\u{FEFF}╔═╗".as_bytes(), "nfo"), "╔═╗");
        assert_eq!(decode("╔═╗ ok".as_bytes(), "nfo"), "╔═╗ ok");
        // plain ASCII is the same either way
        assert_eq!(decode(b"just words", "nfo"), "just words");
    }

    #[test]
    fn names() {
        use std::path::Path;
        assert_eq!(kind_of(Path::new("x/README.NFO")), Some("nfo"));
        assert_eq!(kind_of(Path::new("FILE_ID.DIZ")), Some("nfo"));
        assert_eq!(kind_of(Path::new("logo.ans")), Some("ans"));
        assert_eq!(kind_of(Path::new("notes.txt")), None);
    }
}
