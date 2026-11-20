//! New words in a PDF: the same fonts the window shows them in (Stack Sans for Latin, HarmonyOS Sans SC for the rest,
//! Noto Serif SC from Windows for a serif paragraph when it's there), embedded as subsets with a ToUnicode map so the
//! words can be found and copied again. Glyphs are numbered by the subset; the CID is the new glyph number.

use lopdf::{Dictionary, Document, Object, ObjectId, Stream, dictionary};
use std::collections::BTreeMap;
use std::sync::Arc;
use subsetter::GlyphRemapper;
use ttf_parser::{Face, GlyphId, Tag};

/// The font files the page handed over, by name: "sans", "sans-bold" (HarmonyOS), "latin" (Stack Sans, variable)
pub type Files = BTreeMap<String, Arc<Vec<u8>>>;

struct One {
    data: Arc<Vec<u8>>,
    name: &'static str,
    wght: Option<f32>,
    map: GlyphRemapper,
    /// new glyph -> the text it stands for, and its width in thousandths of the size
    uni: BTreeMap<u16, String>,
    widths: BTreeMap<u16, f32>,
    id: Option<ObjectId>,
    upem: f32,
    ascent: f32,
    descent: f32,
    cap: f32,
    bbox: [f32; 4],
    used: bool,
}

impl One {
    fn new(data: Arc<Vec<u8>>, name: &'static str, wght: Option<f32>) -> Option<One> {
        let mut face = Face::parse(&data, 0).ok()?;
        if let Some(w) = wght {
            face.set_variation(Tag::from_bytes(b"wght"), w);
        }
        let upem = face.units_per_em() as f32;
        let k = 1000.0 / upem;
        let b = face.global_bounding_box();
        Some(One {
            name,
            wght,
            map: GlyphRemapper::new(),
            uni: BTreeMap::new(),
            widths: BTreeMap::new(),
            id: None,
            upem,
            ascent: face.ascender() as f32 * k,
            descent: face.descender() as f32 * k,
            cap: face.capital_height().unwrap_or(face.ascender()) as f32 * k,
            bbox: [b.x_min as f32 * k, b.y_min as f32 * k, b.x_max as f32 * k, b.y_max as f32 * k],
            used: false,
            data,
        })
    }
    fn face(&self) -> Face<'_> {
        let mut f = Face::parse(&self.data, 0).expect("parsed before");
        if let Some(w) = self.wght {
            f.set_variation(Tag::from_bytes(b"wght"), w);
        }
        f
    }
}

/// A piece of text in one font: the new glyph numbers and how wide it is at size 1.
pub struct Run {
    pub font: usize,
    pub gids: Vec<u16>,
    pub chars: Vec<char>,
    /// each glyph's width at size 1
    pub adv: Vec<f32>,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Style {
    pub bold: bool,
    pub serif: bool,
}

pub struct Fonts {
    list: Vec<One>,
    /// which fonts to try, in order, for each style
    chains: [Vec<usize>; 4],
}

fn key(s: Style) -> usize {
    (s.bold as usize) + 2 * (s.serif as usize)
}

impl Fonts {
    pub fn new(files: &Files) -> Fonts {
        let mut list = Vec::new();
        let mut add = |data: Option<&Arc<Vec<u8>>>, name: &'static str, w: Option<f32>| -> Option<usize> {
            let one = One::new(data?.clone(), name, w)?;
            list.push(one);
            Some(list.len() - 1)
        };
        let latin = add(files.get("latin"), "StackSansText", Some(400.0));
        let latin_b = add(files.get("latin"), "StackSansText-Bold", Some(700.0));
        let sans = add(files.get("sans"), "HarmonyOSSansSC", None);
        let sans_b = add(files.get("sans-bold"), "HarmonyOSSansSC-Bold", None);
        let serif_file = serif_from_windows();
        let serif = add(serif_file.as_ref(), "NotoSerifSC", Some(400.0));
        let serif_b = add(serif_file.as_ref(), "NotoSerifSC-Bold", Some(700.0));
        let chain = |v: &[Option<usize>]| v.iter().flatten().copied().collect::<Vec<_>>();
        let chains = [
            chain(&[latin, sans]),
            chain(&[latin_b, sans_b, sans]),
            chain(&[serif, latin, sans]),
            chain(&[serif_b, latin_b, sans_b, sans]),
        ];
        Fonts { list, chains }
    }

    /// Text cut into runs of one font each, glyphs numbered for the subsets.
    pub fn runs(&mut self, style: Style, text: &str) -> Vec<Run> {
        let chain = self.chains[key(style)].clone();
        let mut out: Vec<Run> = Vec::new();
        for c in text.chars() {
            if c == '\n' || c == '\r' {
                continue;
            }
            let pick = chain
                .iter()
                .copied()
                .find(|&i| self.list[i].face().glyph_index(c).is_some())
                .or_else(|| chain.last().copied());
            let Some(i) = pick else { continue };
            let one = &mut self.list[i];
            let face = Face::parse(&one.data, 0).ok();
            let Some(mut face) = face else { continue };
            if let Some(w) = one.wght {
                face.set_variation(Tag::from_bytes(b"wght"), w);
            }
            let old = face.glyph_index(c).unwrap_or(GlyphId(0));
            let adv = face.glyph_hor_advance(old).unwrap_or(0) as f32 / one.upem;
            let new = one.map.remap(old.0);
            one.uni.entry(new).or_insert_with(|| c.to_string());
            one.widths.insert(new, adv * 1000.0);
            one.used = true;
            match out.last_mut() {
                Some(r) if r.font == i => {
                    r.gids.push(new);
                    r.chars.push(c);
                    r.adv.push(adv);
                }
                _ => out.push(Run { font: i, gids: vec![new], chars: vec![c], adv: vec![adv] }),
            }
        }
        out
    }

    pub fn width(&mut self, style: Style, text: &str) -> f32 {
        self.runs(style, text).iter().flat_map(|r| r.adv.iter()).sum()
    }

    /// The font's resource name, the same in every page and appearance that uses it.
    pub fn res_name(i: usize) -> Vec<u8> {
        format!("DAF{i}").into_bytes()
    }

    /// The font object's number, kept for the whole save so content can refer to it before it exists.
    pub fn id(&mut self, doc: &mut Document, i: usize) -> ObjectId {
        if let Some(id) = self.list[i].id {
            return id;
        }
        let id = doc.new_object_id();
        self.list[i].id = Some(id);
        id
    }

    /// A /Font resource dictionary for the fonts these runs use.
    pub fn resources(&mut self, doc: &mut Document, runs: &[Run]) -> Dictionary {
        let mut d = Dictionary::new();
        for r in runs {
            let id = self.id(doc, r.font);
            d.set(Self::res_name(r.font), Object::Reference(id));
        }
        d
    }

    /// Written once at the end of a save: each font used, cut down to the glyphs that were asked for.
    pub fn finish(&mut self, doc: &mut Document) -> Result<Vec<ObjectId>, String> {
        let mut written = Vec::new();
        for i in 0..self.list.len() {
            let Some(id) = self.list[i].id else { continue };
            let one = &self.list[i];
            let vars: Vec<(subsetter::Tag, f32)> =
                one.wght.map(|w| vec![(subsetter::Tag::new(b"wght"), w)]).unwrap_or_default();
            let data = if vars.is_empty() {
                subsetter::subset(&one.data, 0, &one.map)
            } else {
                subsetter::subset_with_variations(&one.data, 0, &vars, &one.map)
            }
            .map_err(|e| format!("subset: {e:?}"))?;
            let cff = data.starts_with(b"OTTO");
            let tag = subset_tag(i, one.map.num_gids());
            let base = format!("{tag}+{}", one.name);
            let mut file = Stream::new(dictionary! {}, data.clone());
            if cff {
                file.dict.set("Subtype", Object::Name(b"OpenType".to_vec()));
            } else {
                file.dict.set("Length1", data.len() as i64);
            }
            let _ = file.compress();
            let file_id = doc.add_object(file);
            let desc = dictionary! {
                "Type" => "FontDescriptor",
                "FontName" => Object::Name(base.clone().into_bytes()),
                "Flags" => 4,
                "FontBBox" => one.bbox.iter().map(|v| Object::Real(*v)).collect::<Vec<_>>(),
                "ItalicAngle" => 0,
                "Ascent" => Object::Real(one.ascent),
                "Descent" => Object::Real(one.descent),
                "CapHeight" => Object::Real(one.cap),
                "StemV" => 80,
                if cff { "FontFile3" } else { "FontFile2" } => Object::Reference(file_id),
            };
            let desc_id = doc.add_object(desc);
            let mut w = Vec::new();
            for (g, v) in &one.widths {
                w.push(Object::Integer(*g as i64));
                w.push(Object::Array(vec![Object::Real(v.round())]));
            }
            let mut cid = dictionary! {
                "Type" => "Font",
                "Subtype" => if cff { "CIDFontType0" } else { "CIDFontType2" },
                "BaseFont" => Object::Name(base.clone().into_bytes()),
                "CIDSystemInfo" => dictionary! { "Registry" => Object::string_literal("Adobe"), "Ordering" => Object::string_literal("Identity"), "Supplement" => 0 },
                "FontDescriptor" => Object::Reference(desc_id),
                "DW" => 1000,
                "W" => w,
            };
            if !cff {
                cid.set("CIDToGIDMap", "Identity");
            }
            let cid_id = doc.add_object(cid);
            let mut cmap = Stream::new(dictionary! {}, to_unicode(&one.uni));
            let _ = cmap.compress();
            let cmap_id = doc.add_object(cmap);
            doc.objects.insert(
                id,
                Object::Dictionary(dictionary! {
                    "Type" => "Font",
                    "Subtype" => "Type0",
                    "BaseFont" => Object::Name(base.into_bytes()),
                    "Encoding" => "Identity-H",
                    "DescendantFonts" => vec![Object::Reference(cid_id)],
                    "ToUnicode" => Object::Reference(cmap_id),
                }),
            );
            written.push(id);
        }
        Ok(written)
    }
}

/// Six capital letters that say "this font is a subset", different per font and per subset.
fn subset_tag(i: usize, n: u16) -> String {
    let mut v = (i as u32 + 1) * 7919 + n as u32 * 104729;
    (0..6)
        .map(|_| {
            let c = (b'A' + (v % 26) as u8) as char;
            v /= 26;
            v += 17;
            c
        })
        .collect()
}

fn to_unicode(uni: &BTreeMap<u16, String>) -> Vec<u8> {
    let mut s = String::from(
        "/CIDInit /ProcSet findresource begin\n12 dict begin\nbegincmap\n/CIDSystemInfo << /Registry (Adobe) /Ordering (UCS) /Supplement 0 >> def\n/CMapName /Adobe-Identity-UCS def\n/CMapType 2 def\n1 begincodespacerange\n<0000> <FFFF>\nendcodespacerange\n",
    );
    let all: Vec<_> = uni.iter().collect();
    for chunk in all.chunks(100) {
        s.push_str(&format!("{} beginbfchar\n", chunk.len()));
        for (g, t) in chunk {
            let hex: String = t.encode_utf16().map(|u| format!("{u:04X}")).collect();
            s.push_str(&format!("<{:04X}> <{hex}>\n", g));
        }
        s.push_str("endbfchar\n");
    }
    s.push_str("endcmap\nCMapName currentdict /CMap defineresource pop\nend\nend\n");
    s.into_bytes()
}

/// Noto Serif SC, which Windows installs with its Chinese language features: only used when it's on this computer.
fn serif_from_windows() -> Option<Arc<Vec<u8>>> {
    let dir =
        std::env::var_os("WINDIR").map(std::path::PathBuf::from).unwrap_or_else(|| "C:\\Windows".into()).join("Fonts");
    let data = std::fs::read(dir.join("NotoSerifSC-VF.ttf")).ok()?;
    // a font that says it mustn't be embedded isn't (OS/2 fsType bit 1)
    let face = Face::parse(&data, 0).ok()?;
    if face.permissions() == Some(ttf_parser::Permissions::Restricted) {
        return None;
    }
    Some(Arc::new(data))
}
