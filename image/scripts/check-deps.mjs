// DotAny Image leaves some formats out on purpose, and no code for them is compiled in, not even to read them. This
// fails the build if the resolved dependency graph holds any crate known to carry one. It runs on every build.
import { execFileSync } from 'node:child_process';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');
const FORBIDDEN =
  /^(rav1e|ravif|dav1d|dav1d-sys|libavif|libavif-sys|avif-parse|avif-serialize|mp4parse|libheif|libheif-rs|libheif-sys|heic|kamadak-heic|jxl-oxide|jpegxl-rs|jpegxl-sys|libjxl|openjp2|jpeg2k|psd|rawloader|rawler|libraw|libraw-sys|x264|x265|openh264|ffmpeg.*|imagequant)$/;
// the image crate's own switches for formats we don't want; none may be on (WebP goes through libwebp instead)
const FORBIDDEN_FEATURES = ['avif', 'avif-native', 'hdr', 'exr', 'dds', 'webp'];
// every one of the image crate's format switches, and the few of them that may be on: a switch that isn't on the
// allowed list fails the build too, so a format can only come in by being written here. tga, qoi, pnm and ff are only
// read
const FORMAT_FEATURES = [
  'avif',
  'avif-native',
  'bmp',
  'dds',
  'exr',
  'ff',
  'gif',
  'hdr',
  'ico',
  'jpeg',
  'png',
  'pnm',
  'qoi',
  'tga',
  'tiff',
  'webp',
];
const ALLOWED_FEATURES = ['bmp', 'ico', 'jpeg', 'png', 'tga', 'qoi', 'pnm', 'ff'];

// both systems the program is built for: a crate or a switch only the Mac's build pulls in is checked as well as
// Windows'. cargo resolves the graph for a target whether or not that target is installed
const PLATFORMS = ['x86_64-pc-windows-msvc', 'aarch64-apple-darwin'];

/** The resolved graph of every platform as one: each crate by its id, with its name and every switch any build turns on. */
function resolved() {
  const crates = new Map();
  for (const platform of PLATFORMS) {
    const meta = JSON.parse(
      execFileSync('cargo', ['metadata', '--format-version', '1', '--filter-platform', platform], {
        cwd: join(root, 'src-tauri'),
        encoding: 'utf8',
        maxBuffer: 2e8,
      })
    );
    const byId = new Map(meta.packages.map((p) => [p.id, p]));
    for (const n of meta.resolve.nodes) {
      const features = new Set([...(crates.get(n.id)?.features || []), ...n.features]);
      crates.set(n.id, { name: byId.get(n.id).name, features: [...features] });
    }
  }
  return [...crates.values()];
}

export function checkDeps() {
  const crates = resolved();
  const bad = [...new Set(crates.map((c) => c.name).filter((n) => FORBIDDEN.test(n)))];
  const features = [...new Set(crates.filter((c) => c.name === 'image').flatMap((c) => c.features))];
  const on = features.filter(
    (f) => FORBIDDEN_FEATURES.includes(f) || (FORMAT_FEATURES.includes(f) && !ALLOWED_FEATURES.includes(f))
  );
  if (bad.length || on.length)
    throw new Error(`Forbidden codec in the build: ${[...bad, ...on.map((f) => 'image/' + f)].join(', ')}`);
  return { crates: crates.length, imageFeatures: features };
}
if (process.argv[1] && import.meta.url.endsWith(process.argv[1].replace(/\\/g, '/')))
  console.log(JSON.stringify(checkDeps()));
