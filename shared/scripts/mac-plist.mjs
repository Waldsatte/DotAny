// Writes each program's src-tauri/Info.plist for the Mac: the kinds of file it opens (so a
// double-click in Finder, Open With and a drop on the Dock icon reach it), its web address for New, the reasons the Mac
// shows when it asks for the microphone or a folder, and the languages it speaks (WKWebView's navigator.language only
// offers the system's language when the bundle declares it). Tauri merges this file into the .app, and into the bare
// program built without a bundle.
//
//   node shared/scripts/mac-plist.mjs          write every program's Info.plist
//   node shared/scripts/mac-plist.mjs --check  exit 1 if any differs from what would be written
//
// The formats are installer/payload.json's, the same list Windows associates; WMV is left out (the Mac can't play it).
// The reasons are words of the family's language tables like any other (shared/web/lang, the Chinese below is the
// key): Info.plist has the English, and src-tauri/lproj/<language>.lproj/InfoPlist.strings has each
// language's, which the .app carries in Contents/Resources (shared/scripts/mac-app.mjs puts them in the throwaway bundle).
// The programs' names are the same in every language (DotAny Text…), so no language gives a CFBundleDisplayName of its own.
import { readFileSync, writeFileSync, existsSync, mkdirSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const REPO = join(dirname(fileURLToPath(import.meta.url)), '..', '..');
const payload = JSON.parse(readFileSync(join(REPO, 'installer', 'payload.json'), 'utf8'));
const FOLDER = { hub: 'hub', model: 'model' };
const NOT_ON_MAC = new Set(['wmv']);

// The system's own name (UTI) for a kind of file, where macOS has one that can be relied on. Any other is claimed by its
// extension: a document type with LSItemContentTypes ignores CFBundleTypeExtensions, so the two are separate entries.
const UTI = {
  txt: 'public.plain-text',
  log: 'com.apple.log',
  docx: 'org.openxmlformats.wordprocessingml.document',
  doc: 'com.microsoft.word.doc',
  rtf: 'public.rtf',
  odt: 'org.oasis-open.opendocument.text',
  json: 'public.json',
  xml: 'public.xml',
  html: 'public.html',
  htm: 'public.html',
  css: 'public.css',
  py: 'public.python-script',
  js: 'com.netscape.javascript-source',
  yaml: 'public.yaml',
  yml: 'public.yaml',
  jpg: 'public.jpeg',
  jpeg: 'public.jpeg',
  png: 'public.png',
  gif: 'com.compuserve.gif',
  bmp: 'com.microsoft.bmp',
  tif: 'public.tiff',
  tiff: 'public.tiff',
  ico: 'com.microsoft.ico',
  svg: 'public.svg-image',
  webp: 'org.webmproject.webp',
  mp3: 'public.mp3',
  wav: 'com.microsoft.waveform-audio',
  aiff: 'public.aiff-audio',
  aif: 'public.aiff-audio',
  m4a: 'com.apple.m4a-audio',
  flac: 'org.xiph.flac',
  mp4: 'public.mpeg-4',
  m4v: 'com.apple.m4v-video',
  mov: 'com.apple.quicktime-movie',
  avi: 'public.avi',
  pdf: 'com.adobe.pdf',
  zip: 'public.zip-archive',
  tar: 'public.tar-archive',
  gz: 'org.gnu.gnu-zip-archive',
  tgz: 'org.gnu.gnu-zip-tar-archive',
  xlsx: 'org.openxmlformats.spreadsheetml.sheet',
  xls: 'com.microsoft.excel.xls',
  csv: 'public.comma-separated-values-text',
  tsv: 'public.tab-separated-values-text',
  ods: 'org.oasis-open.opendocument.spreadsheet',
  pptx: 'org.openxmlformats.presentationml.presentation',
  ppsx: 'org.openxmlformats.presentationml.slideshow',
  potx: 'org.openxmlformats.presentationml.template',
  epub: 'org.idpf.epub-container',
  obj: 'public.geometry-definition-format',
  stl: 'public.standard-tesselated-geometry-format',
};
// Kinds a program that comes with the Mac already opens: DotAny is offered in Open With for them, and the hub's
// Default Apps makes it the default when asked. Every other kind DotAny opens by default once installed (the uncommon
// formats are associated automatically).
const MAC_HAS = new Set([
  'txt',
  'log',
  'docx',
  'doc',
  'rtf',
  'odt',
  'json',
  'xml',
  'html',
  'htm',
  'css',
  'py',
  'js',
  'yaml',
  'yml',
  'jpg',
  'jpeg',
  'jfif',
  'png',
  'gif',
  'bmp',
  'tif',
  'tiff',
  'ico',
  'svg',
  'webp',
  'mp3',
  'wav',
  'aiff',
  'aif',
  'm4a',
  'flac',
  'mp4',
  'm4v',
  'mov',
  'avi',
  'pdf',
  'zip',
  'tar',
  'gz',
  'tgz',
  'xlsx',
  'xls',
  'csv',
  'tsv',
  'ods',
  'pptx',
  'ppsx',
  'potx',
  'epub',
  'obj',
  'stl',
]);
// A program that writes the file back is its editor; the rest only show it.
const EDITS = new Set(['text', 'image', 'audio', 'pdf', 'archive', 'sheets', 'slides']);
const LANGS = ['zh-Hans', 'zh-Hant', 'en', 'ja', 'ko', 'es', 'pt-BR', 'fr', 'de'];
// each .lproj's language table (shared/web/lang/<code>.json; Chinese is the key itself). One Spanish and one
// Portuguese for everyone, as the pages pick them (shared/web/i18n.js)
const TABLE = { 'zh-Hans': 'zh', es: 'es-419' };

// The reasons the Mac shows under its own question ("DotAny Image" would like to access files in your Desktop folder):
// a folder's, when a program lists the files beside the one opened, opens a recent file or watches
// one; the microphone's, Recorder's.
const REASONS = {
  folder: '用于打开你选中的文件，并找到同一文件夹中的其他文件。',
  microphone: '麦克风用于录音。',
};
const tableOf = (code) => {
  const p = join(REPO, 'shared', 'web', 'lang', code + '.json');
  return code === 'zh' ? null : JSON.parse(readFileSync(p, 'utf8'));
};
/** A reason in a language: the table's word, Chinese as written. */
const say = (key, code) => {
  const t = tableOf(code);
  if (t && !(key in t)) throw new Error(`shared/web/lang/${code}.json has no ${key}`);
  return t ? t[key] : key;
};

const esc = (s) => s.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;');
function plist(v, pad = '') {
  const inner = pad + '\t';
  if (Array.isArray(v)) return `<array>\n${v.map((x) => inner + plist(x, inner)).join('\n')}\n${pad}</array>`;
  if (typeof v === 'object')
    return `<dict>\n${Object.entries(v)
      .map(([k, x]) => `${inner}<key>${esc(k)}</key>\n${inner}${plist(x, inner)}`)
      .join('\n')}\n${pad}</dict>`;
  if (typeof v === 'boolean') return v ? '<true/>' : '<false/>';
  return `<string>${esc(String(v))}</string>`;
}

function describe(c) {
  const role = EDITS.has(c.id) ? 'Editor' : 'Viewer';
  const formats = (c.formats || []).filter((f) => !NOT_ON_MAC.has(f));
  const types = [];
  for (const rank of ['Default', 'Alternate']) {
    const mine = formats.filter((f) => (MAC_HAS.has(f) ? 'Alternate' : 'Default') === rank);
    const utis = [...new Set(mine.filter((f) => UTI[f]).map((f) => UTI[f]))];
    const exts = mine.filter((f) => !UTI[f]);
    if (utis.length)
      types.push({
        CFBundleTypeName: `${c.name} (${rank === 'Default' ? 'opens' : 'also opens'})`,
        CFBundleTypeRole: role,
        LSHandlerRank: rank,
        LSItemContentTypes: utis,
      });
    if (exts.length)
      types.push({
        CFBundleTypeName: `${c.name} (${exts.join(', ')})`,
        CFBundleTypeRole: role,
        LSHandlerRank: rank,
        CFBundleTypeExtensions: exts,
      });
  }
  const scheme = c.id === 'hub' ? 'dotany' : 'dotany-' + c.id;
  const info = {
    CFBundleDevelopmentRegion: 'en',
    CFBundleLocalizations: LANGS,
    LSApplicationCategoryType: 'public.app-category.productivity',
    // New from DotAny reaches a program that is already open this way (open -a drops arguments then)
    CFBundleURLTypes: [{ CFBundleURLName: `com.dotany.${c.id}`, CFBundleURLSchemes: [scheme] }],
    ...reasons(c, 'en'),
  };
  if (types.length) info.CFBundleDocumentTypes = types;
  return info;
}

/** The program's reasons in one language: { NSDesktopFolderUsageDescription: '…', … }. */
function reasons(c, code) {
  const out = {};
  for (const k of ['Desktop', 'Documents', 'Downloads', 'RemovableVolumes', 'NetworkVolumes'])
    out[`NS${k}${k.endsWith('Volumes') ? '' : 'Folder'}UsageDescription`] = say(REASONS.folder, code);
  if (c.id === 'recorder') out.NSMicrophoneUsageDescription = say(REASONS.microphone, code);
  return out;
}

/** <lang>.lproj/InfoPlist.strings: the reasons in that language, as the Mac reads them (UTF-8 is accepted). */
const strings = (c, lang) =>
  `/* written by shared/scripts/mac-plist.mjs from shared/web/lang; change the language tables, not this file */\n`
  + Object.entries(reasons(c, TABLE[lang] || lang))
    .map(([k, v]) => `"${k}" = "${v.replace(/\\/g, '\\\\').replace(/"/g, '\\"')}";\n`)
    .join('');

const check = process.argv.includes('--check');
let differs = 0;
const programs = [
  ...payload.components.filter((c) => c.exe),
  { id: 'starter', name: 'Starter', formats: ['txt', 'md'] },
];
for (const c of programs) {
  const folder = c.id === 'starter' ? 'shared/starter' : FOLDER[c.id] || c.id;
  const file = join(REPO, folder, 'src-tauri', 'Info.plist');
  const text = `<?xml version="1.0" encoding="UTF-8"?>\n<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">\n<!-- written by shared/scripts/mac-plist.mjs from installer/payload.json; change that script, not this file -->\n<plist version="1.0">\n${plist(describe(c))}\n</plist>\n`;
  const files = [
    [file, text],
    ...LANGS.map((l) => [join(REPO, folder, 'src-tauri', 'lproj', l + '.lproj', 'InfoPlist.strings'), strings(c, l)]),
  ];
  for (const [f, made] of files) {
    if ((existsSync(f) ? readFileSync(f, 'utf8') : null) === made) continue;
    differs++;
    if (check) console.log('differs: ' + f);
    else {
      mkdirSync(dirname(f), { recursive: true });
      writeFileSync(f, made);
    }
  }
}
if (check && differs) process.exit(1);
console.log(
  check ? 'every Info.plist and InfoPlist.strings is as written' : `wrote ${differs} Info.plist and InfoPlist.strings`
);
