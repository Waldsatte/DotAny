//! The two words a converted document carries for a block it cannot carry: a picture it has no room for (`图像`) and any
//! other block (`暂不支持渲染的元素`). DotAny Text shows these in the interface language, looked up in its language
//! tables by the Chinese sentence as the key; here the nine languages' sentences are kept as a table of their own, and a
//! unit test fails when one drifts from Text's tables (`text/src/lang/<code>.json`).

/// The interface languages' codes, what each says for a picture and for any other block it cannot carry. Chinese
/// (`zh`) is the key itself.
const WORDS: [(&str, &str, &str); 9] = [
    ("zh", "图像", "暂不支持渲染的元素"),
    ("zh-Hant", "影像", "尚不支援呈現的元素"),
    ("en", "Image", "Unsupported Elements"),
    ("ja", "画像", "まだ表示できない要素"),
    ("ko", "이미지", "아직 표시할 수 없는 요소"),
    ("es-419", "Imagen", "Elementos que todavía no se pueden mostrar"),
    ("pt-BR", "Imagem", "Elementos ainda não exibidos"),
    ("fr", "Image", "Éléments non affichables"),
    ("de", "Bild", "Nicht unterstützte Elemente"),
];

/// The words in the language with this code (`zh`, `zh-Hant`, `en`, `ja`, `ko`, `es-419`, `pt-BR`, `fr`, `de`): the
/// picture's, or the other blocks'. A code it doesn't know, or an empty one, is English.
pub fn label(lang: &str, picture: bool) -> &'static str {
    let (_, pic, other) = WORDS.iter().find(|w| w.0 == lang).unwrap_or(&WORDS[2]);
    if picture { pic } else { other }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A picture and another block the converter cannot carry, written to Markdown and to plain text, come out in the
    /// language asked for, whatever that is; a block with words of its own keeps them.
    #[test]
    fn a_converted_document_speaks_the_interface_language() {
        use super::super::md::{blocks_to_doc, doc_to_plain, serialize_markdown};
        use dotany_words::docx::Block;
        let locked = |id: u32, kind: &str, text: &str| Block::Locked {
            id,
            kind: kind.into(),
            text: text.into(),
            style: None,
            rows: Vec::new(),
        };
        let blocks = [locked(1, "picture", ""), locked(2, "other", ""), locked(3, "paragraph", "its own words")];
        for (code, picture, other) in WORDS {
            let doc = blocks_to_doc(&blocks, code);
            let want = format!("{picture}\n{other}\nits own words\n");
            assert_eq!(doc_to_plain(&doc), want, "{code}: plain text");
            assert_eq!(
                serialize_markdown(&doc),
                format!("{picture}\n\n{other}\n\nits own words\n"),
                "{code}: Markdown"
            );
        }
        // no language given: English
        assert_eq!(doc_to_plain(&blocks_to_doc(&blocks, "")), "Image\nUnsupported Elements\nits own words\n");
    }

    #[test]
    fn an_unknown_language_is_english() {
        assert_eq!(label("", true), "Image");
        assert_eq!(label("tlh", false), "Unsupported Elements");
        assert_eq!(label("ja", true), "画像");
        assert_eq!(label("zh", false), "暂不支持渲染的元素");
    }

    /// The words are Text's own: each language's sentence for the two keys is what `text/src/lang/<code>.json` says,
    /// and Chinese is the key. Where Text's folder is not there, this ends at once.
    #[test]
    fn the_words_are_texts_own() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../text/src/lang");
        if !dir.is_dir() {
            eprintln!("skipped: {} is not here", dir.display());
            return;
        }
        for (code, picture, other) in WORDS {
            if code == "zh" {
                assert_eq!((picture, other), ("图像", "暂不支持渲染的元素"));
                continue;
            }
            let file = dir.join(format!("{code}.json"));
            let table: serde_json::Value = serde_json::from_slice(&std::fs::read(&file).unwrap()).unwrap();
            assert_eq!(table["图像"].as_str(), Some(picture), "{code}: the picture's word");
            assert_eq!(table["暂不支持渲染的元素"].as_str(), Some(other), "{code}: the other blocks' word");
        }
        // and no language Text has is missing here
        let mut have: Vec<String> = std::fs::read_dir(&dir)
            .unwrap()
            .flatten()
            .filter_map(|e| e.file_name().to_string_lossy().strip_suffix(".json").map(str::to_string))
            .collect();
        have.push("zh".into());
        have.sort();
        let mut ours: Vec<String> = WORDS.iter().map(|w| w.0.to_string()).collect();
        ours.sort();
        assert_eq!(ours, have);
    }
}
