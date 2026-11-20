// ---------- saving under another name: the family's card (shell.js), with what Text can write ----------
// What each kind of file can become. The first is its own format; the others write a new file and this window stays where
// it is. The format's line is said at the card's foot, as Text has no size estimate to show there; a file that can't be
// held in its encoding asks first and the card stays.
import { app } from '@dotany/web/shared.js';
import * as be from './backend.js';
import { baseName } from '@dotany/web/paths.js';
import { currentText, cur, settleSaveAs, editStamp } from './files.js';
import { view } from './doc-editor.js';
import { docToBlocks, docToPlain } from './docx-view.js';
import { serializeMarkdown, serializeText } from './markdown.js';
import { FORMAT, KINDS } from './formats.js';
import { T } from './i18n.js';

/** The formats the open file offers, its own first */
export const formatsFor = () => (cur.source ? KINDS.source : KINDS[app.kind] || KINDS.md);
// what is only ever viewed: every choice is a new file
const ART = ['nfo', 'ans'];
// a new or plain file keeps the extension it already has
app.refreshSaveFormats = (made) => {
  const ext = (cur.name.match(/\.[^.]+$/) || [''])[0];
  if (app.kind === 'plain') {
    FORMAT.plain.ext = ext;
    FORMAT.plain.label = made ? made.label : '保留排版';
  }
  if (app.kind === 'log') {
    FORMAT.log.ext = ext;
    FORMAT.log.label = ext.toLowerCase() === '.log' ? '日志文件' : '保留排版';
  }
};

// another format: a new file; this window stays on the one it has
function convert(path, F) {
  // DOS text art: its characters, as UTF-8 (an .ans without its colours)
  if (ART.includes(app.kind)) return be.exportText(path, app.artText(), true);
  const doc = view.state.doc;
  if (F.ext === '.docx' && cur.source) return be.saveConvertedAs(path, true);
  if (F.ext === '.docx') return be.exportWord(path, docToBlocks(doc), true, cur.dir);
  if (app.kind === 'txt') return be.exportText(path, serializeText(doc), true);
  if (F.ext === '.md' && app.kind === 'doc')
    return be.exportMarkdown(path, serializeMarkdown(doc, null), picturesOf(doc), T('图{n}'), true);
  if (F.ext === '.md') return be.exportText(path, serializeMarkdown(doc, null), true);
  return be.exportText(path, docToPlain(doc), true);
}
// a Word document's pictures, each once, in its order: the Rust side writes them beside the Markdown (报告 图1.png…)
export function picturesOf(doc) {
  const seen = [];
  doc.descendants((n) => {
    const src = n.type.name === 'image' && n.attrs.src;
    if (src && /^(docx|data):/.test(src) && !seen.includes(src)) seen.push(src);
    return true;
  });
  return seen;
}
function writeOwn(path) {
  if (cur.long || cur.readOnly) return be.copyLongAs(path, true);
  if (app.kind === 'doc') return be.saveWordAs(path, docToBlocks(view.state.doc), true);
  return be.saveDocumentAs(path, currentText(), true, cur.pickedEncoding, cur.pickedEol);
}
/** The card's save: the system's save dialog has already asked before replacing anything */
export async function saveTo(path, key, shell) {
  const F = FORMAT[key],
    own = key === formatsFor()[0] && !cur.source && !ART.includes(app.kind);
  const version = editStamp();
  const r = own ? await writeOwn(path) : await convert(path, F);
  // a read-only format saved as Word: the window moves to the new file, which can be edited
  if (r.status === 'exported' && cur.source && F.ext === '.docx') {
    await shell.openPath(path);
    return { status: 'saved', exported: true, path, name: baseName(path) };
  }
  // a copy (the long scroll's bytes, another format): Saved as, the window stays
  if (r.status === 'copied' || r.status === 'exported') return { status: 'copied', path, name: baseName(path) };
  if (r.status === 'failed') return { status: 'failed', kind: r.kind };
  // saved as itself: the window moves there and says Saved; or the encoding's question is asked, and the card stays
  return settleSaveAs(r, version) ? { status: 'saved', path: cur.path, name: cur.name, said: true } : { status: 'ask' };
}
app.formatsFor = formatsFor;
app.saveTo = saveTo;
