// Generates the licence notices from what is really in the program: the Rust crates compiled for Windows and the JavaScript
// packages esbuild actually bundles. Licence texts come from each package's own files; identical texts are printed once.
// Writes src/notices.generated.js, which the licence page loads when it is opened. Run by scripts/build.mjs.
import { execFileSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { existsSync, readdirSync, readFileSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { build } from 'esbuild';
import { mplSources, ownLicence } from '../../shared/scripts/own-licence.mjs';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');
const LICENCE_FILE = /^(licen[sc]e|copying|notice|copyright|unlicense|authors)/i;
// crates published without their licence file; the text is kept from each one's upstream:
// github.com/dropbox/rust-alloc-no-stdlib LICENSE, github.com/wravery/webview2-rs LICENSE, mozilla.org/MPL/2.0 (selectors, from servo/stylo)
const UPSTREAM = {
  'alloc-stdlib': 'alloc-stdlib.txt',
  selectors: 'selectors.txt',
  'webview2-com': 'webview2-com.txt',
  'webview2-com-macros': 'webview2-com.txt',
  'webview2-com-sys': 'webview2-com.txt',
};

function licenceFiles(dir, license) {
  if (!existsSync(dir)) return [];
  const all = readdirSync(dir, { withFileTypes: true })
    .filter((e) => e.isFile() && LICENCE_FILE.test(e.name))
    .map((e) => e.name);
  // with a choice of licences, MIT is enough to carry; a NOTICE file always travels
  const either = /\bOR\b|\//.test(license || '') && !/\bAND\b/.test(license || '');
  const picked =
    either && /MIT/.test(license) && all.some((n) => /mit/i.test(n)) ? all.filter((n) => /mit|notice/i.test(n)) : all;
  return picked.filter((n) => !/^authors/i.test(n) || !picked.some((m) => /^licen/i.test(m)));
}

export async function generate() {
  const packages = [];
  const target = join(root, 'src/notices.generated.js');
  // the bundle below imports this file, so it has to exist the first time
  if (!existsSync(target)) writeFileSync(target, 'export const NOTICES = ""; export const NOTICE_COUNT = 0;');

  // ---------- Rust: walk the resolved graph from our crate, normal dependencies only, for this target ----------
  const meta = JSON.parse(
    execFileSync('cargo', ['metadata', '--format-version', '1', '--filter-platform', 'x86_64-pc-windows-msvc'], {
      cwd: join(root, 'src-tauri'),
      encoding: 'utf8',
      maxBuffer: 2e8,
    })
  );
  const nodes = new Map(meta.resolve.nodes.map((n) => [n.id, n]));
  const byId = new Map(meta.packages.map((p) => [p.id, p]));
  const seen = new Set(),
    stack = [meta.resolve.root];
  while (stack.length) {
    const id = stack.pop();
    if (seen.has(id)) continue;
    seen.add(id);
    for (const d of nodes.get(id).deps) if (d.dep_kinds.some((k) => k.kind === null)) stack.push(d.pkg);
  }
  for (const id of seen) {
    // our own crates, the tool's and dotany-base, come from a folder rather than a registry
    if (id === meta.resolve.root || byId.get(id).source == null) continue;
    const p = byId.get(id);
    packages.push({
      kind: 'rust',
      name: p.name,
      version: p.version,
      license: p.license || (p.license_file ? 'see file' : 'unknown'),
      dir: dirname(p.manifest_path),
      authors: (p.authors || []).join(', '),
      source: p.source,
    });
  }

  // ---------- JavaScript: only what ends up in the bundle ----------
  const { bundleOptions } = await import('../../shared/scripts/build.mjs');
  const bundle = await build({
    entryPoints: [join(root, 'src/main.js')],
    bundle: true,
    splitting: true,
    format: 'esm',
    write: false,
    outdir: join(root, 'dist'),
    metafile: true,
    logLevel: 'silent',
    ...bundleOptions(root),
  });
  const jsNames = new Set();
  for (const input of Object.keys(bundle.metafile.inputs)) {
    const m = input.match(/node_modules\/((?:@[^/]+\/)?[^/]+)\//);
    if (m) jsNames.add(m[1]);
  }
  for (const name of jsNames) {
    const dir = join(root, 'node_modules', name),
      pkg = JSON.parse(readFileSync(join(dir, 'package.json'), 'utf8'));
    packages.push({
      kind: 'js',
      name,
      version: pkg.version,
      license: pkg.license || 'unknown',
      dir,
      authors: typeof pkg.author === 'string' ? pkg.author : (pkg.author && pkg.author.name) || '',
    });
  }
  packages.sort((a, b) => (a.kind + a.name).localeCompare(b.kind + b.name));

  // ---------- group identical texts ----------
  const texts = new Map(),
    noText = [];
  for (const p of packages) {
    const found = licenceFiles(p.dir, p.license).map((f) => join(p.dir, f));
    const files =
      found.length || !UPSTREAM[p.name] ? found : [join(root, 'scripts/upstream-licences', UPSTREAM[p.name])];
    if (!files.length) noText.push(p);
    for (const f of files) {
      const text = readFileSync(f, 'utf8').replace(/\r\n/g, '\n').trim();
      const key = createHash('sha1').update(text).digest('hex');
      if (!texts.has(key)) texts.set(key, { text, who: [] });
      texts.get(key).who.push(`${p.name} ${p.version}`);
    }
  }
  const line = '-'.repeat(80);
  // the family's fonts' licence from the one shared copy; JetBrains Mono and Unscii are Text's own
  const fonts = [
    join(root, '../shared/fonts/OFL-StackSans.txt'),
    join(root, 'fonts/OFL-JetBrainsMono.txt'),
    join(root, '../shared/fonts/OFL-NotoSansJP.txt'),
    join(root, '../shared/fonts/OFL-Pretendard.txt'),
  ].map((f) => readFileSync(f, 'utf8').replace(/\r\n/g, '\n').trim());
  const out = [
    `${line}\nStack Sans\n\n${fonts[0]}`,
    `${line}\nJetBrains Mono\n\n${fonts[1]}`,
    `${line}\nNoto Sans JP (cut to common kanji, as Noto Sans JP DotAny)\n\n${fonts[2]}`,
    `${line}\nPretendard\n\n${fonts[3]}`,
    `${line}\nUnscii 16 (DOS text art)\n\n${readFileSync(join(root, 'fonts/Unscii-PUBLIC-DOMAIN.txt'), 'utf8').replace(/\r\n/g, '\n').trim()}`,
    // Text's icons are copied from lucide-static into src/icons.js and src/code-icons.js
    `${line}\nLucide icons (ISC; some drawn from Feather, MIT)\n\n${readFileSync(join(root, '../shared/upstream-licences/lucide-static-LICENSE.txt'), 'utf8').replace(/\r\n/g, '\n').trim()}`,
    `${line}\nEvery library in this program (${packages.length}):\n\n`
      + packages.map((p) => `${p.name} ${p.version}  ${p.license}`).join('\n'),
    ...mplSources(packages),
    ...[...texts.values()].map((t) => `${line}\n${t.who.join(', ')}\n\n${t.text}`),
  ];
  if (noText.length)
    out.push(
      `${line}\nNo licence file in the package; licensed as stated above, by its authors:\n\n`
        + noText.map((p) => `${p.name} ${p.version}  ${p.license}  ${p.authors}`).join('\n')
    );

  const count = (re) => packages.filter((p) => re.test(p.license)).length;
  const summary = {
    total: packages.length,
    rust: packages.filter((p) => p.kind === 'rust').length,
    js: packages.filter((p) => p.kind === 'js').length,
    withoutText: noText.map((p) => `${p.name} (${p.license})`),
    mpl: packages.filter((p) => /MPL/.test(p.license)).map((p) => `${p.name} ${p.version}`),
    copyleft: packages.filter((p) => /GPL/.test(p.license) && !/\bOR\b/.test(p.license)).map((p) => p.name),
    unknown: count(/unknown/),
  };
  const own = ownLicence({ root, name: 'DotAny Text' });
  summary.own = own.licence;
  const content = `// Generated by scripts/notices.mjs. Do not edit.\nexport const OWN_LICENCE = ${JSON.stringify(own.licence)};\nexport const OWN = ${JSON.stringify(own.text)};\nexport const NOTICES = ${JSON.stringify(out.join('\n\n'))};\nexport const NOTICE_COUNT = ${packages.length};\n`;
  if (!existsSync(target) || readFileSync(target, 'utf8') !== content) writeFileSync(target, content);
  return { ...summary, kb: Math.round(content.length / 1024) };
}

if (process.argv[1] && fileURLToPath(import.meta.url) === process.argv[1].replace(/\//g, '\\'))
  console.log(JSON.stringify(await generate(), null, 1));
