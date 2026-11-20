//! Word: the paragraphs the window found on each page, with their size, weight and colour, one page after another; a
//! page with no words on it (a scan) becomes its picture. A .docx is a ZIP of a few XML parts, written here by hand.

use serde::Deserialize;
use std::collections::HashMap;
use std::io::{Cursor, Write};
use zip::write::SimpleFileOptions;

#[derive(Deserialize)]
pub struct Para {
    pub text: String,
    pub size: f32,
    #[serde(default)]
    pub bold: bool,
    #[serde(default)]
    pub serif: bool,
    #[serde(default)]
    pub color: String,
}

#[derive(Deserialize)]
pub struct Page {
    pub w: f32,
    pub h: f32,
    #[serde(default)]
    pub paras: Vec<Para>,
    /// a blob's id: the page drawn as a picture
    #[serde(default)]
    pub picture: Option<String>,
    #[serde(default)]
    pub pw: u32,
    #[serde(default)]
    pub ph: u32,
}

#[derive(Deserialize)]
pub struct Book {
    pub pages: Vec<Page>,
}

fn esc(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => o.push_str("&amp;"),
            '<' => o.push_str("&lt;"),
            '>' => o.push_str("&gt;"),
            '"' => o.push_str("&quot;"),
            // characters XML 1.0 can't carry
            c if (c as u32) < 0x20 && c != '\t' => {}
            c => o.push(c),
        }
    }
    o
}

const MARGIN_PT: f32 = 54.0;

pub fn write(book: &Book, blobs: &HashMap<String, Vec<u8>>) -> Result<Vec<u8>, String> {
    let first = book.pages.first().map(|p| (p.w, p.h)).unwrap_or((595.0, 842.0));
    let mut body = String::new();
    let mut media: Vec<(String, Vec<u8>)> = Vec::new();
    for (i, p) in book.pages.iter().enumerate() {
        if i > 0 {
            body.push_str(r#"<w:p><w:r><w:br w:type="page"/></w:r></w:p>"#);
        }
        if let Some(blob) = p.picture.as_ref().and_then(|b| blobs.get(b)) {
            let n = media.len() + 1;
            let rid = format!("rIdImg{n}");
            let avail = (first.0 - 2.0 * MARGIN_PT).max(72.0);
            let tall = (first.1 - 2.0 * MARGIN_PT).max(72.0);
            let (pw, ph) = (p.pw.max(1) as f32, p.ph.max(1) as f32);
            let k = (avail / pw).min(tall / ph);
            let (cx, cy) = ((pw * k * 12700.0) as i64, (ph * k * 12700.0) as i64);
            body.push_str(&format!(
                r#"<w:p><w:r><w:drawing><wp:inline distT="0" distB="0" distL="0" distR="0"><wp:extent cx="{cx}" cy="{cy}"/><wp:docPr id="{n}" name="Page {n}"/><a:graphic xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main"><a:graphicData uri="http://schemas.openxmlformats.org/drawingml/2006/picture"><pic:pic xmlns:pic="http://schemas.openxmlformats.org/drawingml/2006/picture"><pic:nvPicPr><pic:cNvPr id="{n}" name="page{n}.png"/><pic:cNvPicPr/></pic:nvPicPr><pic:blipFill><a:blip r:embed="{rid}"/><a:stretch><a:fillRect/></a:stretch></pic:blipFill><pic:spPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="{cx}" cy="{cy}"/></a:xfrm><a:prstGeom prst="rect"><a:avLst/></a:prstGeom></pic:spPr></pic:pic></a:graphicData></a:graphic></wp:inline></w:drawing></w:r></w:p>"#
            ));
            let ext = if blob.starts_with(&[0xFF, 0xD8]) { "jpeg" } else { "png" };
            media.push((format!("image{n}.{ext}"), blob.clone()));
            continue;
        }
        for para in &p.paras {
            let (latin, east) = if para.serif { ("Times New Roman", "SimSun") } else { ("Arial", "Microsoft YaHei") };
            let half_points = (para.size * 2.0).round().clamp(8.0, 400.0) as i64;
            let color = para.color.trim_start_matches('#');
            let color = if color.len() == 6 && color.chars().all(|c| c.is_ascii_hexdigit()) {
                color.to_uppercase()
            } else {
                "000000".into()
            };
            body.push_str(&format!(
                r#"<w:p><w:pPr><w:spacing w:after="{}"/></w:pPr><w:r><w:rPr><w:rFonts w:ascii="{latin}" w:hAnsi="{latin}" w:eastAsia="{east}" w:cs="{latin}"/>{}<w:color w:val="{color}"/><w:sz w:val="{half_points}"/><w:szCs w:val="{half_points}"/></w:rPr><w:t xml:space="preserve">{}</w:t></w:r></w:p>"#,
                (para.size * 8.0).round() as i64,
                if para.bold { "<w:b/><w:bCs/>" } else { "" },
                esc(&para.text)
            ));
        }
        if p.paras.is_empty() {
            body.push_str("<w:p/>");
        }
    }
    let twips = |pt: f32| (pt * 20.0).round() as i64;
    let orient = if first.0 > first.1 { r#" w:orient="landscape""# } else { "" };
    let doc = format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" xmlns:wp="http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing"><w:body>{body}<w:sectPr><w:pgSz w:w="{}" w:h="{}"{orient}/><w:pgMar w:top="{m}" w:right="{m}" w:bottom="{m}" w:left="{m}" w:header="0" w:footer="0" w:gutter="0"/></w:sectPr></w:body></w:document>"#,
        twips(first.0),
        twips(first.1),
        m = twips(MARGIN_PT)
    );
    let mut rels = String::from(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rIdStyles" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles" Target="styles.xml"/>"#,
    );
    for (k, (name, _)) in media.iter().enumerate() {
        rels.push_str(&format!(r#"<Relationship Id="rIdImg{}" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/image" Target="media/{name}"/>"#, k + 1));
    }
    rels.push_str("</Relationships>");
    let types = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Default Extension="png" ContentType="image/png"/><Default Extension="jpeg" ContentType="image/jpeg"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/><Override PartName="/word/styles.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.styles+xml"/></Types>"#;
    let root_rels = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>"#;
    let styles = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:styles xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:docDefaults><w:rPrDefault><w:rPr><w:rFonts w:ascii="Arial" w:hAnsi="Arial" w:eastAsia="Microsoft YaHei" w:cs="Arial"/><w:sz w:val="22"/><w:lang w:val="en-US" w:eastAsia="zh-CN"/></w:rPr></w:rPrDefault><w:pPrDefault><w:pPr><w:spacing w:after="120" w:line="276" w:lineRule="auto"/></w:pPr></w:pPrDefault></w:docDefaults><w:style w:type="paragraph" w:default="1" w:styleId="Normal"><w:name w:val="Normal"/></w:style></w:styles>"#;

    let mut out = Cursor::new(Vec::new());
    {
        let mut z = zip::ZipWriter::new(&mut out);
        let opt = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        let stored = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
        let mut put = |name: &str, data: &[u8], o: SimpleFileOptions| -> Result<(), String> {
            z.start_file(name, o).map_err(|e| e.to_string())?;
            z.write_all(data).map_err(|e| e.to_string())
        };
        put("[Content_Types].xml", types.as_bytes(), opt)?;
        put("_rels/.rels", root_rels.as_bytes(), opt)?;
        put("word/document.xml", doc.as_bytes(), opt)?;
        put("word/styles.xml", styles.as_bytes(), opt)?;
        put("word/_rels/document.xml.rels", rels.as_bytes(), opt)?;
        for (name, data) in &media {
            put(&format!("word/media/{name}"), data, stored)?;
        }
        z.finish().map_err(|e| e.to_string())?;
    }
    Ok(out.into_inner())
}
