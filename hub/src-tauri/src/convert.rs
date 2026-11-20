//! The one place the converter's work comes from: `dotany-convert` (shared/convert). The page speaks the crate's own
//! words for families, refusals and reasons (they serialise under the names the page reads), so this only reads the
//! page's choices.

pub use dotany_convert::*;

/// The page's choices: quality 10–100, tone normal | high | top, pack fast | normal | small; `picture` is the word for a
/// picture written beside a Markdown file, in the language on screen (`图{n}`, `Figure {n}`); `lang` is that language's
/// code, for the words a converted document carries where a block can't be carried.
pub fn options(quality: u8, tone: &str, pack: &str, picture: &str, lang: &str) -> Options {
    Options {
        quality: quality.clamp(10, 100),
        tone: match tone {
            "normal" => Tone::Normal,
            "top" => Tone::Top,
            _ => Tone::High,
        },
        pack: match pack {
            "fast" => Pack::Fast,
            "small" => Pack::Small,
            _ => Pack::Normal,
        },
        picture: if picture.trim().is_empty() { Options::default().picture } else { picture.to_string() },
        lang: if lang.trim().is_empty() { Options::default().lang } else { lang.to_string() },
    }
}
