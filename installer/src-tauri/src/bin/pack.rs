//! dotany-pack: reads installer/payload.json, measures the built programs, and writes DotAny-Setup.exe: the installer
//! program with every file appended as one zstd stream (see src/payload.rs), plus manifest.json beside it, the
//! release description a release publishes for the hub's updates.
//!
//!     dotany-pack [--repo <DotAny folder>] [--setup <dotany-setup.exe>] [--out <folder>] [--level 1-22]

use dotany_setup::manifest::{ClaimRules, Component, FileEntry, Release, SCHEMA, mb};
use dotany_setup::payload::{FOOTER, MAGIC, WINDOW_LOG, hex};
use dotany_setup::system::now;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::fs::{self, File};
use std::io::{self, BufReader, BufWriter, Read, Write};
use std::path::PathBuf;

#[derive(Deserialize)]
struct Spec {
    product: String,
    version: String,
    #[serde(default)]
    claim: ClaimRules,
    components: Vec<SpecComponent>,
}

#[derive(Deserialize)]
struct SpecComponent {
    id: String,
    #[serde(default)]
    name: String,
    #[serde(default)]
    required: bool,
    #[serde(default)]
    shared: bool,
    #[serde(default)]
    needs: Vec<String>,
    #[serde(default)]
    exe: Option<String>,
    #[serde(default)]
    version_from: Option<String>,
    files: Vec<SpecFile>,
    #[serde(default)]
    formats: Vec<String>,
    #[serde(default)]
    cleanup: Vec<String>,
    /// Each cleanup key with `{ext}` in it is written once for each of these.
    #[serde(default)]
    cleanup_exts: Vec<String>,
}

#[derive(Deserialize)]
struct SpecFile {
    to: String,
    from: String,
}

fn arg(name: &str) -> Option<String> {
    let args: Vec<String> = std::env::args().collect();
    args.iter().position(|a| a == name).and_then(|i| args.get(i + 1).cloned())
}

fn find_repo() -> PathBuf {
    let mut at = std::env::current_dir().unwrap();
    loop {
        if at.join("installer/payload.json").is_file() {
            return at;
        }
        if !at.pop() {
            panic!("run from inside the DotAny repository, or pass --repo");
        }
    }
}

/// The folder the programs were built into ({build} in payload.json): DOTANY_BUILD, or else the folder this program was
/// itself built into, when it sits in that layout (<build>/cargo-target-installer/release/dotany-pack.exe).
fn build_folder() -> String {
    if let Ok(b) = std::env::var("DOTANY_BUILD") {
        return b;
    }
    let exe = std::env::current_exe().ok();
    let release = exe.as_deref().and_then(|e| e.parent());
    let target = release.and_then(|r| r.parent());
    match (release, target) {
        (Some(r), Some(t))
            if r.file_name().is_some_and(|n| n == "release")
                && t.file_name().is_some_and(|n| n == "cargo-target-installer") =>
        {
            t.parent().unwrap().to_string_lossy().into_owned()
        }
        _ => panic!("set DOTANY_BUILD to the folder the programs were built into (<it>/cargo-target-<name>/release/)"),
    }
}

fn main() -> io::Result<()> {
    let repo = arg("--repo").map(PathBuf::from).unwrap_or_else(find_repo);
    let build = build_folder();
    let setup = arg("--setup")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(&build).join("cargo-target-installer/release/dotany-setup.exe"));
    let out_dir =
        arg("--out").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(&build).join("cargo-target-installer"));
    let level: i32 = arg("--level").and_then(|l| l.parse().ok()).unwrap_or(19);
    let spec: Spec = serde_json::from_slice(&fs::read(repo.join("installer/payload.json"))?).expect("payload.json");

    let resolve = |p: &str| -> PathBuf {
        let p = p.replace("{build}", &build);
        let path = PathBuf::from(&p);
        if path.is_absolute() { path } else { repo.join(path) }
    };

    // every file once, in the order the components list them. Anything missing stops the pack: an installer must never
    // carry a stand-in, or say it installed a program that isn't there
    let mut sources: Vec<(String, PathBuf)> = Vec::new();
    let mut components: Vec<Component> = Vec::new();
    let mut missing: Vec<String> = Vec::new();
    for c in &spec.components {
        for f in &c.files {
            let from = resolve(&f.from);
            if !from.is_file() {
                missing.push(format!("{}: {} isn't built", c.id, from.display()));
                continue;
            }
            if !sources.iter().any(|(to, _)| to == &f.to) {
                sources.push((f.to.clone(), from));
            }
        }
        let version = match &c.version_from {
            // a part with no program of its own (the fonts) has no version to read
            None => "0.0.0".to_string(),
            Some(v) => match fs::read(repo.join(v))
                .ok()
                .and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).ok())
                .and_then(|j| j["version"].as_str().map(String::from))
            {
                Some(version) => version,
                None => {
                    missing.push(format!("{}: no version in {}", c.id, repo.join(v).display()));
                    String::new()
                }
            },
        };
        components.push(Component {
            id: c.id.clone(),
            name: c.name.clone(),
            version,
            exe: c.exe.clone(),
            required: c.required,
            shared: c.shared,
            placeholder: false,
            files: c.files.iter().map(|f| f.to.clone()).collect(),
            formats: c.formats.clone(),
            needs: c.needs.clone(),
            cleanup: c
                .cleanup
                .iter()
                .flat_map(|k| {
                    if k.contains("{ext}") {
                        c.cleanup_exts.iter().map(|e| k.replace("{ext}", e)).collect()
                    } else {
                        vec![k.clone()]
                    }
                })
                .collect(),
        });
    }

    if !missing.is_empty() {
        eprintln!(
            "dotany-pack: nothing was packed, because {} {} missing:",
            missing.len(),
            if missing.len() == 1 { "thing is" } else { "things are" }
        );
        for m in &missing {
            eprintln!("  {m}");
        }
        eprintln!("build every program first, then pack again:");
        eprintln!("  bash .github/scripts/build-programs.sh <windows|macos>  (CARGO_TARGET_DIR and DOTANY_BUILD set)");
        std::process::exit(2);
    }

    fs::create_dir_all(&out_dir)?;
    let target = out_dir.join("DotAny-Setup.exe");
    let mut out = BufWriter::with_capacity(1 << 20, File::create(&target)?);
    let program = fs::read(&setup)?;
    out.write_all(&program)?;

    let started = std::time::Instant::now();
    let mut files = Vec::new();
    let mut offset = 0u64;
    let mut counter = Counter { inner: out, n: 0 };
    {
        let mut z = zstd::stream::write::Encoder::new(&mut counter, level)?;
        z.multithread(4)?;
        z.long_distance_matching(true)?;
        z.window_log(WINDOW_LOG)?;
        z.include_checksum(true)?;
        for (to, from) in &sources {
            let mut input = BufReader::with_capacity(1 << 20, File::open(from)?);
            let mut hash = Sha256::new();
            let mut buf = vec![0u8; 1 << 20];
            let mut size = 0u64;
            loop {
                let n = input.read(&mut buf)?;
                if n == 0 {
                    break;
                }
                hash.update(&buf[..n]);
                z.write_all(&buf[..n])?;
                size += n as u64;
            }
            files.push(FileEntry { path: to.clone(), size, sha256: hex(&hash.finalize()), offset });
            offset += size;
        }
        z.finish()?;
    }
    let stream_len = counter.n;
    let mut out = counter.inner;
    let release = Release {
        schema: SCHEMA,
        product: spec.product,
        version: spec.version,
        built: now(),
        components,
        files,
        claim: spec.claim,
    };
    let json = serde_json::to_vec(&release).unwrap();
    out.write_all(&json)?;
    out.write_all(MAGIC)?;
    for n in [program.len() as u64, stream_len, json.len() as u64] {
        out.write_all(&n.to_le_bytes())?;
    }
    out.flush()?;
    drop(out);
    debug_assert_eq!(FOOTER, 40);
    fs::write(out_dir.join("manifest.json"), serde_json::to_vec_pretty(&release).unwrap())?;

    let total = fs::metadata(&target)?.len();
    println!(
        "{} ({} bytes, {:.1} MB) in {:.0}s",
        target.display(),
        total,
        total as f64 / 1048576.0,
        started.elapsed().as_secs_f64()
    );
    println!(
        "installer program {:.1} MB, payload {:.1} MB packed from {:.1} MB",
        program.len() as f64 / 1048576.0,
        stream_len as f64 / 1048576.0,
        offset as f64 / 1048576.0
    );
    println!("\n| program | version | files | MB |\n|---|---|---|---|");
    for c in &release.components {
        let bytes: u64 = c.files.iter().filter_map(|p| release.file(p)).map(|f| f.size).sum();
        let label = if c.shared { format!("({})", c.id) } else { c.name.clone() };
        println!("| {} | {} | {} | {} |", label, c.version, c.files.join(", "), mb(bytes));
    }
    Ok(())
}

struct Counter<W: Write> {
    inner: W,
    n: u64,
}

impl<W: Write> Write for Counter<W> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let n = self.inner.write(buf)?;
        self.n += n as u64;
        Ok(n)
    }
    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}
