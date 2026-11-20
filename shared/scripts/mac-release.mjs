// Makes DotAny for the Mac as a person gets it: each program a real .app in the DotAny folder, then
// DotAny.dmg (that folder and a shortcut to Applications) and DotAny-mac.zip (the same folder, what the hub's updater
// downloads, hub/src-tauri/src/update.rs `mac`). Apple silicon only, macOS 13 or later.
//
//   node shared/scripts/mac-release.mjs <program>...   build each in release and put its .app in <out>/DotAny
//   node shared/scripts/mac-release.mjs --dmg          <out>/DotAny.dmg from <out>/DotAny
//   node shared/scripts/mac-release.mjs --zip          <out>/DotAny-mac.zip from <out>/DotAny
//   node shared/scripts/mac-release.mjs --package      both; each also writes <out>/SHA256SUMS-mac.txt for what it made
//
// A release signs the .apps, makes the dmg, notarises it once and staples it, staples the .apps (their
// tickets came with the dmg's), and only then makes the zip, so the .apps the hub puts in place carry their tickets.
//
//   --out=<folder>       default ~/dotany-target/dist
//   --no-build           no cargo, no page build: the .app made last (in <out>/DotAny, else ~/dotany-target/dist/DotAny)
//                        signed and checked again, e.g. with --hardened into another --out
//   --identity=<name>    the signature. "-" (the default): ad-hoc, which a Mac accepts for a program built on it and
//                        refuses for a downloaded one. A Developer ID ("Developer ID Application: … (TEAMID)", in the
//                        keychain): also the hardened runtime, a secure timestamp and each program's entitlements, all
//                        that notarisation asks of an .app; --package then signs the dmg with it too. Notarising and
//                        stapling the dmg is the release workflow's (it needs the Apple ID's secrets).
//   --hardened           with the ad-hoc signature: the hardened runtime and the entitlements as a Developer ID build
//                        has them, to try on this Mac that every program still runs under them.
//
// Cargo's target folder: CARGO_TARGET_DIR when it is set (CI: one for every program), else ~/dotany-target/<program>
// (the Mac's one target folder per program; the release profile sits beside the debug one).
//
// Tauri's own bundler makes each .app (`tauri build --bundles app`): it merges src-tauri/Info.plist (kinds of file, the
// web address, the reasons, CFBundleLocalizations: shared/scripts/mac-plist.mjs), puts in icons/icon.icns, and is told
// here, through --config so tauri.conf.json (and with it the Windows build) stays as it is, what only the Mac needs:
//   - LSMinimumSystemVersion 13.0;
//   - each language's InfoPlist.strings (src-tauri/lproj/<language>.lproj) in Contents/Resources;
//   - the hub: the shared fonts, the same files the Windows installer puts in its fonts folder (installer/payload.json's
//     "fonts"), in Contents/Resources/fonts, where every tool looks for them (shared/rust/src/fonts.rs);
//   - the hub, Audio and Recorder: libmp3lame.dylib in Contents/Frameworks (shared/sound-encode/src/lame.rs), its own
//     file there so a person can replace it (LGPL).
// Then this script copies it into the DotAny folder with ditto, signs it there (each library inside first, then the
// .app; codesign's --deep is for checking, not signing) and checks it (`check`, below).
import {
  cpSync,
  existsSync,
  mkdirSync,
  readFileSync,
  readdirSync,
  rmSync,
  statSync,
  writeFileSync,
  symlinkSync,
} from 'node:fs';
import { homedir, tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import { execFileSync, spawnSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { fileURLToPath } from 'node:url';
import { programOf } from './mac-app.mjs';

const REPO = join(dirname(fileURLToPath(import.meta.url)), '..', '..');
const LSREGISTER =
  '/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister';

/** The programs in the DotAny folder: the hub and the 11 tools (the Starter is a template, never shipped). */
export const PROGRAMS = [
  'hub',
  'text',
  'image',
  'audio',
  'video',
  'pdf',
  'archive',
  'sheets',
  'slides',
  'recorder',
  'books',
  'model',
];
/** The programs that write MP3 with LAME, and so carry libmp3lame.dylib. */
const LAME = new Set(['hub', 'audio', 'recorder']);
const MIN_MAC = '13.0';
const LANGUAGES = ['zh-Hans', 'zh-Hant', 'en', 'ja', 'ko', 'es', 'pt-BR', 'fr', 'de'];

/** What each program may do under the hardened runtime (a Developer ID build, or --hardened). */
export function entitlementsOf(name) {
  const e = {};
  // Recorder records from the microphone (NSMicrophoneUsageDescription is its reason)
  if (name === 'recorder') e['com.apple.security.device.audio-input'] = true;
  // LAME is LGPL: a person may put their own libmp3lame.dylib in Contents/Frameworks, which isn't covered by the app's signature,
  // so the system must let the program load a library it didn't sign
  if (LAME.has(name)) e['com.apple.security.cs.disable-library-validation'] = true;
  return e;
}

const plist = (keys) =>
  '<?xml version="1.0" encoding="UTF-8"?>\n<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" '
  + '"http://www.apple.com/DTDs/PropertyList-1.0.dtd">\n<plist version="1.0">\n<dict>\n'
  + Object.keys(keys)
    .map((k) => `\t<key>${k}</key>\n\t<true/>\n`)
    .join('')
  + '</dict>\n</plist>\n';

const run = (cmd, args, opts = {}) => execFileSync(cmd, args, { encoding: 'utf8', ...opts });
const say = (s) => console.log(s);

function targetOf(name) {
  return process.env.CARGO_TARGET_DIR || join(homedir(), 'dotany-target', name);
}

/** The shared fonts, as the Windows installer carries them: { 'fonts/x.ttf': '<repo>/shared/fonts/x.ttf' }. */
function fonts() {
  const payload = JSON.parse(readFileSync(join(REPO, 'installer', 'payload.json'), 'utf8'));
  const part = payload.components.find((c) => c.id === 'fonts');
  return Object.fromEntries(part.files.map((f) => [f.to, join(REPO, f.from)]));
}

/** What tauri build is told on top of tauri.conf.json, for the Mac only. */
function macConfig(name, p, target) {
  const files = {};
  for (const lang of LANGUAGES) {
    const from = join(p.dir, 'src-tauri', 'lproj', `${lang}.lproj`, 'InfoPlist.strings');
    if (!existsSync(from)) throw new Error(`no ${from} (node shared/scripts/mac-plist.mjs)`);
    files[`Resources/${lang}.lproj/InfoPlist.strings`] = from;
  }
  if (name === 'hub') for (const [to, from] of Object.entries(fonts())) files[`Resources/${to}`] = from;
  const macOS = { minimumSystemVersion: MIN_MAC, files };
  if (LAME.has(name)) macOS.frameworks = [join(target, 'release', 'libmp3lame.dylib')];
  return { mainBinaryName: p.bin, bundle: { active: true, targets: ['app'], macOS } };
}

function build(name, p, target) {
  const conf = join(tmpdir(), `dotany-release-mac-${name}.json`);
  writeFileSync(conf, JSON.stringify(macConfig(name, p, target), null, 2));
  rmSync(join(target, 'release', 'bundle', 'macos'), { recursive: true, force: true });
  say(`== ${name}: tauri build --bundles app (target ${target})`);
  // after `--`, cargo's own: exactly the crates Cargo.lock names, or stop
  execFileSync('npx', ['tauri', 'build', '--bundles', 'app', '--config', conf, '--', '--locked'], {
    cwd: p.dir,
    stdio: 'inherit',
    // The release profile strips (strip = true). Cargo strips the proc-macro libraries the compiler loads too, and the
    // Mac's strip (Command Line Tools 27) leaves those unloadable ("mis-aligned LINKEDIT string pool"), so the build
    // stops at the first derive. Only those are left as they are; the programs themselves are still stripped.
    env: { ...process.env, CARGO_TARGET_DIR: target, CARGO_PROFILE_RELEASE_BUILD_OVERRIDE_STRIP: 'false' },
  });
  rmSync(conf, { force: true });
}

function sign(app, name, identity, hardened) {
  const opts = ['--force', '--sign', identity];
  if (hardened) {
    opts.push('--options', 'runtime');
    if (identity !== '-') opts.push('--timestamp');
  }
  // inside out: each library, then the .app, whose signature seals them
  const fw = join(app, 'Contents', 'Frameworks');
  if (existsSync(fw)) for (const f of readdirSync(fw)) run('codesign', [...opts, join(fw, f)]);
  const e = entitlementsOf(name);
  if (hardened && Object.keys(e).length) {
    const file = join(tmpdir(), `dotany-entitlements-${name}.plist`);
    writeFileSync(file, plist(e));
    run('codesign', [...opts, '--entitlements', file, app]);
    rmSync(file, { force: true });
  } else run('codesign', [...opts, app]);
}

const info = (app) => JSON.parse(run('plutil', ['-convert', 'json', '-o', '-', join(app, 'Contents', 'Info.plist')]));

/** Everything a person's Mac needs from the bundle, or an Error saying what is missing. */
export function check(app, name) {
  const p = programOf(name);
  const i = info(app);
  const own = JSON.parse(run('plutil', ['-convert', 'json', '-o', '-', join(p.dir, 'src-tauri', 'Info.plist')]));
  const bad = [];
  const want = (k, v) => i[k] !== v && bad.push(`${k} is ${JSON.stringify(i[k])}, not ${JSON.stringify(v)}`);
  want('CFBundleExecutable', p.bin);
  want('CFBundleIdentifier', p.id.replace(/\.test$/, ''));
  want('CFBundleName', p.product);
  want('CFBundleShortVersionString', p.version);
  want('LSMinimumSystemVersion', MIN_MAC);
  want('CFBundlePackageType', 'APPL');
  for (const k of [
    'CFBundleLocalizations',
    'CFBundleDocumentTypes',
    'CFBundleURLTypes',
    'NSMicrophoneUsageDescription',
  ])
    if (k in own && JSON.stringify(i[k]) !== JSON.stringify(own[k])) bad.push(`${k} differs from src-tauri/Info.plist`);
  const res = join(app, 'Contents', 'Resources');
  if (!existsSync(join(res, (i.CFBundleIconFile || '').replace(/(\.icns)?$/, '.icns')))) bad.push('no icon');
  for (const lang of LANGUAGES)
    if (!existsSync(join(res, `${lang}.lproj`, 'InfoPlist.strings'))) bad.push(`no ${lang}.lproj/InfoPlist.strings`);
  if (name === 'hub')
    for (const to of Object.keys(fonts())) if (!existsSync(join(res, to))) bad.push(`no Resources/${to}`);
  const exe = join(app, 'Contents', 'MacOS', p.bin);
  const code = [exe];
  if (LAME.has(name)) code.push(join(app, 'Contents', 'Frameworks', 'libmp3lame.dylib'));
  for (const f of code) {
    if (!existsSync(f)) bad.push(`no ${f.slice(app.length + 1)}`);
    else if (run('lipo', ['-archs', f]).trim() !== 'arm64') bad.push(`${f.slice(app.length + 1)} is not arm64 only`);
  }
  try {
    run('codesign', ['--verify', '--deep', '--strict', '--verbose=2', app], { stdio: 'pipe' });
  } catch (e) {
    bad.push('codesign --verify: ' + String(e.stderr || e.message).trim());
  }
  if (bad.length) throw new Error(`${app}:\n  ` + bad.join('\n  '));
}

/** Bytes of a folder or file, as `du -sk` counts them. */
export const sizeOf = (path) => Number(run('du', ['-sk', path]).split(/\s/)[0]) * 1024;
const mb = (b) => (b / 1048576).toFixed(1) + ' MB';

export function makeProgram(name, { out, identity = '-', hardened = false, doBuild = true }) {
  const p = programOf(name);
  const target = targetOf(name);
  if (doBuild) build(name, p, target);
  const made = join(target, 'release', 'bundle', 'macos', `${p.product}.app`);
  const folder = join(out, 'DotAny');
  const app = join(folder, `${p.product}.app`);
  // what the bundler just made; with --no-build, the copy already in this DotAny folder or in the default one
  const from = [made, app, join(homedir(), 'dotany-target', 'dist', 'DotAny', `${p.product}.app`)].find((a) =>
    existsSync(a)
  );
  if (!from) throw new Error('no ' + made + ' (build it first)');
  mkdirSync(folder, { recursive: true });
  if (from !== app) {
    rmSync(app, { recursive: true, force: true });
    run('ditto', [from, app]);
  }
  sign(app, name, identity, hardened || identity !== '-');
  check(app, name);
  // The Mac lists an .app among its programs as soon as it notices one (Spotlight does, within seconds), and would hand
  // files to whichever copy it likes: the bundler's copy goes once it is in the DotAny folder (the next build makes it
  // again), and the list forgets both.
  for (const a of [made, app]) spawnSync(LSREGISTER, ['-u', a]);
  rmSync(made, { recursive: true, force: true });
  say(`${name}: ${app} ${mb(sizeOf(app))} (version ${p.version})`);
  return app;
}

const sha256 = (file) => createHash('sha256').update(readFileSync(file)).digest('hex');

/** The DotAny folder, checked: DotAny.app in it, every .app's signature whole. */
function folderOf(out) {
  const folder = join(out, 'DotAny');
  const apps = readdirSync(folder).filter((f) => f.endsWith('.app'));
  if (!apps.includes('DotAny.app')) throw new Error('no DotAny.app in ' + folder);
  for (const a of apps) run('codesign', ['--verify', '--deep', '--strict', join(folder, a)]);
  return folder;
}

/** <out>/DotAny.dmg: the DotAny folder and a shortcut to Applications. */
export function makeDmg({ out, identity = '-' }) {
  const folder = folderOf(out);
  // the dmg's window: the DotAny folder and a shortcut to Applications (a symbolic link, which Finder shows with the
  // Applications folder's icon), as every Mac program's dmg has them
  const stage = join(out, '.dmg-stage');
  rmSync(stage, { recursive: true, force: true });
  mkdirSync(stage);
  run('ditto', [folder, join(stage, 'DotAny')]);
  symlinkSync('/Applications', join(stage, 'Applications'));
  const dmg = join(out, 'DotAny.dmg');
  rmSync(dmg, { force: true });
  // HFS+ (the file system every dmg made for a range of macOS versions uses); ULMO: LZMA, readable since macOS 10.15
  run('hdiutil', ['create', '-volname', 'DotAny', '-srcfolder', stage, '-fs', 'HFS+', '-format', 'ULMO', '-ov', dmg]);
  rmSync(stage, { recursive: true, force: true });
  if (identity !== '-') run('codesign', ['--force', '--sign', identity, '--timestamp', dmg]);
  say(`DotAny folder ${mb(sizeOf(folder))}; ${dmg} ${mb(statSync(dmg).size)}`);
  return dmg;
}

/** <out>/DotAny-mac.zip, the hub's update: the DotAny folder zipped with its links, permissions and signatures kept
 * (update.rs unpacks it with ditto). Made after the dmg is notarised, so the .apps in it carry their stapled tickets. */
export function makeZip({ out }) {
  const folder = folderOf(out);
  const zip = join(out, 'DotAny-mac.zip');
  rmSync(zip, { force: true });
  run('ditto', ['-c', '-k', '--keepParent', folder, zip]);
  say(`${zip} ${mb(statSync(zip).size)}`);
  return zip;
}

/** SHA256SUMS-mac.txt: sha256sum's lines (`<hex>  <name>`), the form the hub reads from a release's SHA256SUMS.txt. */
export function writeSums(out, files) {
  const sums = files.map((f) => `${sha256(f)}  ${f.slice(out.length + 1)}`).join('\n') + '\n';
  writeFileSync(join(out, 'SHA256SUMS-mac.txt'), sums);
  say(sums.trim());
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  if (process.platform !== 'darwin') throw new Error('mac-release.mjs is for the Mac');
  const args = process.argv.slice(2);
  const flag = (n) => args.find((a) => a.startsWith(`--${n}=`))?.slice(n.length + 3);
  const out = flag('out') || join(homedir(), 'dotany-target', 'dist');
  const identity = flag('identity') || '-';
  const names = args.filter((a) => !a.startsWith('--'));
  for (const n of names) if (!PROGRAMS.includes(n)) throw new Error(`${n} is not one of ${PROGRAMS.join(' ')}`);
  for (const n of names)
    makeProgram(n, { out, identity, hardened: args.includes('--hardened'), doBuild: !args.includes('--no-build') });
  const both = args.includes('--package');
  const made = [];
  if (both || args.includes('--dmg')) made.push(makeDmg({ out, identity }));
  if (both || args.includes('--zip')) made.push(makeZip({ out }));
  if (made.length) writeSums(out, made);
  if (!names.length && !made.length) {
    console.error('say which programs (' + PROGRAMS.join(' ') + '), or --dmg, --zip, --package');
    process.exit(2);
  }
}
