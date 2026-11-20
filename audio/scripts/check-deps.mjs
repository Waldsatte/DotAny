// DotAny Audio leaves some formats out on purpose, and no code for them is compiled in, not even to read them. This
// fails the build if the resolved dependency graph holds a crate or a Symphonia switch for one, or if LAME's decoder or
// anything calling itself GPL slips in. It runs on every build.
// What is compiled from vendor/ is an allowlist too: only the libraries in VENDORED, and libopenmpt (tracker modules)
// with none of its codec switches on and none of the decoders it can carry present. Audio's C is built by the crates that
// bind it, so their vendor/ folders and build scripts are checked with Audio's own: LAME and libFLAC by
// dotany-sound-encode (shared/sound-encode), libopenmpt by dotany-sound (shared/sound).
import { execFileSync } from 'node:child_process';
import { existsSync, readdirSync, readFileSync } from 'node:fs';
import { dirname, join, relative, sep } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');
const ENCODE = join(root, '../shared/sound-encode'),
  SOUND = join(root, '../shared/sound');
const FORBIDDEN =
  /^(symphonia-codec-aac|symphonia-codec-alac|symphonia-codec-adpcm|symphonia-format-isomp4|symphonia-format-mkv|symphonia-format-caf|fdk-aac.*|faad.*|opus.*|audiopus.*|libopus.*|magnum-opus|mp4.*|ffmpeg.*|mp3lame.*|minimp3.*|puremp3|openmpt.*)$/;
// Symphonia's own switches for formats we don't want; none may be on
const FORBIDDEN_FEATURES = [
  'aac',
  'alac',
  'adpcm',
  'isomp4',
  'mkv',
  'caf',
  'mp1',
  'mp2',
  'mpa',
  'all',
  'all-codecs',
  'all-formats',
];
// what may be compiled from vendor/: LAME's encoder (LGPL, its own DLL), libFLAC (BSD), libopenmpt (BSD)
const VENDORED = ['flac', 'lame-3.100', 'libopenmpt'];
// libopenmpt's switches for the libraries it can use: zlib and miniz, mpg123 and minimp3, Ogg, Vorbis and stb_vorbis,
// FLAC, Opus, Media Foundation. Through them it would decode MP3 and Vorbis samples inside MO3 files. None may be on.
const OPENMPT_SWITCH = /MPT_WITH_[A-Z0-9_]+/;
// the folder its release carries those decoders in, and their own names
const OPENMPT_THIRD_PARTY = [
  'include',
  'include/minimp3',
  'include/stb_vorbis',
  'include/miniz',
  'include/mpg123',
  'include/vorbis',
  'include/ogg',
  'include/zlib',
];

// libopenmpt as shared/sound/build.rs builds it
function openmptProblems(buildRs) {
  const from = buildRs.indexOf('fn build_openmpt'),
    to = buildRs.indexOf('fn main');
  if (from < 0) return ['shared/sound/build.rs: no build_openmpt'];
  const problems = buildRs
    .slice(from, to)
    .split('\n')
    .filter((l) => OPENMPT_SWITCH.test(l) && !/^\s*\/\//.test(l))
    .map((l) => 'openmpt/' + l.match(OPENMPT_SWITCH)[0]);
  const lib = join(SOUND, 'vendor/libopenmpt');
  problems.push(...OPENMPT_THIRD_PARTY.filter((d) => existsSync(join(lib, d))).map((d) => 'openmpt/' + d));
  return problems;
}

// both systems the program is built for: a crate or a switch only the Mac's build pulls in is checked as well as
// Windows'. cargo resolves the graph for a target whether or not that target is installed
const PLATFORMS = ['x86_64-pc-windows-msvc', 'aarch64-apple-darwin'];

/** The resolved graph of every platform as one: each crate's package by its id, and every switch any build turns on. */
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
      crates.set(n.id, { package: byId.get(n.id), features: [...features] });
    }
  }
  return [...crates.values()];
}

export function checkDeps() {
  const crates = resolved();
  const names = crates.map((c) => c.package);
  const bad = [...new Set(names.map((p) => p.name).filter((n) => FORBIDDEN.test(n)))];
  const symphonia = crates.some((c) => c.package.name === 'symphonia')
    ? { features: [...new Set(crates.filter((c) => c.package.name === 'symphonia').flatMap((c) => c.features))] }
    : null;
  const on = symphonia ? symphonia.features.filter((f) => FORBIDDEN_FEATURES.includes(f)) : [];
  // nothing that's only GPL. LGPL is LAME's own, and it isn't a crate
  const gpl = names
    .filter((p) => /GPL/.test(p.license || '') && !/\bOR\b|\//.test(p.license || ''))
    .map((p) => `${p.name} (${p.license})`);
  // LAME's decoder must not be here at all
  const lame = join(ENCODE, 'vendor/lame-3.100');
  const decoder = ['mpglib', 'libmp3lame/mpglib_interface.c'].filter((f) => existsSync(join(lame, f)));
  const buildRs = (dir) => readFileSync(join(dir, 'build.rs'), 'utf8');
  const defined = [join(root, 'src-tauri'), ENCODE, SOUND].some((d) =>
    buildRs(d)
      .split('\n')
      .some((l) => /\.define\("HAVE_MPGLIB"/.test(l))
  );
  // what may sit in a vendor/ folder Audio compiles from: Audio's own, if it has one, and the two crates'
  const strangers = [root, ENCODE, SOUND]
    .filter((d) => existsSync(join(d, 'vendor')))
    .flatMap((d) =>
      readdirSync(join(d, 'vendor'))
        .filter((v) => !VENDORED.includes(v))
        .map((v) =>
          relative(join(root, '..'), join(d, 'vendor', v))
            .split(sep)
            .join('/')
        )
    );
  const problems = [
    ...bad,
    ...on.map((f) => 'symphonia/' + f),
    ...gpl,
    ...decoder.map((f) => 'lame/' + f),
    ...(defined ? ['HAVE_MPGLIB'] : []),
    ...strangers,
    ...openmptProblems(buildRs(SOUND)),
  ];
  if (problems.length) throw new Error(`Forbidden codec or licence in the build: ${problems.join(', ')}`);
  return { crates: crates.length, symphonia: symphonia ? symphonia.features : [], vendored: VENDORED };
}
if (process.argv[1] && fileURLToPath(import.meta.url) === process.argv[1].replace(/\//g, '\\'))
  console.log(JSON.stringify(checkDeps()));
