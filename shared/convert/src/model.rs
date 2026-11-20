//! Models: glb, obj and stl are written; gltf, 3mf, ply and off are read too. DotAny 3D reads and writes models in its
//! page with three.js; this is the same work in Rust, held to what 3D's Save As does (`model/src/models.js`):
//! - every format has its own size and up: glTF stands Y up in metres, STL Z up in millimetres, OBJ Y up and read in
//!   centimetres, 3MF Z up in the unit it names. A model is turned and scaled on its way out, as 3D's `outward` does,
//!   so it opens again the same size and the same way up;
//! - STL is the triangles alone ("Preserves 3D geometry only, excluding colors and animations."); OBJ is shapes,
//!   normals and texture places, without materials, as three's OBJExporter writes it ("Animations and textures will not
//!   be included."); GLB carries colours and pictures;
//! - a glTF becomes a GLB by packing its own buffers and pictures into the one file, everything else as it was:
//!   animations, skins and compressed shapes included, nothing decoded.
//! Draco and Meshopt shapes are only unpacked by 3D's page: such a model goes to GLB, not to OBJ or STL. FBX is not read
//! here at all (README.md says why).

use crate::out::Outputs;
use crate::{Reason, Refusal, Step};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::io::{Cursor, Read};
use std::path::{Path, PathBuf};

type M4 = [[f64; 4]; 4];

const IDENTITY: M4 = [[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [0.0, 0.0, 0.0, 1.0]];

/// Column-major, as glTF and three.js keep them: m[column][row].
fn mul(a: &M4, b: &M4) -> M4 {
    let mut o = [[0.0; 4]; 4];
    for c in 0..4 {
        for r in 0..4 {
            o[c][r] = (0..4).map(|k| a[k][r] * b[c][k]).sum();
        }
    }
    o
}

fn apply(m: &M4, p: [f32; 3]) -> [f32; 3] {
    let v = [p[0] as f64, p[1] as f64, p[2] as f64];
    let mut o = [0f32; 3];
    for r in 0..3 {
        o[r] = (m[0][r] * v[0] + m[1][r] * v[1] + m[2][r] * v[2] + m[3][r]) as f32;
    }
    o
}

/// A direction: the matrix's inverse transpose, which for turns and even scales is the turn itself, renormalised.
fn apply_dir(m: &M4, n: [f32; 3]) -> [f32; 3] {
    let a = [[m[0][0], m[0][1], m[0][2]], [m[1][0], m[1][1], m[1][2]], [m[2][0], m[2][1], m[2][2]]];
    // cofactors of the 3×3 part are the inverse transpose times the determinant; its sign keeps the normal outward
    let cof = |r: usize, c: usize| {
        let (r1, r2, c1, c2) = ((r + 1) % 3, (r + 2) % 3, (c + 1) % 3, (c + 2) % 3);
        a[c1][r1] * a[c2][r2] - a[c2][r1] * a[c1][r2]
    };
    let v = [n[0] as f64, n[1] as f64, n[2] as f64];
    let mut o = [0f64; 3];
    for r in 0..3 {
        o[r] = (0..3).map(|c| cof(r, c) * v[c]).sum();
    }
    let len = (o[0] * o[0] + o[1] * o[1] + o[2] * o[2]).sqrt();
    if len > 0.0 { [(o[0] / len) as f32, (o[1] / len) as f32, (o[2] / len) as f32] } else { n }
}

#[derive(Clone, Default)]
struct Material {
    color: [f32; 4],
    /// (mime type, bytes)
    picture: Option<(String, Vec<u8>)>,
}

/// One shape, its points already where the whole model puts them, in the source's own units and up.
#[derive(Default)]
struct Mesh {
    name: String,
    positions: Vec<[f32; 3]>,
    normals: Option<Vec<[f32; 3]>>,
    uvs: Option<Vec<[f32; 2]>>,
    colors: Option<Vec<[f32; 4]>>,
    indices: Vec<u32>,
    material: Material,
}

struct Scene {
    meshes: Vec<Mesh>,
    /// millimetres in one unit of the file
    mm: f64,
    z_up: bool,
}

/// Millimetres in one unit of a 3MF file, by the unit's name there (the sizes of 3D's `UNITS`).
fn mm_of(unit: &str) -> f64 {
    match unit {
        "micron" => 0.001,
        "centimeter" => 10.0,
        "meter" => 1000.0,
        "inch" => 25.4,
        "foot" => 304.8,
        _ => 1.0,
    }
}

/// The turn and scale around the model for a format that stands `z_up` in `mm` millimetres per unit (3D's `outward`,
/// with its `WANT` table: GLB 1000 and Y up, OBJ 10 and Y up, STL 1 and Z up).
fn outward(s: &Scene, mm: f64, z_up: bool) -> M4 {
    let k = s.mm / mm;
    // rotation.x: -90° takes Z up to Y up, +90° Y up to Z up
    let (sin, cos) = match (s.z_up, z_up) {
        (true, false) => (-1.0, 0.0),
        (false, true) => (1.0, 0.0),
        _ => (0.0, 1.0),
    };
    [[k, 0.0, 0.0, 0.0], [0.0, k * cos, k * sin, 0.0], [0.0, -k * sin, k * cos, 0.0], [0.0, 0.0, 0.0, 1.0]]
}

// ---------- reading glTF ----------

fn data_uri(uri: &str) -> Option<Vec<u8>> {
    let (head, body) = uri.strip_prefix("data:")?.split_once(',')?;
    if !head.ends_with(";base64") {
        return Some(percent_decode(body));
    }
    let val = |c: u8| match c {
        b'A'..=b'Z' => Some(c - b'A'),
        b'a'..=b'z' => Some(c - b'a' + 26),
        b'0'..=b'9' => Some(c - b'0' + 52),
        b'+' | b'-' => Some(62),
        b'/' | b'_' => Some(63),
        _ => None,
    };
    let v: Vec<u8> = body.bytes().filter_map(val).collect();
    let mut out = Vec::with_capacity(v.len() * 3 / 4);
    for c in v.chunks(4) {
        let n = c.iter().enumerate().fold(0u32, |a, (i, x)| a | (*x as u32) << (18 - 6 * i));
        for k in 0..c.len().saturating_sub(1) {
            out.push((n >> (16 - 8 * k)) as u8);
        }
    }
    Some(out)
}

fn percent_decode(s: &str) -> Vec<u8> {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    out
}

/// A file a glTF names beside it: only within its own folder, as 3D's protocol serves them.
fn beside(dir: &Path, uri: &str) -> Result<Vec<u8>, Reason> {
    if let Some(d) = data_uri(uri) {
        return Ok(d);
    }
    let name = String::from_utf8_lossy(&percent_decode(uri)).replace('\\', "/");
    if name.contains("..") || name.starts_with('/') || name.contains(':') {
        return Err(Reason::Damaged);
    }
    std::fs::read(dir.join(name)).map_err(|_| Reason::Unreadable)
}

const PACKED: [&str; 3] = ["KHR_draco_mesh_compression", "EXT_meshopt_compression", "KHR_meshopt_compression"];

fn read_gltf(p: &Path, bytes: &[u8]) -> Result<Scene, Reason> {
    let g = gltf::Gltf::from_slice_without_validation(bytes).map_err(|_| Reason::Damaged)?;
    if g.extensions_used().any(|x| PACKED.contains(&x)) {
        return Err(Reason::Refused(Refusal::Unsupported));
    }
    let dir = p.parent().unwrap_or(Path::new("."));
    let mut buffers = Vec::new();
    for b in g.buffers() {
        buffers.push(match b.source() {
            gltf::buffer::Source::Bin => g.blob.clone().ok_or(Reason::Damaged)?,
            gltf::buffer::Source::Uri(u) => beside(dir, u)?,
        });
        if buffers.last().unwrap().len() < b.length() {
            return Err(Reason::Damaged);
        }
    }
    let scene = g.default_scene().or_else(|| g.scenes().next()).ok_or(Reason::Damaged)?;
    let mut meshes = Vec::new();
    fn walk(node: gltf::Node, parent: &M4, buffers: &[Vec<u8>], out: &mut Vec<Mesh>) -> Result<(), Reason> {
        let local = node.transform().matrix();
        let local: M4 = local.map(|c| c.map(|v| v as f64));
        let world = mul(parent, &local);
        if let Some(mesh) = node.mesh() {
            for prim in mesh.primitives() {
                let reader = prim.reader(|b| buffers.get(b.index()).map(|v| v.as_slice()));
                let Some(pos) = prim.get(&gltf::Semantic::Positions) else { continue };
                if pos.data_type() != gltf::accessor::DataType::F32 {
                    return Err(Reason::Refused(Refusal::Unsupported));
                }
                let positions: Vec<[f32; 3]> =
                    reader.read_positions().ok_or(Reason::Damaged)?.map(|q| apply(&world, q)).collect();
                let n = positions.len() as u32;
                let raw: Vec<u32> = match reader.read_indices() {
                    Some(i) => i.into_u32().collect(),
                    None => (0..n).collect(),
                };
                if raw.iter().any(|i| *i >= n) {
                    return Err(Reason::Damaged);
                }
                use gltf::mesh::Mode;
                let indices: Vec<u32> = match prim.mode() {
                    Mode::Triangles => raw[..raw.len() / 3 * 3].to_vec(),
                    Mode::TriangleStrip => (2..raw.len())
                        .flat_map(|i| {
                            if i % 2 == 0 { [raw[i - 2], raw[i - 1], raw[i]] } else { [raw[i - 1], raw[i - 2], raw[i]] }
                        })
                        .collect(),
                    Mode::TriangleFan => (2..raw.len()).flat_map(|i| [raw[0], raw[i - 1], raw[i]]).collect(),
                    // points and lines have no surface to keep
                    _ => continue,
                };
                let normals = reader
                    .read_normals()
                    .filter(|_| {
                        prim.get(&gltf::Semantic::Normals)
                            .is_some_and(|a| a.data_type() == gltf::accessor::DataType::F32)
                    })
                    .map(|it| it.map(|q| apply_dir(&world, q)).collect::<Vec<_>>());
                let uvs = reader.read_tex_coords(0).map(|t| t.into_f32().collect::<Vec<_>>());
                let colors = reader.read_colors(0).map(|c| c.into_rgba_f32().collect::<Vec<_>>());
                let pbr = prim.material().pbr_metallic_roughness();
                out.push(Mesh {
                    name: mesh.name().or(node.name()).unwrap_or("").to_string(),
                    normals: normals.filter(|v| v.len() == positions.len()),
                    uvs: uvs.filter(|v| v.len() == positions.len()),
                    colors: colors.filter(|v| v.len() == positions.len()),
                    positions,
                    indices,
                    material: Material { color: pbr.base_color_factor(), picture: None },
                });
            }
        }
        for c in node.children() {
            walk(c, &world, buffers, out)?;
        }
        Ok(())
    }
    for n in scene.nodes() {
        walk(n, &IDENTITY, &buffers, &mut meshes)?;
    }
    Ok(Scene { meshes, mm: 1000.0, z_up: false })
}

/// A glTF with its buffers and pictures packed into one GLB, everything else in it exactly as it was.
fn pack_gltf(p: &Path, bytes: &[u8]) -> Result<Vec<u8>, Reason> {
    let mut doc: Value =
        serde_json::from_slice(bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(bytes)).map_err(|_| Reason::Damaged)?;
    let dir = p.parent().unwrap_or(Path::new("."));
    let mut bin: Vec<u8> = Vec::new();
    let align = |bin: &mut Vec<u8>| bin.resize(bin.len().div_ceil(4) * 4, 0);
    // every buffer's start in the one packed buffer
    let mut starts = Vec::new();
    for b in doc["buffers"].as_array().cloned().unwrap_or_default() {
        let uri = b["uri"].as_str().ok_or(Reason::Damaged)?;
        let data = beside(dir, uri)?;
        let len = b["byteLength"].as_u64().ok_or(Reason::Damaged)? as usize;
        if data.len() < len {
            return Err(Reason::Damaged);
        }
        align(&mut bin);
        starts.push(bin.len() as u64);
        bin.extend_from_slice(&data[..len]);
    }
    if let Some(views) = doc["bufferViews"].as_array_mut() {
        for v in views {
            let b = v["buffer"].as_u64().ok_or(Reason::Damaged)? as usize;
            let at = *starts.get(b).ok_or(Reason::Damaged)? + v["byteOffset"].as_u64().unwrap_or(0);
            v["buffer"] = json!(0);
            v["byteOffset"] = json!(at);
        }
    }
    // pictures named beside the file go in too, each as a view of its own
    let mut pictures = Vec::new();
    if let Some(images) = doc["images"].as_array() {
        for (i, im) in images.iter().enumerate() {
            if let Some(uri) = im["uri"].as_str() {
                pictures.push((i, beside(dir, uri)?, uri.to_lowercase()));
            }
        }
    }
    for (i, data, uri) in pictures {
        align(&mut bin);
        let view = doc["bufferViews"].as_array().map_or(0, |v| v.len());
        if doc.get("bufferViews").is_none() {
            doc["bufferViews"] = json!([]);
        }
        doc["bufferViews"]
            .as_array_mut()
            .unwrap()
            .push(json!({ "buffer": 0, "byteOffset": bin.len(), "byteLength": data.len() }));
        bin.extend_from_slice(&data);
        let mime = if data.starts_with(b"\x89PNG") || uri.ends_with(".png") {
            "image/png"
        } else if data.starts_with(b"RIFF") {
            "image/webp"
        } else {
            "image/jpeg"
        };
        let im = &mut doc["images"][i];
        im.as_object_mut().unwrap().remove("uri");
        im["bufferView"] = json!(view);
        im["mimeType"] = json!(mime);
    }
    align(&mut bin);
    if !bin.is_empty() || doc.get("buffers").is_some() {
        doc["buffers"] = json!([{ "byteLength": bin.len() }]);
    }
    Ok(glb(&doc, &bin))
}

fn glb(doc: &Value, bin: &[u8]) -> Vec<u8> {
    let mut js = serde_json::to_vec(doc).unwrap();
    js.resize(js.len().div_ceil(4) * 4, b' ');
    let total = 12 + 8 + js.len() + if bin.is_empty() { 0 } else { 8 + bin.len() };
    let mut out = Vec::with_capacity(total);
    out.extend_from_slice(b"glTF");
    out.extend_from_slice(&2u32.to_le_bytes());
    out.extend_from_slice(&(total as u32).to_le_bytes());
    out.extend_from_slice(&(js.len() as u32).to_le_bytes());
    out.extend_from_slice(b"JSON");
    out.extend_from_slice(&js);
    if !bin.is_empty() {
        out.extend_from_slice(&(bin.len() as u32).to_le_bytes());
        out.extend_from_slice(b"BIN\0");
        out.extend_from_slice(bin);
    }
    out
}

// ---------- reading STL ----------

fn read_stl(bytes: &[u8], name: &str) -> Result<Scene, Reason> {
    let mut mesh = Mesh {
        name: name.to_string(),
        material: Material { color: [1.0, 1.0, 1.0, 1.0], picture: None },
        ..Default::default()
    };
    let binary =
        bytes.len() >= 84 && 84 + 50 * u32::from_le_bytes(bytes[80..84].try_into().unwrap()) as usize == bytes.len();
    if binary {
        let n = u32::from_le_bytes(bytes[80..84].try_into().unwrap()) as usize;
        // Magics writes COLOR= in the header and a colour in each face's two spare bytes; three's loader reads them too
        let header = &bytes[..80];
        let colored = header.windows(6).any(|w| w == b"COLOR=");
        let mut colors = Vec::new();
        let f = |b: &[u8], at: usize| f32::from_le_bytes(b[at..at + 4].try_into().unwrap());
        for t in 0..n {
            let r = &bytes[84 + t * 50..84 + t * 50 + 50];
            for v in 0..3 {
                let at = 12 + v * 12;
                mesh.positions.push([f(r, at), f(r, at + 4), f(r, at + 8)]);
            }
            if colored {
                let a = u16::from_le_bytes([r[48], r[49]]);
                let c = if a & 0x8000 == 0 {
                    [(a & 31) as f32 / 31.0, ((a >> 5) & 31) as f32 / 31.0, ((a >> 10) & 31) as f32 / 31.0, 1.0]
                } else {
                    [1.0; 4]
                };
                colors.extend([c, c, c]);
            }
        }
        if colored {
            mesh.colors = Some(colors);
        }
    } else {
        let text = String::from_utf8_lossy(bytes);
        if !text.trim_start().starts_with("solid") {
            return Err(Reason::Damaged);
        }
        for line in text.lines() {
            let mut w = line.split_whitespace();
            if w.next() == Some("vertex") {
                let v: Vec<f32> = w.take(3).map(|x| x.parse().unwrap_or(f32::NAN)).collect();
                if v.len() != 3 || v.iter().any(|x| !x.is_finite()) {
                    return Err(Reason::Damaged);
                }
                mesh.positions.push([v[0], v[1], v[2]]);
            }
        }
        mesh.positions.truncate(mesh.positions.len() / 3 * 3);
    }
    if mesh.positions.is_empty() {
        return Err(Reason::Damaged);
    }
    mesh.indices = (0..mesh.positions.len() as u32).collect();
    Ok(Scene { meshes: vec![mesh], mm: 1.0, z_up: true })
}

// ---------- reading OBJ and its MTL ----------

fn read_mtl(dir: &Path, text: &str) -> HashMap<String, Material> {
    let mut out = HashMap::new();
    let mut cur: Option<(String, Material)> = None;
    for line in text.lines() {
        let line = line.trim();
        let (k, rest) = line.split_once(char::is_whitespace).map(|(a, b)| (a, b.trim())).unwrap_or((line, ""));
        match k {
            "newmtl" => {
                if let Some((n, m)) = cur.take() {
                    out.insert(n, m);
                }
                cur = Some((rest.to_string(), Material { color: [1.0, 1.0, 1.0, 1.0], picture: None }));
            }
            "Kd" => {
                if let Some((_, m)) = cur.as_mut() {
                    let v: Vec<f32> = rest.split_whitespace().filter_map(|x| x.parse().ok()).collect();
                    if v.len() >= 3 {
                        m.color[..3].copy_from_slice(&v[..3]);
                    }
                }
            }
            "d" => {
                if let (Some((_, m)), Ok(v)) = (cur.as_mut(), rest.parse::<f32>()) {
                    m.color[3] = v;
                }
            }
            "map_Kd" => {
                if let Some((_, m)) = cur.as_mut() {
                    // options come first (-s 1 1 1 and the like); the file is the rest
                    let file = rest
                        .rsplit_once(" -")
                        .map(|(_, b)| b.split_once(' ').map(|x| x.1).unwrap_or(b))
                        .unwrap_or(rest);
                    let file = rest.split_whitespace().last().filter(|_| file.contains(' ')).unwrap_or(file);
                    if let Ok(b) = beside(dir, file.trim()) {
                        let mime = if b.starts_with(b"\x89PNG") {
                            "image/png"
                        } else if b.starts_with(&[0xFF, 0xD8]) {
                            "image/jpeg"
                        } else {
                            ""
                        };
                        if !mime.is_empty() {
                            m.picture = Some((mime.into(), b));
                            m.color[..3].copy_from_slice(&[1.0, 1.0, 1.0]);
                        }
                    }
                }
            }
            _ => {}
        }
    }
    if let Some((n, m)) = cur {
        out.insert(n, m);
    }
    out
}

fn read_obj(p: &Path, bytes: &[u8]) -> Result<Scene, Reason> {
    let text = String::from_utf8_lossy(bytes);
    let dir = p.parent().unwrap_or(Path::new("."));
    let (mut v, mut vt, mut vn): (Vec<[f32; 3]>, Vec<[f32; 2]>, Vec<[f32; 3]>) = (Vec::new(), Vec::new(), Vec::new());
    let mut mats: HashMap<String, Material> = HashMap::new();
    let mut meshes: Vec<Mesh> = Vec::new();
    // one mesh per object or group and material, as three's OBJLoader splits them; a corner is its v/vt/vn triple
    let mut cur = Mesh::default();
    let mut corner: HashMap<(i64, i64, i64), u32> = HashMap::new();
    let (mut group, mut mat) = (String::new(), String::new());
    let flush = |cur: &mut Mesh, corner: &mut HashMap<(i64, i64, i64), u32>, meshes: &mut Vec<Mesh>| {
        if !cur.indices.is_empty() {
            meshes.push(std::mem::take(cur));
        }
        *cur = Mesh::default();
        corner.clear();
    };
    let num = |s: &str| s.parse::<f32>().map_err(|_| Reason::Damaged);
    for line in text.lines() {
        let line = line.trim();
        let mut w = line.split_whitespace();
        match w.next() {
            Some("v") => {
                let x: Vec<f32> = w.take(3).map(num).collect::<Result<_, _>>()?;
                if x.len() < 3 {
                    return Err(Reason::Damaged);
                }
                v.push([x[0], x[1], x[2]]);
            }
            Some("vt") => {
                let x: Vec<f32> = w.take(2).map(num).collect::<Result<_, _>>()?;
                vt.push([*x.first().unwrap_or(&0.0), *x.get(1).unwrap_or(&0.0)]);
            }
            Some("vn") => {
                let x: Vec<f32> = w.take(3).map(num).collect::<Result<_, _>>()?;
                if x.len() < 3 {
                    return Err(Reason::Damaged);
                }
                vn.push([x[0], x[1], x[2]]);
            }
            Some("o") | Some("g") => {
                flush(&mut cur, &mut corner, &mut meshes);
                group = w.collect::<Vec<_>>().join(" ");
            }
            Some("usemtl") => {
                flush(&mut cur, &mut corner, &mut meshes);
                mat = w.collect::<Vec<_>>().join(" ");
            }
            Some("mtllib") => {
                let lib = line[6..].trim();
                if let Ok(b) = beside(dir, lib) {
                    mats.extend(read_mtl(dir, &String::from_utf8_lossy(&b)));
                }
            }
            Some("f") => {
                let resolve = |s: &str, len: usize| -> Result<i64, Reason> {
                    if s.is_empty() {
                        return Ok(-1);
                    }
                    let i: i64 = s.parse().map_err(|_| Reason::Damaged)?;
                    let at = if i < 0 { len as i64 + i } else { i - 1 };
                    if at < 0 || at >= len as i64 {
                        return Err(Reason::Damaged);
                    }
                    Ok(at)
                };
                let mut ids = Vec::new();
                for c in w {
                    let mut parts = c.split('/');
                    let pi = resolve(parts.next().unwrap_or(""), v.len())?;
                    let ti = resolve(parts.next().unwrap_or(""), vt.len())?;
                    let ni = resolve(parts.next().unwrap_or(""), vn.len())?;
                    if pi < 0 {
                        return Err(Reason::Damaged);
                    }
                    if cur.positions.is_empty() {
                        cur.name = group.clone();
                        cur.material =
                            mats.get(&mat).cloned().unwrap_or(Material { color: [1.0, 1.0, 1.0, 1.0], picture: None });
                    }
                    let id = *corner.entry((pi, ti, ni)).or_insert_with(|| {
                        cur.positions.push(v[pi as usize]);
                        if ti >= 0 {
                            cur.uvs.get_or_insert_with(Vec::new).push(vt[ti as usize]);
                        }
                        if ni >= 0 {
                            cur.normals.get_or_insert_with(Vec::new).push(vn[ni as usize]);
                        }
                        cur.positions.len() as u32 - 1
                    });
                    ids.push(id);
                }
                for k in 1..ids.len().saturating_sub(1) {
                    cur.indices.extend([ids[0], ids[k], ids[k + 1]]);
                }
            }
            _ => {}
        }
    }
    flush(&mut cur, &mut corner, &mut meshes);
    // a mesh where only some corners had texture places or normals keeps neither, rather than mismatched ones
    for m in &mut meshes {
        if m.uvs.as_ref().is_some_and(|u| u.len() != m.positions.len()) {
            m.uvs = None;
        }
        if m.normals.as_ref().is_some_and(|n| n.len() != m.positions.len()) {
            m.normals = None;
        }
    }
    if meshes.is_empty() {
        return Err(Reason::Damaged);
    }
    Ok(Scene { meshes, mm: 10.0, z_up: false })
}

// ---------- reading 3MF ----------

#[allow(deprecated)]
fn attr(tag: &quick_xml::events::BytesStart, name: &[u8]) -> Option<String> {
    tag.attributes()
        .flatten()
        .find(|a| a.key.local_name().as_ref() == name)
        .and_then(|a| a.unescape_value().ok())
        .map(|v| v.into_owned())
}

/// A 3MF transform, "m00 m01 m02 m10 m11 m12 m20 m21 m22 m30 m31 m32", as a column-major matrix.
fn mf_matrix(t: Option<String>) -> M4 {
    let Some(t) = t else { return IDENTITY };
    let v: Vec<f64> = t.split_whitespace().filter_map(|x| x.parse().ok()).collect();
    if v.len() != 12 {
        return IDENTITY;
    }
    [[v[0], v[1], v[2], 0.0], [v[3], v[4], v[5], 0.0], [v[6], v[7], v[8], 0.0], [v[9], v[10], v[11], 1.0]]
}

fn hex_color(s: &str) -> [f32; 4] {
    let h = s.trim_start_matches('#');
    let c = |i: usize| u8::from_str_radix(h.get(i..i + 2).unwrap_or("ff"), 16).unwrap_or(255) as f32 / 255.0;
    [c(0), c(2), c(4), if h.len() >= 8 { c(6) } else { 1.0 }]
}

#[derive(Default)]
struct MfObject {
    verts: Vec<[f32; 3]>,
    /// v1, v2, v3 and the colour of each
    tris: Vec<([u32; 3], Option<[f32; 4]>)>,
    comps: Vec<(String, M4)>,
    color: Option<[f32; 4]>,
}

fn read_3mf(bytes: &[u8]) -> Result<Scene, Reason> {
    use quick_xml::events::Event;
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).map_err(|_| Reason::Damaged)?;
    let name = zip
        .file_names()
        .filter_map(|n| n.ok().map(|n| n.into_owned()))
        .find(|n| n.to_lowercase().ends_with(".model"))
        .ok_or(Reason::Damaged)?;
    let mut xml = String::new();
    zip.by_name(&name).map_err(|_| Reason::Damaged)?.read_to_string(&mut xml).map_err(|_| Reason::Damaged)?;
    let mut r = quick_xml::Reader::from_str(&xml);
    let mut unit = "millimeter".to_string();
    let mut objects: HashMap<String, MfObject> = HashMap::new();
    let mut colors: HashMap<String, Vec<[f32; 4]>> = HashMap::new();
    let mut items: Vec<(String, M4)> = Vec::new();
    let (mut obj_id, mut group) = (None::<String>, None::<String>);
    loop {
        let ev = r.read_event().map_err(|_| Reason::Damaged)?;
        let (Event::Start(t) | Event::Empty(t)) = &ev else {
            if let Event::End(t) = &ev {
                if t.local_name().as_ref() == b"object" {
                    obj_id = None;
                }
            }
            if matches!(ev, Event::Eof) {
                break;
            }
            continue;
        };
        match t.local_name().as_ref() {
            b"model" => unit = attr(t, b"unit").unwrap_or(unit),
            b"basematerials" => {
                group = attr(t, b"id");
                if let Some(g) = &group {
                    colors.insert(g.clone(), Vec::new());
                }
            }
            b"base" => {
                if let (Some(g), Some(c)) = (&group, attr(t, b"displaycolor")) {
                    colors.get_mut(g).unwrap().push(hex_color(&c));
                }
            }
            b"object" => {
                let id = attr(t, b"id").ok_or(Reason::Damaged)?;
                let color = attr(t, b"pid")
                    .zip(attr(t, b"pindex"))
                    .and_then(|(g, i)| colors.get(&g)?.get(i.parse::<usize>().ok()?).copied());
                objects.insert(id.clone(), MfObject { color, ..Default::default() });
                obj_id = Some(id);
            }
            b"vertex" => {
                let o = objects.get_mut(obj_id.as_ref().ok_or(Reason::Damaged)?).unwrap();
                let c = |k: &[u8]| attr(t, k).and_then(|v| v.parse::<f32>().ok()).ok_or(Reason::Damaged);
                o.verts.push([c(b"x")?, c(b"y")?, c(b"z")?]);
            }
            b"triangle" => {
                let o = objects.get_mut(obj_id.as_ref().ok_or(Reason::Damaged)?).unwrap();
                let c = |k: &[u8]| attr(t, k).and_then(|v| v.parse::<u32>().ok()).ok_or(Reason::Damaged);
                let tri = [c(b"v1")?, c(b"v2")?, c(b"v3")?];
                let color = attr(t, b"pid")
                    .zip(attr(t, b"p1"))
                    .and_then(|(g, i)| colors.get(&g)?.get(i.parse::<usize>().ok()?).copied());
                o.tris.push((tri, color));
            }
            b"component" => {
                let o = objects.get_mut(obj_id.as_ref().ok_or(Reason::Damaged)?).unwrap();
                o.comps.push((attr(t, b"objectid").ok_or(Reason::Damaged)?, mf_matrix(attr(t, b"transform"))));
            }
            b"item" => items.push((attr(t, b"objectid").ok_or(Reason::Damaged)?, mf_matrix(attr(t, b"transform")))),
            _ => {}
        }
    }
    let mut meshes = Vec::new();
    fn put(
        id: &str,
        world: &M4,
        objects: &HashMap<String, MfObject>,
        out: &mut Vec<Mesh>,
        depth: u32,
    ) -> Result<(), Reason> {
        let o = objects.get(id).ok_or(Reason::Damaged)?;
        if depth > 32 {
            return Err(Reason::Damaged);
        }
        if !o.tris.is_empty() {
            let colored = o.tris.iter().any(|t| t.1.is_some());
            let mut m = Mesh {
                name: format!("object {id}"),
                material: Material { color: o.color.unwrap_or([1.0; 4]), picture: None },
                ..Default::default()
            };
            if colored {
                // a colour per triangle: each triangle gets corners of its own
                let mut cols = Vec::new();
                for (tri, c) in &o.tris {
                    for v in tri {
                        m.positions.push(apply(world, *o.verts.get(*v as usize).ok_or(Reason::Damaged)?));
                        cols.push(c.or(o.color).unwrap_or([1.0; 4]));
                    }
                }
                m.indices = (0..m.positions.len() as u32).collect();
                m.colors = Some(cols);
                m.material.color = [1.0; 4];
            } else {
                m.positions = o.verts.iter().map(|v| apply(world, *v)).collect();
                for (tri, _) in &o.tris {
                    if tri.iter().any(|v| *v as usize >= o.verts.len()) {
                        return Err(Reason::Damaged);
                    }
                    m.indices.extend(tri);
                }
            }
            out.push(m);
        }
        for (c, t) in &o.comps {
            put(c, &mul(world, t), objects, out, depth + 1)?;
        }
        Ok(())
    }
    for (id, t) in &items {
        put(id, t, &objects, &mut meshes, 0)?;
    }
    if meshes.is_empty() {
        return Err(Reason::Damaged);
    }
    Ok(Scene { meshes, mm: mm_of(&unit.to_lowercase()), z_up: true })
}

// ---------- writing ----------

fn face_normal(a: [f32; 3], b: [f32; 3], c: [f32; 3]) -> [f32; 3] {
    let u = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
    let v = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
    let n = [u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0]];
    let l = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
    if l > 0.0 { [n[0] / l, n[1] / l, n[2] / l] } else { [0.0, 0.0, 0.0] }
}

/// Binary STL in millimetres, Z up: three's STLExporter, the triangles and their normals, nothing else.
fn write_stl(s: &Scene) -> Vec<u8> {
    let m = outward(s, 1.0, true);
    let tris: usize = s.meshes.iter().map(|x| x.indices.len() / 3).sum();
    let mut out = Vec::with_capacity(84 + tris * 50);
    let mut header = [b' '; 80];
    header[..18].copy_from_slice(b"DotAny STL Export\0");
    out.extend_from_slice(&header);
    out.extend_from_slice(&(tris as u32).to_le_bytes());
    for mesh in &s.meshes {
        for t in mesh.indices.chunks_exact(3) {
            let [a, b, c] = [0, 1, 2].map(|k| apply(&m, mesh.positions[t[k] as usize]));
            for v in [face_normal(a, b, c), a, b, c] {
                for x in v {
                    out.extend_from_slice(&x.to_le_bytes());
                }
            }
            out.extend_from_slice(&[0, 0]);
        }
    }
    out
}

/// OBJ in centimetres, Y up: three's OBJExporter, an object per mesh with its points, texture places and normals.
fn write_obj(s: &Scene) -> Vec<u8> {
    use std::fmt::Write;
    let m = outward(s, 10.0, false);
    let mut out = String::new();
    let (mut base_v, mut base_t, mut base_n) = (1usize, 1usize, 1usize);
    for (i, mesh) in s.meshes.iter().enumerate() {
        let name = if mesh.name.is_empty() { format!("mesh{i}") } else { mesh.name.replace(['\n', '\r'], " ") };
        let _ = writeln!(out, "o {name}");
        for p in &mesh.positions {
            let q = apply(&m, *p);
            let _ = writeln!(out, "v {} {} {}", q[0], q[1], q[2]);
        }
        if let Some(uv) = &mesh.uvs {
            for t in uv {
                let _ = writeln!(out, "vt {} {}", t[0], 1.0 - t[1]);
            }
        }
        if let Some(ns) = &mesh.normals {
            for n in ns {
                let q = apply_dir(&m, *n);
                let _ = writeln!(out, "vn {} {} {}", q[0], q[1], q[2]);
            }
        }
        for t in mesh.indices.chunks_exact(3) {
            let corner = |k: u32| match (&mesh.uvs, &mesh.normals) {
                (Some(_), Some(_)) => {
                    format!("{}/{}/{}", base_v + k as usize, base_t + k as usize, base_n + k as usize)
                }
                (Some(_), None) => format!("{}/{}", base_v + k as usize, base_t + k as usize),
                (None, Some(_)) => format!("{}//{}", base_v + k as usize, base_n + k as usize),
                (None, None) => format!("{}", base_v + k as usize),
            };
            let _ = writeln!(out, "f {} {} {}", corner(t[0]), corner(t[1]), corner(t[2]));
        }
        base_v += mesh.positions.len();
        base_t += mesh.uvs.as_ref().map_or(0, Vec::len);
        base_n += mesh.normals.as_ref().map_or(0, Vec::len);
    }
    out.into_bytes()
}

/// GLB in metres, Y up: every mesh with its colour, its picture, and its corners' colours.
fn write_glb(s: &Scene) -> Vec<u8> {
    let m = outward(s, 1000.0, false);
    let mut bin: Vec<u8> = Vec::new();
    let (mut views, mut accessors, mut meshes, mut nodes, mut materials, mut images, mut textures) =
        (vec![], vec![], vec![], vec![], vec![], vec![], vec![]);
    let mut view = |bin: &mut Vec<u8>, data: &[u8], target: Option<u32>| -> usize {
        bin.resize(bin.len().div_ceil(4) * 4, 0);
        let mut v = json!({ "buffer": 0, "byteOffset": bin.len(), "byteLength": data.len() });
        if let Some(t) = target {
            v["target"] = json!(t);
        }
        bin.extend_from_slice(data);
        views.push(v);
        views.len() - 1
    };
    let floats = |v: &[f32]| v.iter().flat_map(|x| x.to_le_bytes()).collect::<Vec<u8>>();
    for (i, mesh) in s.meshes.iter().enumerate() {
        let pos: Vec<[f32; 3]> = mesh.positions.iter().map(|p| apply(&m, *p)).collect();
        let (mut lo, mut hi) = ([f32::MAX; 3], [f32::MIN; 3]);
        for p in &pos {
            for k in 0..3 {
                lo[k] = lo[k].min(p[k]);
                hi[k] = hi[k].max(p[k]);
            }
        }
        // normals: the file's own, or each triangle's for a shape that came without
        let normals: Vec<[f32; 3]> = match &mesh.normals {
            Some(ns) => ns.iter().map(|n| apply_dir(&m, *n)).collect(),
            None => {
                let mut acc = vec![[0f32; 3]; pos.len()];
                for t in mesh.indices.chunks_exact(3) {
                    let n = face_normal(pos[t[0] as usize], pos[t[1] as usize], pos[t[2] as usize]);
                    for k in t {
                        for d in 0..3 {
                            acc[*k as usize][d] += n[d];
                        }
                    }
                }
                acc.into_iter()
                    .map(|n| {
                        let l = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
                        if l > 0.0 { [n[0] / l, n[1] / l, n[2] / l] } else { [0.0, 1.0, 0.0] }
                    })
                    .collect()
            }
        };
        let pv = view(&mut bin, &floats(&pos.concat()), Some(34962));
        accessors.push(json!({ "bufferView": pv, "componentType": 5126, "count": pos.len(), "type": "VEC3", "min": lo, "max": hi }));
        let mut attrs = json!({ "POSITION": accessors.len() - 1 });
        let nv = view(&mut bin, &floats(&normals.concat()), Some(34962));
        accessors.push(json!({ "bufferView": nv, "componentType": 5126, "count": normals.len(), "type": "VEC3" }));
        attrs["NORMAL"] = json!(accessors.len() - 1);
        if let Some(uv) = &mesh.uvs {
            let tv = view(&mut bin, &floats(&uv.concat()), Some(34962));
            accessors.push(json!({ "bufferView": tv, "componentType": 5126, "count": uv.len(), "type": "VEC2" }));
            attrs["TEXCOORD_0"] = json!(accessors.len() - 1);
        }
        if let Some(c) = &mesh.colors {
            let cv = view(&mut bin, &floats(&c.concat()), Some(34962));
            accessors.push(json!({ "bufferView": cv, "componentType": 5126, "count": c.len(), "type": "VEC4" }));
            attrs["COLOR_0"] = json!(accessors.len() - 1);
        }
        let iv = view(&mut bin, &mesh.indices.iter().flat_map(|x| x.to_le_bytes()).collect::<Vec<u8>>(), Some(34963));
        accessors
            .push(json!({ "bufferView": iv, "componentType": 5125, "count": mesh.indices.len(), "type": "SCALAR" }));
        let mut pbr = json!({ "baseColorFactor": mesh.material.color, "metallicFactor": 0.0, "roughnessFactor": 1.0 });
        if let (Some((mime, data)), Some(_)) = (&mesh.material.picture, &mesh.uvs) {
            let v = view(&mut bin, data, None);
            images.push(json!({ "bufferView": v, "mimeType": mime }));
            textures.push(json!({ "source": images.len() - 1 }));
            pbr["baseColorTexture"] = json!({ "index": textures.len() - 1 });
        }
        let mut mat = json!({ "pbrMetallicRoughness": pbr, "doubleSided": true });
        if mesh.material.color[3] < 1.0 {
            mat["alphaMode"] = json!("BLEND");
        }
        materials.push(mat);
        meshes.push(json!({ "name": mesh.name, "primitives": [{ "attributes": attrs, "indices": accessors.len() - 1, "material": materials.len() - 1 }] }));
        nodes.push(json!({ "mesh": i, "name": mesh.name }));
    }
    bin.resize(bin.len().div_ceil(4) * 4, 0);
    let mut doc = json!({
        "asset": { "version": "2.0", "generator": "DotAny" },
        "scene": 0,
        "scenes": [{ "nodes": (0..nodes.len()).collect::<Vec<_>>() }],
        "nodes": nodes,
        "meshes": meshes,
        "materials": materials,
        "accessors": accessors,
        "bufferViews": views,
        "buffers": [{ "byteLength": bin.len() }],
    });
    if !images.is_empty() {
        doc["images"] = json!(images);
        doc["textures"] = json!(textures);
    }
    glb(&doc, &bin)
}

// ---------- reading PLY and OFF ----------
// 3D reads PLY with three's PLYLoader and OFF with its own readOff (`model/src/models.js`); these are the same rules.
// Neither says its unit or up: 3D takes millimetres, Y up, for both.

#[derive(Clone, Copy, PartialEq)]
enum Num {
    I8,
    U8,
    I16,
    U16,
    I32,
    U32,
    F32,
    F64,
}

impl Num {
    fn of(t: &str) -> Option<Num> {
        Some(match t {
            "char" | "int8" => Num::I8,
            "uchar" | "uint8" => Num::U8,
            "short" | "int16" => Num::I16,
            "ushort" | "uint16" => Num::U16,
            "int" | "int32" => Num::I32,
            "uint" | "uint32" => Num::U32,
            "float" | "float32" => Num::F32,
            "double" | "float64" => Num::F64,
            _ => return None,
        })
    }
    fn size(self) -> usize {
        match self {
            Num::I8 | Num::U8 => 1,
            Num::I16 | Num::U16 => 2,
            Num::I32 | Num::U32 | Num::F32 => 4,
            Num::F64 => 8,
        }
    }
    /// What a colour of this type is divided by (three's getColorScale).
    fn colour_scale(self) -> f64 {
        match self {
            Num::U16 => 65535.0,
            Num::F32 | Num::F64 => 1.0,
            _ => 255.0,
        }
    }
}

struct PlyProp {
    name: String,
    kind: Num,
    /// a list: the type of its length
    count: Option<Num>,
}

struct PlyElement {
    name: String,
    count: usize,
    props: Vec<PlyProp>,
}

/// The body's numbers, one after another, as text or as binary.
enum PlyBody<'a> {
    Text(std::str::SplitAsciiWhitespace<'a>),
    Binary { b: &'a [u8], at: usize, big: bool },
}

impl PlyBody<'_> {
    fn value(&mut self, k: Num) -> Result<f64, Reason> {
        match self {
            PlyBody::Text(words) => words.next().and_then(|w| w.parse::<f64>().ok()).ok_or(Reason::Damaged),
            PlyBody::Binary { b, at, big } => {
                let n = k.size();
                let raw = b.get(*at..*at + n).ok_or(Reason::Damaged)?;
                *at += n;
                let mut a = [0u8; 8];
                a[..n].copy_from_slice(raw);
                if *big {
                    a[..n].reverse();
                }
                Ok(match k {
                    Num::I8 => a[0] as i8 as f64,
                    Num::U8 => a[0] as f64,
                    Num::I16 => i16::from_le_bytes([a[0], a[1]]) as f64,
                    Num::U16 => u16::from_le_bytes([a[0], a[1]]) as f64,
                    Num::I32 => i32::from_le_bytes([a[0], a[1], a[2], a[3]]) as f64,
                    Num::U32 => u32::from_le_bytes([a[0], a[1], a[2], a[3]]) as f64,
                    Num::F32 => f32::from_le_bytes([a[0], a[1], a[2], a[3]]) as f64,
                    Num::F64 => f64::from_le_bytes(a),
                })
            }
        }
    }
}

/// sRGB to linear, as three's Color.setRGB(…, SRGBColorSpace) keeps a PLY's colours.
fn srgb_to_linear(c: f64) -> f32 {
    (if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }) as f32
}

/// PLY, text or binary either way round: the points (x y z, and the normals, texture places and colours three reads),
/// and the faces of three or four corners (three's loader leaves the rest out). A cloud of points alone has no shape
/// to write.
fn read_ply(bytes: &[u8], name: &str) -> Result<Scene, Reason> {
    let end = bytes.windows(10).position(|w| w == b"end_header").ok_or(Reason::Damaged)?;
    let body_at = bytes[end..].iter().position(|&c| c == b'\n').map(|i| end + i + 1).ok_or(Reason::Damaged)?;
    let head = std::str::from_utf8(&bytes[..end]).map_err(|_| Reason::Damaged)?;
    let mut lines = head.lines().map(str::trim);
    if lines.next() != Some("ply") {
        return Err(Reason::Damaged);
    }
    let (mut format, mut elements) = (None, Vec::<PlyElement>::new());
    for line in lines {
        let w: Vec<&str> = line.split_whitespace().collect();
        match w.as_slice() {
            ["format", f, ..] => format = Some(f.to_string()),
            ["element", n, c] => elements.push(PlyElement {
                name: n.to_string(),
                count: c.parse().map_err(|_| Reason::Damaged)?,
                props: Vec::new(),
            }),
            ["property", "list", c, k, n] => {
                let e = elements.last_mut().ok_or(Reason::Damaged)?;
                e.props.push(PlyProp {
                    name: n.to_string(),
                    kind: Num::of(k).ok_or(Reason::Damaged)?,
                    count: Some(Num::of(c).ok_or(Reason::Damaged)?),
                });
            }
            ["property", k, n] => {
                let e = elements.last_mut().ok_or(Reason::Damaged)?;
                e.props.push(PlyProp { name: n.to_string(), kind: Num::of(k).ok_or(Reason::Damaged)?, count: None });
            }
            _ => {}
        }
    }
    let mut body = match format.as_deref() {
        Some("ascii") => {
            PlyBody::Text(std::str::from_utf8(&bytes[body_at..]).map_err(|_| Reason::Damaged)?.split_ascii_whitespace())
        }
        Some("binary_little_endian") => PlyBody::Binary { b: bytes, at: body_at, big: false },
        Some("binary_big_endian") => PlyBody::Binary { b: bytes, at: body_at, big: true },
        _ => return Err(Reason::Damaged),
    };
    // a count no file this size could hold is a damaged header, not a reason to ask for the memory
    if elements.iter().any(|e| e.count > bytes.len()) {
        return Err(Reason::Damaged);
    }

    let mut mesh = Mesh {
        name: name.to_string(),
        material: Material { color: [1.0, 1.0, 1.0, 1.0], picture: None },
        ..Default::default()
    };
    let (mut normals, mut uvs, mut colors) = (Vec::new(), Vec::new(), Vec::new());
    for e in &elements {
        let find = |names: &[&str]| e.props.iter().position(|p| p.count.is_none() && names.contains(&p.name.as_str()));
        let (x, y, z) = (find(&["x"]), find(&["y"]), find(&["z"]));
        let (nx, ny, nz) = (find(&["nx", "normalx"]), find(&["ny", "normaly"]), find(&["nz", "normalz"]));
        let (s, t) = (find(&["s", "u", "texture_u", "tx"]), find(&["t", "v", "texture_v", "ty"]));
        let (r, g, b) = (
            find(&["red", "diffuse_red", "r", "diffuse_r"]),
            find(&["green", "diffuse_green", "g", "diffuse_g"]),
            find(&["blue", "diffuse_blue", "b", "diffuse_b"]),
        );
        let corners =
            e.props.iter().position(|p| p.count.is_some() && (p.name == "vertex_indices" || p.name == "vertex_index"));
        let mut one = vec![0f64; e.props.len()];
        let mut list = Vec::new();
        for _ in 0..e.count {
            for (i, p) in e.props.iter().enumerate() {
                match p.count {
                    None => one[i] = body.value(p.kind)?,
                    Some(c) => {
                        let n = body.value(c)?;
                        if !(0.0..=1e6).contains(&n) {
                            return Err(Reason::Damaged);
                        }
                        let mut items = Vec::with_capacity(n as usize);
                        for _ in 0..n as usize {
                            items.push(body.value(p.kind)?);
                        }
                        if Some(i) == corners {
                            list = items;
                        }
                    }
                }
            }
            if e.name == "vertex" {
                let (Some(x), Some(y), Some(z)) = (x, y, z) else { return Err(Reason::Damaged) };
                mesh.positions.push([one[x] as f32, one[y] as f32, one[z] as f32]);
                if let (Some(a), Some(b2), Some(c)) = (nx, ny, nz) {
                    normals.push([one[a] as f32, one[b2] as f32, one[c] as f32]);
                }
                if let (Some(a), Some(b2)) = (s, t) {
                    uvs.push([one[a] as f32, one[b2] as f32]);
                }
                if let Some(r) = r {
                    let k = e.props[r].kind.colour_scale();
                    let c = |i: Option<usize>| i.map_or(0.0, |i| srgb_to_linear(one[i] / k));
                    colors.push([c(Some(r)), c(g), c(b), 1.0]);
                }
            } else if e.name == "face" && corners.is_some() {
                let ids: Vec<u32> = list
                    .iter()
                    .map(|&v| if v >= 0.0 && v.fract() == 0.0 && v < u32::MAX as f64 { v as u32 } else { u32::MAX })
                    .collect();
                match ids.len() {
                    3 => mesh.indices.extend([ids[0], ids[1], ids[2]]),
                    4 => mesh.indices.extend([ids[0], ids[1], ids[3], ids[1], ids[2], ids[3]]),
                    _ => {}
                }
            }
        }
    }
    if mesh.positions.is_empty() || mesh.indices.iter().any(|&i| i as usize >= mesh.positions.len()) {
        return Err(Reason::Damaged);
    }
    if mesh.indices.is_empty() {
        // a cloud of points: 3D shows it and saves it as GLB, but there is no shape here to write
        return Err(Reason::Refused(Refusal::Unsupported));
    }
    let n = mesh.positions.len();
    mesh.normals = Some(normals).filter(|v| v.len() == n);
    mesh.uvs = Some(uvs).filter(|v| v.len() == n);
    mesh.colors = Some(colors).filter(|v| v.len() == n);
    Ok(Scene { meshes: vec![mesh], mm: 1.0, z_up: false })
}

/// OFF, as 3D's readOff takes it from the Geomview manual: an optional header word ([ST][C][N]OFF), the counts, a point
/// to a line, then a face to a line (its number of corners, their indices, and an optional colour, 0..1 or 0..255),
/// each face a fan of triangles. "#" starts a comment. The binary and the 4D forms aren't read.
fn read_off(bytes: &[u8], name: &str) -> Result<Scene, Reason> {
    let text = String::from_utf8_lossy(bytes);
    let lines: Vec<Vec<&str>> = text
        .lines()
        .map(|l| l.split('#').next().unwrap_or("").trim())
        .filter(|l| !l.is_empty())
        .map(|l| l.split_whitespace().collect())
        .collect();
    let (mut li, mut wi) = (0usize, 0usize);
    let next = |li: &mut usize, wi: &mut usize| -> Result<&str, Reason> {
        while *li < lines.len() && *wi >= lines[*li].len() {
            *li += 1;
            *wi = 0;
        }
        let w = lines.get(*li).ok_or(Reason::Damaged)?[*wi];
        *wi += 1;
        Ok(w)
    };
    let num =
        |w: &str| -> Result<f64, Reason> { w.parse::<f64>().ok().filter(|v| v.is_finite()).ok_or(Reason::Damaged) };
    let end_line = |li: &mut usize, wi: &mut usize| {
        if *wi > 0 {
            *li += 1;
            *wi = 0;
        }
    };
    // the header word: ST, C, N, 4 and n in that order, each optional, then OFF
    if let Some(first) = lines.first().and_then(|l| l.first()) {
        let mut s: &str = first;
        let mut four = false;
        for p in ["ST", "C", "N", "4", "n"] {
            if let Some(rest) = s.strip_prefix(p) {
                four |= p == "4" || p == "n";
                s = rest;
            }
        }
        if s == "OFF" {
            if four {
                return Err(Reason::Damaged);
            }
            next(&mut li, &mut wi)?;
            if lines.get(li).and_then(|l| l.get(wi)) == Some(&"BINARY") {
                return Err(Reason::Damaged);
            }
        }
    }
    let nv = num(next(&mut li, &mut wi)?)?;
    let nf = num(next(&mut li, &mut wi)?)?;
    num(next(&mut li, &mut wi)?)?;
    end_line(&mut li, &mut wi);
    if !(nv > 0.0 && nf > 0.0) || nv > 5e7 || nf > 5e7 {
        return Err(Reason::Damaged);
    }
    let (nv, nf) = (nv as usize, nf as usize);
    let mut pts = Vec::with_capacity(nv.min(bytes.len() / 6));
    for _ in 0..nv {
        let p = [
            num(next(&mut li, &mut wi)?)? as f32,
            num(next(&mut li, &mut wi)?)? as f32,
            num(next(&mut li, &mut wi)?)? as f32,
        ];
        pts.push(p);
        end_line(&mut li, &mut wi);
    }
    let (mut tris, mut cols): (Vec<u32>, Vec<Option<[f32; 4]>>) = (Vec::new(), Vec::new());
    let mut coloured = false;
    for _ in 0..nf {
        let n = num(next(&mut li, &mut wi)?)?;
        let mut ids = Vec::new();
        let mut k = 0.0;
        while k < n {
            let i = num(next(&mut li, &mut wi)?)?;
            if i < 0.0 || i >= nv as f64 || i.fract() != 0.0 {
                return Err(Reason::Damaged);
            }
            ids.push(i as u32);
            k += 1.0;
        }
        // what's left on the face's line is its colour
        let rest: Vec<f64> = if wi > 0 {
            lines[li][wi..lines[li].len().min(wi + 3)].iter().map(|w| w.parse::<f64>().unwrap_or(f64::NAN)).collect()
        } else {
            Vec::new()
        };
        end_line(&mut li, &mut wi);
        let mut c = None;
        if rest.len() == 3 && rest.iter().all(|v| v.is_finite()) {
            let d = if rest.iter().any(|&v| v > 1.0) { 255.0 } else { 1.0 };
            c = Some([(rest[0] / d) as f32, (rest[1] / d) as f32, (rest[2] / d) as f32, 1.0]);
            coloured = true;
        }
        for k in 1..ids.len().saturating_sub(1) {
            tris.extend([ids[0], ids[k], ids[k + 1]]);
            cols.extend([c, c, c]);
        }
    }
    if tris.is_empty() {
        return Err(Reason::Damaged);
    }
    let mut mesh = Mesh {
        name: name.to_string(),
        material: Material { color: [1.0, 1.0, 1.0, 1.0], picture: None },
        ..Default::default()
    };
    if coloured {
        // colours belong to faces: every corner its own point, grey where a face has none
        let grey = [0.78, 0.77, 0.74, 1.0];
        mesh.positions = tris.iter().map(|&i| pts[i as usize]).collect();
        mesh.colors = Some(cols.iter().map(|c| c.unwrap_or(grey)).collect());
        mesh.indices = (0..tris.len() as u32).collect();
    } else {
        mesh.positions = pts;
        mesh.indices = tris;
    }
    Ok(Scene { meshes: vec![mesh], mm: 1.0, z_up: false })
}

// ---------- the conversion ----------

fn read(p: &Path, bytes: &[u8]) -> Result<Scene, Reason> {
    let name = crate::stem_of(p);
    let ext = crate::ext_of(p);
    let scene = match ext.as_str() {
        "glb" | "gltf" => read_gltf(p, bytes)?,
        "stl" => read_stl(bytes, &name)?,
        "obj" => read_obj(p, bytes)?,
        "3mf" => read_3mf(bytes)?,
        "ply" => read_ply(bytes, &name)?,
        "off" => read_off(bytes, &name)?,
        _ => return Err(Reason::Refused(Refusal::Unsupported)),
    };
    if scene.meshes.iter().all(|m| m.indices.is_empty())
        || scene.meshes.iter().any(|m| m.positions.iter().any(|q| !q.iter().all(|v| v.is_finite())))
    {
        return Err(Reason::Damaged);
    }
    Ok(scene)
}

pub fn convert(p: &Path, target: &str, out: &mut Outputs, step: Step) -> Result<(), Reason> {
    let bytes = crate::read_all(p)?;
    step(0.1);
    let ext = crate::ext_of(p);
    if ext == "gltf" && target == "glb" {
        // a glTF and its own files, packed into one, nothing decoded
        let made = pack_gltf(p, &bytes)?;
        step(0.9);
        return out.write("glb", &made);
    }
    let scene = std::panic::catch_unwind(|| read(p, &bytes)).unwrap_or(Err(Reason::Damaged))?;
    step(0.6);
    let made = match target {
        "stl" => write_stl(&scene),
        "obj" => write_obj(&scene),
        _ => write_glb(&scene),
    };
    step(0.9);
    out.write(target, &made)
}

pub fn estimate(p: &Path, size: u64, target: &str) -> u64 {
    let ext = crate::ext_of(p);
    if ext == "gltf" && target == "glb" {
        // the file's own buffers and pictures, which sit beside it, go in
        let dir = p.parent().map(Path::to_path_buf).unwrap_or_else(|| PathBuf::from("."));
        let extra: u64 = std::fs::read(p)
            .ok()
            .and_then(|b| serde_json::from_slice::<Value>(&b).ok())
            .map(|d| {
                ["buffers", "images"]
                    .iter()
                    .flat_map(|k| d[*k].as_array().cloned().unwrap_or_default())
                    .filter_map(|b| b["uri"].as_str().map(str::to_string))
                    .filter(|u| !u.starts_with("data:"))
                    .filter_map(|u| {
                        std::fs::metadata(dir.join(String::from_utf8_lossy(&percent_decode(&u)).as_ref())).ok()
                    })
                    .map(|m| m.len())
                    .sum()
            })
            .unwrap_or(0);
        return size + extra;
    }
    let Ok(bytes) = std::fs::read(p) else { return size };
    let Ok(scene) = read(p, &bytes) else { return size };
    let tris: u64 = scene.meshes.iter().map(|m| m.indices.len() as u64 / 3).sum();
    match target {
        "stl" => 84 + 50 * tris,
        "obj" => {
            // about 30 characters a point, 15 a texture place, 30 a normal, and 10 a corner
            scene
                .meshes
                .iter()
                .map(|m| {
                    m.positions.len() as u64 * 30
                        + m.uvs.as_ref().map_or(0, |u| u.len() as u64 * 15)
                        + m.normals.as_ref().map_or(0, |n| n.len() as u64 * 30)
                        + m.indices.len() as u64 * 10
                })
                .sum()
        }
        _ => {
            scene
                .meshes
                .iter()
                .map(|m| {
                    m.positions.len() as u64
                        * (24 + if m.uvs.is_some() { 8 } else { 0 } + if m.colors.is_some() { 16 } else { 0 })
                        + m.indices.len() as u64 * 4
                        + m.material.picture.as_ref().map_or(0, |p| p.1.len() as u64)
                        + 400
                })
                .sum::<u64>()
                + 600
        }
    }
}
