//! The pictures of a Word document turned into Markdown: written beside the Markdown file as `<its name> 图1.png`,
//! `<its name> 图2.jpg`, … (the word is the language's: 图, Figure, 図…) and linked from it by relative paths. Nothing
//! is overwritten: a name already taken gives way to `… 图1 (2).png`, the way the converter names everything it makes.
//!
//! Plain std on purpose: DotAny Text's Save As uses it through dotany-base, and the hub's converter (shared/convert)
//! includes this very file, so both write the same names and the same links.

use std::collections::HashSet;
use std::io;
use std::path::Path;

/// Characters no file name may hold on any system the family runs on, and the path separators.
const NOT_IN_NAMES: &[char] = &['<', '>', ':', '"', '/', '\\', '|', '?', '*'];

/// The word for "picture n" from the language's table: `图{n}`, `Figure {n}`. A word without `{n}` gets the number after it.
pub fn picture_word(template: &str, n: usize) -> String {
    let template: String = template.chars().filter(|c| !NOT_IN_NAMES.contains(c) && !c.is_control()).collect();
    let template = template.trim();
    let template = if template.is_empty() { "图{n}" } else { template };
    if template.contains("{n}") { template.replace("{n}", &n.to_string()) } else { format!("{template}{n}") }
}

/// A picture's extension from its part name (`media/image1.jpeg`) or its type (`image/png`): jpeg is written jpg.
pub fn picture_ext(name_or_type: &str) -> Option<&'static str> {
    let tail = name_or_type.rsplit(['.', '/']).next().unwrap_or("").to_ascii_lowercase();
    Some(match tail.as_str() {
        "png" => "png",
        "jpg" | "jpeg" => "jpg",
        "gif" => "gif",
        "bmp" => "bmp",
        "webp" => "webp",
        _ => return None,
    })
}

/// `stem.ext` when it is free in `dir` and not among `taken` (lower case), else `stem (2).ext`, `stem (3).ext`…
pub fn free_name(dir: &Path, stem: &str, ext: &str, taken: &HashSet<String>) -> String {
    for i in 1..100_000u32 {
        let name = if i == 1 { format!("{stem}.{ext}") } else { format!("{stem} ({i}).{ext}") };
        if !taken.contains(&name.to_lowercase()) && !dir.join(&name).exists() {
            return name;
        }
    }
    format!("{stem} ({}).{ext}", std::process::id())
}

/// The names for a document's pictures, in its order: `<md stem> 图1.png`, `<md stem> 图2.jpg`…, each free in `dir`.
/// `exts` are the pictures' extensions; `taken` holds names already promised to something else (the Markdown file).
pub fn picture_names(dir: &Path, md_stem: &str, template: &str, exts: &[&str], taken: &HashSet<String>) -> Vec<String> {
    let mut taken = taken.clone();
    exts.iter()
        .enumerate()
        .map(|(i, ext)| {
            let name = free_name(dir, &format!("{md_stem} {}", picture_word(template, i + 1)), ext, &taken);
            taken.insert(name.to_lowercase());
            name
        })
        .collect()
}

/// A file beside the Markdown, as a Markdown link: the name with what a link can't hold written as %XX (spaces,
/// brackets, %, #). Letters of any language stay as they are, as Markdown editors write them.
pub fn link(name: &str) -> String {
    let mut out = String::with_capacity(name.len() + 8);
    for c in name.chars() {
        if c.is_whitespace() || c.is_control() || "()[]<>%#`\\^{}|\"'".contains(c) {
            let mut buf = [0u8; 4];
            for b in c.encode_utf8(&mut buf).bytes() {
                out.push_str(&format!("%{b:02X}"));
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// A link read back as a file name: %XX decoded where it makes UTF-8, everything else left as written.
pub fn unlink(link: &str) -> String {
    let b = link.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        let hex = |c: u8| (c as char).to_digit(16);
        if b[i] == b'%' && i + 2 < b.len() {
            if let (Some(h), Some(l)) = (hex(b[i + 1]), hex(b[i + 2])) {
                out.push((h * 16 + l) as u8);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8(out).unwrap_or_else(|_| link.to_string())
}

/// Writes a new file in `dir` under `name`, or under the next free name if it has been taken meanwhile. Never replaces
/// anything. Returns the name used.
pub fn write_new(dir: &Path, name: &str, bytes: &[u8]) -> io::Result<String> {
    use std::io::Write;
    let (stem, ext) = name.rsplit_once('.').unwrap_or((name, ""));
    let mut tried = HashSet::new();
    for _ in 0..1000 {
        let candidate = if tried.is_empty() { name.to_string() } else { free_name(dir, stem, ext, &tried) };
        match std::fs::OpenOptions::new().write(true).create_new(true).open(dir.join(&candidate)) {
            Ok(mut f) => {
                if let Err(e) = f.write_all(bytes).and_then(|_| f.flush()) {
                    drop(f);
                    let _ = std::fs::remove_file(dir.join(&candidate));
                    return Err(e);
                }
                return Ok(candidate);
            }
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {
                tried.insert(candidate.to_lowercase());
            }
            Err(e) => return Err(e),
        }
    }
    Err(io::Error::new(io::ErrorKind::AlreadyExists, "no free name"))
}

/// A picture to write beside a Markdown file: the address the Markdown has for it now, its bytes, its extension.
pub struct Picture {
    pub src: String,
    pub bytes: Vec<u8>,
    pub ext: &'static str,
}

/// Text's Save As: `markdown` written at `target` by `write` (the family's atomic save), each picture first written
/// beside it under a free name (`picture_names`) and every `](<its src>)` in the text turned into a link to that file.
/// Nothing is replaced but the Markdown file itself (the save dialog has asked); if anything fails, the pictures this
/// call wrote are removed again. Returns the pictures' file names, in order.
pub fn write_markdown<E>(
    target: &Path,
    markdown: &str,
    pictures: &[Picture],
    template: &str,
    write: impl FnOnce(&Path, &[u8]) -> Result<(), E>,
) -> Result<Vec<String>, WriteError<E>> {
    let dir = target.parent().unwrap_or(Path::new(""));
    let stem = target.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let md_name = target.file_name().map(|s| s.to_string_lossy().to_lowercase()).unwrap_or_default();
    let exts: Vec<&str> = pictures.iter().map(|p| p.ext).collect();
    let names = picture_names(dir, &stem, template, &exts, &HashSet::from([md_name]));
    let mut written: Vec<String> = Vec::new();
    let mut text = markdown.to_string();
    let undo = |written: &[String]| {
        for w in written {
            let _ = std::fs::remove_file(dir.join(w));
        }
    };
    for (p, name) in pictures.iter().zip(&names) {
        match write_new(dir, name, &p.bytes) {
            Ok(used) => {
                text = text.replace(&format!("]({})", p.src), &format!("]({})", link(&used)));
                written.push(used);
            }
            Err(e) => {
                undo(&written);
                return Err(WriteError::Picture(e));
            }
        }
    }
    if let Err(e) = write(target, text.as_bytes()) {
        undo(&written);
        return Err(WriteError::Markdown(e));
    }
    Ok(written)
}

/// What stopped `write_markdown`: a picture (an io error) or the Markdown file (the writer's own error).
#[derive(Debug)]
pub enum WriteError<E> {
    Picture(io::Error),
    Markdown(E),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn markdown_with_its_pictures() {
        let d = scratch("markdown");
        let target = d.join("报告.md");
        std::fs::write(d.join("报告 图1.png"), b"someone else's").unwrap();
        let pictures = [
            Picture { src: "docx:rId5".into(), bytes: b"png".to_vec(), ext: "png" },
            Picture { src: "data:image/jpeg;base64,AAAA".into(), bytes: b"jpg".to_vec(), ext: "jpg" },
        ];
        let md = "# 标题\n\n![一](docx:rId5)\n\n![二](data:image/jpeg;base64,AAAA)\n\n![一](docx:rId5)\n";
        let names = write_markdown(&target, md, &pictures, "图{n}", |p, b| std::fs::write(p, b)).unwrap();
        assert_eq!(names, ["报告 图1 (2).png", "报告 图2.jpg"]);
        assert_eq!(
            std::fs::read_to_string(&target).unwrap(),
            "# 标题\n\n![一](报告%20图1%20%282%29.png)\n\n![二](报告%20图2.jpg)\n\n![一](报告%20图1%20%282%29.png)\n"
        );
        assert_eq!(std::fs::read(d.join("报告 图1.png")).unwrap(), b"someone else's");
        assert_eq!(std::fs::read(d.join("报告 图2.jpg")).unwrap(), b"jpg");
        // English names its pictures in English
        let names = write_markdown(&d.join("Report.md"), "![](docx:rId5)", &pictures[..1], "Figure {n}", |p, b| {
            std::fs::write(p, b)
        })
        .unwrap();
        assert_eq!(names, ["Report Figure 1.png"]);
        // a Markdown file that can't be written leaves no pictures behind
        let before = std::fs::read_dir(&d).unwrap().count();
        let failed = write_markdown(&d.join("x.md"), "![](docx:rId5)", &pictures, "图{n}", |_, _| Err("no room"));
        assert!(matches!(failed, Err(WriteError::Markdown("no room"))));
        assert_eq!(std::fs::read_dir(&d).unwrap().count(), before);
        let _ = std::fs::remove_dir_all(&d);
    }

    fn scratch(tag: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("dotany-beside-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn words_per_language() {
        assert_eq!(picture_word("图{n}", 1), "图1");
        assert_eq!(picture_word("Figure {n}", 12), "Figure 12");
        assert_eq!(picture_word("Abbildung", 3), "Abbildung3");
        assert_eq!(picture_word("a/b:{n}", 2), "ab2");
        assert_eq!(picture_word("", 4), "图4");
    }

    #[test]
    fn extensions() {
        assert_eq!(picture_ext("media/image1.jpeg"), Some("jpg"));
        assert_eq!(picture_ext("image/png"), Some("png"));
        assert_eq!(picture_ext("media/image2.PNG"), Some("png"));
        assert_eq!(picture_ext("media/image3.emf"), None);
    }

    #[test]
    fn names_never_take_an_existing_file() {
        let d = scratch("names");
        std::fs::write(d.join("报告 图1.png"), b"old").unwrap();
        std::fs::write(d.join("报告 图2 (2).jpg"), b"old").unwrap();
        std::fs::write(d.join("报告 图2.jpg"), b"old").unwrap();
        let names = picture_names(&d, "报告", "图{n}", &["png", "jpg", "gif"], &HashSet::new());
        assert_eq!(names, ["报告 图1 (2).png", "报告 图2 (3).jpg", "报告 图3.gif"]);
        // two pictures that would land on the same name in the same go are told apart
        let mut taken = HashSet::new();
        taken.insert("x 图1.png".to_string());
        assert_eq!(picture_names(&d, "x", "图{n}", &["png"], &taken), ["x 图1 (2).png"]);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn links_round_trip() {
        assert_eq!(link("报告 图1.png"), "报告%20图1.png");
        assert_eq!(link("Q3 (final) Figure 1 (2).png"), "Q3%20%28final%29%20Figure%201%20%282%29.png");
        assert_eq!(link("50% #1.png"), "50%25%20%231.png");
        for n in ["报告 图1.png", "Q3 (final) Figure 1 (2).png", "50% #1.png", "a%zz.png", "end%"] {
            assert_eq!(unlink(&link(n)), n);
        }
        assert_eq!(unlink("a%zz.png"), "a%zz.png");
        assert_eq!(unlink("end%2"), "end%2");
    }

    #[test]
    fn write_new_never_replaces() {
        let d = scratch("write");
        std::fs::write(d.join("a 图1.png"), b"old").unwrap();
        let used = write_new(&d, "a 图1.png", b"new").unwrap();
        assert_eq!(used, "a 图1 (2).png");
        assert_eq!(std::fs::read(d.join("a 图1.png")).unwrap(), b"old");
        assert_eq!(std::fs::read(d.join(&used)).unwrap(), b"new");
        let _ = std::fs::remove_dir_all(&d);
    }
}
