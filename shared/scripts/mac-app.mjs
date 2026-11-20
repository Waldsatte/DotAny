// Wraps a program built on the Mac in the least .app the Mac treats as a program, for testing:
// files the system hands over (a double-click, `open -a`, a drop on the Dock icon) reach a program only as
// RunEvent::Opened, and only when it runs from a bundle. A bare cargo build has none.
//
//   node shared/scripts/mac-app.mjs <program> [--target=<folder>] [--release]   make it, print its path
//   node shared/scripts/mac-app.mjs <program> --forget [--target=<folder>]     take it off the Mac's list of programs
//
// The bundle is <target>/apps/<product name>.app (target: ~/dotany-target, the Mac's one cargo target folder per
// program), never in /Applications: Contents/MacOS/<the program> is a hard link to <target>/<program>/debug/<binary>,
// made again each time (cargo replaces the file it builds, so an old link would keep the old program), and
// Contents/Info.plist is the program's own src-tauri/Info.plist (its kinds of file, its web address, its reasons) with
// what a bundle needs added: its name, the program to run, its icon (src-tauri/icons/icon.icns, copied to
// Contents/Resources) and an identifier. The identifier is the program's own with
// `.test` after it, so this bundle never stands for the real program in the Mac's list of programs or WebKit's
// folders; it is listed there all the same once opened (`open -a` registers it), so --forget takes it off again when
// the run is done. Nothing is signed: a program built on this Mac runs as it is. Contents/Resources has each
// language's reasons, the program's src-tauri/lproj/<language>.lproj (shared/scripts/mac-plist.mjs writes them).
import { cpSync, existsSync, linkSync, mkdirSync, readFileSync, rmSync, writeFileSync, copyFileSync } from 'node:fs';
import { homedir } from 'node:os';
import { dirname, join } from 'node:path';
import { execFileSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';

const REPO = join(dirname(fileURLToPath(import.meta.url)), '..', '..');
const DIRS = { starter: 'shared/starter' };
const LSREGISTER =
  '/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister';

const xml = (s) => s.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;');

/** What a program is: its folder, its binary's name (Cargo's [[bin]]), its name and identifier (tauri.conf.json). */
export function programOf(name) {
  const dir = join(REPO, DIRS[name] || name);
  const cargo = readFileSync(join(dir, 'src-tauri', 'Cargo.toml'), 'utf8');
  const bin = /\[\[bin\]\][^[]*?name\s*=\s*"([^"]+)"/.exec(cargo)?.[1] || /^name\s*=\s*"([^"]+)"/m.exec(cargo)[1];
  const conf = JSON.parse(readFileSync(join(dir, 'src-tauri', 'tauri.conf.json'), 'utf8'));
  return { name, dir, bin, product: conf.productName, id: conf.identifier + '.test', version: conf.version };
}

export const appPath = (name, target = join(homedir(), 'dotany-target')) =>
  join(target, 'apps', programOf(name).product + '.app');

/** Makes (again) the throwaway bundle of `name` from its built binary; gives its path. */
export function makeApp(name, { target = join(homedir(), 'dotany-target'), profile = 'debug' } = {}) {
  const p = programOf(name);
  const exe = join(target, name, profile, p.bin);
  if (!existsSync(exe)) throw new Error('no program at ' + exe + ' (build it first)');
  const app = appPath(name, target);
  rmSync(app, { recursive: true, force: true });
  mkdirSync(join(app, 'Contents', 'MacOS'), { recursive: true });
  const inside = join(app, 'Contents', 'MacOS', p.bin);
  try {
    linkSync(exe, inside);
  } catch {
    // another disk: a copy
    copyFileSync(exe, inside);
  }
  // the program's Mac icon, which the Dock and Finder draw as they draw every program's
  const icon = join(p.dir, 'src-tauri', 'icons', 'icon.icns');
  if (!existsSync(icon))
    throw new Error('no Mac icon at ' + icon + ' (every program keeps its Mac icon as src-tauri/icons/icon.icns)');
  mkdirSync(join(app, 'Contents', 'Resources'), { recursive: true });
  copyFileSync(icon, join(app, 'Contents', 'Resources', 'icon.icns'));
  const own = readFileSync(join(p.dir, 'src-tauri', 'Info.plist'), 'utf8');
  const keys = {
    CFBundleIconFile: 'icon',
    CFBundleExecutable: p.bin,
    CFBundleIdentifier: p.id,
    CFBundleName: p.product,
    CFBundleDisplayName: p.product,
    CFBundlePackageType: 'APPL',
    CFBundleInfoDictionaryVersion: '6.0',
    CFBundleShortVersionString: p.version,
    CFBundleVersion: p.version,
    LSMinimumSystemVersion: '13.0',
  };
  const added =
    Object.entries(keys)
      .map(([k, v]) => `\t<key>${k}</key>\n\t<string>${xml(v)}</string>\n`)
      .join('') + '\t<key>NSHighResolutionCapable</key>\n\t<true/>\n';
  const at = own.indexOf('<dict>') + '<dict>'.length;
  if (at < '<dict>'.length) throw new Error('no <dict> in ' + p.dir + '/src-tauri/Info.plist');
  writeFileSync(
    join(app, 'Contents', 'Info.plist'),
    own.slice(0, at) + '\n' + added + own.slice(at).replace(/^\n/, '')
  );
  const lproj = join(p.dir, 'src-tauri', 'lproj');
  if (existsSync(lproj)) cpSync(lproj, join(app, 'Contents', 'Resources'), { recursive: true });
  return app;
}

/** Takes the throwaway bundle off the Mac's list of programs (Launch Services), and away. */
export function forgetApp(name, { target = join(homedir(), 'dotany-target') } = {}) {
  const app = appPath(name, target);
  if (existsSync(app)) {
    try {
      execFileSync(LSREGISTER, ['-u', app], { stdio: 'ignore' });
    } catch {
      /* never listed */
    }
    rmSync(app, { recursive: true, force: true });
  }
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  const args = process.argv.slice(2);
  const flag = (n) => args.find((a) => a.startsWith(`--${n}=`))?.slice(n.length + 3);
  const name = args.find((a) => !a.startsWith('--'));
  if (!name) {
    console.error('say which program: node shared/scripts/mac-app.mjs image');
    process.exit(2);
  }
  if (process.platform !== 'darwin') throw new Error('mac-app.mjs is for the Mac');
  const target = flag('target') || join(homedir(), 'dotany-target');
  if (args.includes('--forget')) forgetApp(name, { target });
  else console.log(makeApp(name, { target, profile: args.includes('--release') ? 'release' : 'debug' }));
}
