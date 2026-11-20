// Turning files into books. A text book is chapters of clean HTML; a page book (comics, scans) is a list of pictures.
// The Rust side has already refused a locked book; the checks here are a second look. A chapter with no title of its own
// gets one from the page, in the window's language: `auto` says which kind ('part' for a long text cut in pieces,
// 'start' for what comes before the first chapter).
import { unzipSync, strFromU8 } from 'fflate';

// Every hexadecimal number here with a capital letter in it is marked prettier-ignore, so Prettier leaves it as it is
// written. Prettier would write it in small letters: the same number, but esbuild picks the minifier's short names by
// how often each letter occurs in the source, and with these numbers in small letters the built page's names come out
// differently. Marked, the build is the same bytes whether or not the code is formatted.

export class Refused extends Error {
  constructor(kind) {
    super(kind);
    this.kind = kind;
  }
}

// ---------- a zip's own sizes are only what it says ----------
// fflate makes each entry's buffer at the size the zip declares, before inflating it. Deflate can't shrink anything
// more than about 1032 to 1, so an entry claiming more than that over what it really takes in the file is a lie; and
// the whole book may unpack to at most 8 times its file (256 MB for any small one, never over 2 GB). Past either, the
// book isn't opened, rather than the window running out of memory.
const DEFLATE_MOST = 1032;
export function unzipBounded(bytes) {
  const limit = Math.min(2 ** 31, Math.max(256 * 2 ** 20, bytes.length * 8));
  let total = 0;
  return unzipSync(bytes, {
    filter: (f) => {
      const honest = f.compression === 0 ? f.originalSize <= f.size : f.originalSize <= f.size * DEFLATE_MOST + 1024;
      total += f.originalSize;
      if (!honest || total > limit) throw new Error('too large');
      return true;
    },
  });
}
const parseXml = (s) => new DOMParser().parseFromString(s, 'application/xml');
// XHTML may close any tag with a slash; the HTML parser would otherwise swallow what follows
const parseHtml = (s) =>
  new DOMParser().parseFromString(
    s.replace(/<(a|div|span|p|title|script|style|h[1-6]|i|b|em|td)(\s[^<>]*)?\/>/gi, '<$1$2></$1>'),
    'text/html'
  );
const esc = (s) => s.replace(/[&<>"]/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;' })[c]);
const natural = new Intl.Collator(undefined, { numeric: true, sensitivity: 'base' }).compare;
const plainLen = (h) => h.replace(/<[^>]+>/g, '').replace(/&nbsp;|\s/g, '').length;

export function decodeText(buf) {
  const b = new Uint8Array(buf);
  if (b[0] === /* prettier-ignore */ 0xFF && b[1] === /* prettier-ignore */ 0xFE)
    return new TextDecoder('utf-16le').decode(b);
  if (b[0] === /* prettier-ignore */ 0xFE && b[1] === /* prettier-ignore */ 0xFF)
    return new TextDecoder('utf-16be').decode(b);
  try {
    return new TextDecoder('utf-8', { fatal: true }).decode(b);
  } catch {
    return new TextDecoder('gb18030').decode(b);
  }
}

// ---------- only plain reading content survives: no scripts, no styles, nothing that reaches out ----------
const KEEP = new Set(
  'p div h1 h2 h3 h4 h5 h6 em i strong b u s sub sup br hr blockquote ul ol li img a table thead tbody tr td th pre code ruby rt rp figure figcaption section article aside span cite small dl dt dd center'.split(
    ' '
  )
);
const DROP = new Set(
  'script style link meta iframe object embed audio video svg math form input button nav head title'.split(' ')
);
function clean(node, ctx) {
  const out = [];
  for (const n of node.childNodes) {
    if (n.nodeType === 3) {
      out.push(esc(n.nodeValue));
      continue;
    }
    if (n.nodeType !== 1) continue;
    const tag = n.localName.toLowerCase();
    if (tag === 'svg') {
      const im = n.querySelector('image'),
        s = im && ctx.image && ctx.image(im);
      if (s) out.push(`<img src="${esc(s)}" alt="">`);
      continue;
    }
    if (DROP.has(tag)) continue;
    const inner = () => clean(n, ctx);
    if (!KEEP.has(tag)) {
      out.push(inner());
      continue;
    }
    if (tag === 'img') {
      const src = ctx.image && ctx.image(n);
      if (src) out.push(`<img src="${esc(src)}" alt="${esc(n.getAttribute('alt') || '')}">`);
      continue;
    }
    if (tag === 'br' || tag === 'hr') {
      out.push(`<${tag}>`);
      continue;
    }
    const id = n.getAttribute('id');
    const attr = id ? ` id="${esc(id)}"` : '';
    if (tag === 'a') {
      const note = ctx.note && ctx.note(n);
      out.push(
        note ? `<a class="noteref" data-note="${esc(note)}" href="#">${inner()}</a>` : `<span${attr}>${inner()}</span>`
      );
      continue;
    }
    const t = tag === 'center' ? 'p' : tag;
    out.push(`<${t}${attr}>${inner()}</${t}>`);
  }
  return out.join('');
}
// every picture is a blob: address, kept with the book being read and let go when that book closes
let urls = new Set();
const imageUrl = (bytes, type) => {
  const u = URL.createObjectURL(new Blob([bytes], type ? { type } : {}));
  urls.add(u);
  return u;
};
/** Call before reading a file: the pictures made from here on are the new book's, and this returns their list. */
export function newImages() {
  urls = new Set();
  return urls;
}
export function releaseImages(list) {
  list.forEach((u) => URL.revokeObjectURL(u));
  list.clear();
}
const MIME = {
  jpg: 'image/jpeg',
  jpeg: 'image/jpeg',
  png: 'image/png',
  gif: 'image/gif',
  webp: 'image/webp',
  svg: 'image/svg+xml',
  bmp: 'image/bmp',
};

// ---------- EPUB ----------
const resolve = (base, rel) => {
  const parts = (base.replace(/[^/]*$/, '') + decodeURI(rel.split('#')[0])).split('/'),
    out = [];
  for (const p of parts) {
    if (p === '..') out.pop();
    else if (p && p !== '.') out.push(p);
  }
  return out.join('/');
};
export async function readEpub(buf) {
  const zip = unzipBounded(new Uint8Array(buf));
  if (zip['META-INF/rights.xml'] || zip['META-INF/license.lcpl']) throw new Refused('drm');
  // obfuscating fonts is allowed; encrypting anything else means the book is locked
  if (zip['META-INF/encryption.xml']) {
    const algos = [...parseXml(strFromU8(zip['META-INF/encryption.xml'])).querySelectorAll('EncryptionMethod')].map(
      (e) => e.getAttribute('Algorithm') || ''
    );
    if (algos.some((a) => !/obfuscation|ns\.adobe\.com\/pdf\/enc#RC/.test(a))) throw new Refused('drm');
  }
  const opfPath = parseXml(strFromU8(zip['META-INF/container.xml']))
    .querySelector('rootfile')
    .getAttribute('full-path');
  const opf = parseXml(strFromU8(zip[opfPath]));
  const items = new Map([...opf.querySelectorAll('manifest > item')].map((i) => [i.getAttribute('id'), i]));
  const text = (sel) => (opf.getElementsByTagName(sel)[0] || {}).textContent;
  const spine = [...opf.querySelectorAll('spine > itemref')].map((r) =>
    resolve(opfPath, items.get(r.getAttribute('idref')).getAttribute('href'))
  );
  // the table of contents: EPUB 3's nav, or the older NCX
  const titles = new Map(),
    toc = [];
  const addToc = (href, label, base, depth) => {
    const p = resolve(base, href);
    if (!titles.has(p)) titles.set(p, label);
    toc.push({ path: p, title: label, depth });
  };
  const navItem = [...items.values()].find((i) => /\bnav\b/.test(i.getAttribute('properties') || ''));
  const ncxItem = [...items.values()].find((i) => i.getAttribute('media-type') === 'application/x-dtbncx+xml');
  if (navItem) {
    const p = resolve(opfPath, navItem.getAttribute('href')),
      doc = parseHtml(strFromU8(zip[p]));
    const nav =
      [...doc.querySelectorAll('nav')].find((n) =>
        /toc/.test(n.getAttribute('epub:type') || n.getAttribute('type') || '')
      ) || doc.querySelector('nav');
    if (nav)
      for (const a of nav.querySelectorAll('a[href]')) {
        let d = 0;
        for (let e = a.parentElement; e && e !== nav; e = e.parentElement) if (e.localName === 'ol') d++;
        addToc(a.getAttribute('href'), a.textContent.trim(), p, d - 1);
      }
  } else if (ncxItem) {
    const p = resolve(opfPath, ncxItem.getAttribute('href')),
      doc = parseXml(strFromU8(zip[p]));
    for (const np of doc.querySelectorAll('navPoint')) {
      let d = 0;
      for (let e = np.parentElement; e; e = e.parentElement) if (e.localName === 'navPoint') d++;
      addToc(np.querySelector('content').getAttribute('src'), np.querySelector('navLabel').textContent.trim(), p, d);
    }
  }
  const docs = spine.filter((p) => zip[p]).map((p) => [p, parseHtml(strFromU8(zip[p]))]);
  // footnotes: read every target first so a click can show it where you are
  const notes = {},
    noteTargets = new Map();
  for (const [p, doc] of docs)
    for (const a of doc.querySelectorAll('a[href]')) {
      const href = a.getAttribute('href');
      if (/^[a-z]+:/i.test(href) || !href.includes('#')) continue;
      const isNote =
        /noteref/.test(a.getAttribute('epub:type') || '')
        || a.closest('sup')
        || /^\[?[\d*†]+\]?$/.test(a.textContent.trim());
      if (isNote) noteTargets.set(a, [href.startsWith('#') ? p : resolve(p, href), href.split('#')[1]]);
    }
  const byPath = new Map(docs);
  const removeLater = new Set();
  for (const [a, [path, id]] of noteTargets) {
    const doc = byPath.get(path),
      t = doc && doc.getElementById(id);
    if (!t) continue;
    const box = t.closest('aside, li, p, div') || t;
    const key = path + '#' + id;
    notes[key] = clean(box, { image: () => null });
    a.setAttribute('data-note-key', key);
    if (box.localName === 'aside') removeLater.add(box);
  }
  removeLater.forEach((b) => b.remove());
  const chapters = docs
    .map(([path, doc]) => {
      const image = (n) => {
        const raw = n.getAttribute('src') || n.getAttribute('xlink:href') || n.getAttribute('href');
        if (!raw || /^[a-z]+:/i.test(raw)) return null;
        const p = resolve(path, raw),
          b = zip[p];
        return b ? imageUrl(b, MIME[p.split('.').pop().toLowerCase()]) : null;
      };
      const html = clean(doc.body, { image, note: (a) => a.getAttribute('data-note-key') });
      const head = doc.querySelector('h1, h2, h3');
      return { path, title: titles.get(path) || (head && head.textContent.trim()) || '', html, len: plainLen(html) };
    })
    .filter((c) => c.len > 0 || /<img/.test(c.html));
  const index = new Map(chapters.map((c, i) => [c.path, i]));
  const list = toc
    .filter((t) => index.has(t.path))
    .map((t) => ({ title: t.title, ch: index.get(t.path), depth: t.depth || 0 }));
  return {
    type: 'text',
    title: text('dc:title') || '',
    author: text('dc:creator') || '',
    chapters,
    toc: list.length ? list : chapters.map((c, i) => ({ title: c.title, ch: i, depth: 0 })),
    notes,
  };
}

// ---------- plain text: chapters are found, not assumed ----------
const CHAPTER =
  /^\s*(第\s*[0-9一二三四五六七八九十百千万零〇两]+\s*[章节回卷集部篇].{0,40}|序[章言]?|楔子|引子|前言|后记|尾声|番外.{0,30}|chapter\s+[\divxlc]+\b.{0,50})\s*$/i;
export function readTxt(text, name) {
  const lines = text.split(/\r\n|\r|\n/);
  const chapters = [];
  let cur = { title: '', out: [] };
  const push = () => {
    if (cur.title || cur.out.length) chapters.push(cur);
  };
  for (const raw of lines) {
    const line = raw.replace(/^[\s　]+|\s+$/g, '');
    if (!line) continue;
    if (CHAPTER.test(line)) {
      push();
      cur = { title: line, out: [] };
      continue;
    }
    cur.out.push(line);
  }
  push();
  // a title line or two above the first chapter is not a chapter of its own
  const base = name.replace(/.[^.]+$/, '');
  if (chapters.length > 1 && !chapters[0].title && chapters[0].out.join('').length < 80) {
    const [front] = chapters.splice(0, 1);
    chapters[0].front = front.out.filter((l) => l !== base);
  }
  // very long text without chapters is cut so a page never has to hold a whole book
  const made = [];
  for (const c of chapters) {
    for (let i = 0, part = 0; i < c.out.length || !part; i += 400, part++) {
      const body = c.out
        .slice(i, i + 400)
        .map((l) => `<p>${esc(l)}</p>`)
        .join('');
      const front = !part && c.front ? c.front.map((l) => `<p>${esc(l)}</p>`).join('') : '';
      made.push({
        title: c.title,
        auto: c.title ? null : chapters.length === 1 ? 'part' : 'start',
        n: part + 1,
        html: front + (c.title && !part ? `<h2>${esc(c.title)}</h2>` : '') + body,
      });
    }
  }
  const out = made.map((c) => ({ ...c, len: plainLen(c.html) }));
  return {
    type: 'text',
    title: name.replace(/\.[^.]+$/, ''),
    author: '',
    chapters: out,
    toc: out.map((c, i) => ({ title: c.title, ch: i, depth: 0 })),
    notes: {},
  };
}

// ---------- FB2: one XML file, or the same inside a zip ----------
export function readFb2(buf) {
  let bytes = new Uint8Array(buf);
  if (bytes[0] === 0x50 && bytes[1] === /* prettier-ignore */ 0x4B) {
    const z = unzipBounded(bytes);
    const f = Object.keys(z).find((k) => /\.fb2$/i.test(k));
    if (!f) throw new Error('no fb2');
    bytes = z[f];
  }
  const head = new TextDecoder('latin1').decode(bytes.subarray(0, 200));
  const label = (head.match(/encoding=["']([^"']+)/i) || [])[1] || 'utf-8';
  const doc = parseXml(new TextDecoder(label).decode(bytes));
  if (doc.querySelector('parsererror')) throw new Error('bad xml');
  const bin = new Map([...doc.getElementsByTagName('binary')].map((b) => [b.getAttribute('id'), b]));
  const image = (n) => {
    const id = (
        n.getAttribute('l:href')
        || n.getAttributeNS('http://www.w3.org/1999/xlink', 'href')
        || n.getAttribute('href')
        || ''
      ).replace(/^#/, ''),
      b = bin.get(id);
    if (!b) return null;
    return imageUrl(
      Uint8Array.from(atob(b.textContent.replace(/\s/g, '')), (c) => c.charCodeAt(0)),
      b.getAttribute('content-type')
    );
  };
  const notes = {};
  const conv = (el) =>
    [...el.childNodes]
      .map((n) => {
        if (n.nodeType === 3) return esc(n.nodeValue);
        if (n.nodeType !== 1) return '';
        const t = n.localName,
          kids = () => conv(n);
        switch (t) {
          case 'p':
            return `<p>${kids()}</p>`;
          case 'emphasis':
            return `<em>${kids()}</em>`;
          case 'strong':
            return `<strong>${kids()}</strong>`;
          case 'sup':
            return `<sup>${kids()}</sup>`;
          case 'sub':
            return `<sub>${kids()}</sub>`;
          case 'strikethrough':
            return `<s>${kids()}</s>`;
          case 'code':
            return `<code>${kids()}</code>`;
          case 'empty-line':
            return '<p class="gap">&nbsp;</p>';
          case 'subtitle':
            return `<h3>${kids()}</h3>`;
          case 'epigraph':
          case 'cite':
            return `<blockquote>${kids()}</blockquote>`;
          case 'poem':
          case 'stanza':
            return `<blockquote class="verse">${kids()}</blockquote>`;
          case 'v':
            return `<p>${kids()}</p>`;
          case 'text-author':
            return `<p class="by">${kids()}</p>`;
          case 'title':
            return `<h2>${[...n.children].map((p) => p.textContent).join(' ')}</h2>`;
          case 'image': {
            const s = image(n);
            return s ? `<img src="${esc(s)}" alt="">` : '';
          }
          case 'a': {
            const id = (
              n.getAttribute('l:href')
              || n.getAttributeNS('http://www.w3.org/1999/xlink', 'href')
              || ''
            ).replace(/^#/, '');
            return notes[id] != null
              ? `<a class="noteref" data-note="${esc(id)}" href="#">${kids()}</a>`
              : `<span>${kids()}</span>`;
          }
          default:
            return kids();
        }
      })
      .join('');
  const bodies = [...doc.getElementsByTagName('body')];
  for (const b of bodies.filter((x) => /notes|comments/.test(x.getAttribute('name') || '')))
    for (const s of b.getElementsByTagName('section')) notes[s.getAttribute('id')] = conv(s);
  const main = bodies.find((x) => !/notes|comments/.test(x.getAttribute('name') || '')) || bodies[0];
  const chapters = [];
  const walk = (node) => {
    const subs = [...node.children].filter((c) => c.localName === 'section');
    const own = node.cloneNode(true);
    [...own.children].forEach((c) => {
      if (c.localName === 'section') c.remove();
    });
    const title = [...node.children].find((c) => c.localName === 'title');
    const html = conv(own);
    if (plainLen(html) > 0 || /<img/.test(html))
      chapters.push({
        title: title
          ? [...title.children]
              .map((p) => p.textContent)
              .join(' ')
              .trim()
              .replace(/\s+/g, ' ')
          : '',
        html,
        len: plainLen(html),
      });
    subs.forEach(walk);
  };
  walk(main);
  const info = doc.getElementsByTagName('title-info')[0];
  const person = info && info.getElementsByTagName('author')[0];
  return {
    type: 'text',
    title:
      info && info.getElementsByTagName('book-title')[0] ? info.getElementsByTagName('book-title')[0].textContent : '',
    author: person ? [...person.children].map((c) => c.textContent).join(' ') : '',
    chapters,
    toc: chapters.map((c, i) => ({ title: c.title, ch: i, depth: 0 })),
    notes,
  };
}

// ---------- MOBI: PalmDB records; the lock is read from a flag and never opened ----------
class Huff {
  constructor(records) {
    const d = new DataView(records[0].buffer, records[0].byteOffset, records[0].byteLength);
    const off1 = d.getUint32(8),
      off2 = d.getUint32(12);
    this.table = [];
    this.min = new Array(33).fill(0n);
    this.max = new Array(33).fill(0n);
    for (let i = 0; i < 256; i++) {
      const v = d.getUint32(off1 + i * 4),
        len = v & /* prettier-ignore */ 0x1F;
      this.table.push([len, !!(v & 0x80), ((BigInt(v >>> 8) + 1n) << BigInt(32 - len)) - 1n]);
    }
    for (let len = 1; len <= 32; len++) {
      const a = d.getUint32(off2 + (len - 1) * 8),
        b = d.getUint32(off2 + (len - 1) * 8 + 4);
      this.min[len] = BigInt(a) << BigInt(32 - len);
      this.max[len] = ((BigInt(b) + 1n) << BigInt(32 - len)) - 1n;
    }
    this.dict = [];
    for (const r of records.slice(1)) {
      const v = new DataView(r.buffer, r.byteOffset, r.byteLength),
        phrases = v.getUint32(8),
        bits = v.getUint32(12),
        n = Math.min(1 << bits, phrases - this.dict.length);
      for (let i = 0; i < n; i++) {
        const o = v.getUint16(16 + i * 2),
          blen = v.getUint16(16 + o);
        this.dict.push([r.subarray(16 + o + 2, 16 + o + 2 + (blen & /* prettier-ignore */ 0x7FFF)), !!(blen & 0x8000)]);
      }
    }
  }
  unpack(data) {
    const padded = new Uint8Array(data.length + 8);
    padded.set(data);
    const v = new DataView(padded.buffer),
      out = [];
    let bitsLeft = data.length * 8,
      pos = 0,
      x = v.getBigUint64(0),
      n = 32;
    for (;;) {
      if (n <= 0) {
        pos += 4;
        x = v.getBigUint64(pos);
        n += 32;
      }
      const code = (x >> BigInt(n)) & /* prettier-ignore */ 0xFFFFFFFFn;
      let [len, term, max] = this.table[Number(code >> 24n)];
      if (!term) {
        while (code < this.min[len]) len++;
        max = this.max[len];
      }
      n -= len;
      bitsLeft -= len;
      if (bitsLeft < 0) break;
      const r = Number((max - code) >> BigInt(32 - len));
      let [piece, done] = this.dict[r];
      if (!done) {
        this.dict[r] = null;
        piece = this.unpack(piece);
        this.dict[r] = [piece, true];
      }
      out.push(piece);
    }
    return concat(out);
  }
}
const concat = (parts) => {
  const n = parts.reduce((a, p) => a + p.length, 0),
    o = new Uint8Array(n);
  let at = 0;
  for (const p of parts) {
    o.set(p, at);
    at += p.length;
  }
  return o;
};
function palmdoc(src) {
  const out = new Uint8Array(src.length * 8 + 16);
  let o = 0;
  for (let i = 0; i < src.length;) {
    const c = src[i++];
    if (c >= 1 && c <= 8) {
      for (let k = 0; k < c && i < src.length; k++) out[o++] = src[i++];
    } else if (c < 0x80) out[o++] = c;
    else if (c >= /* prettier-ignore */ 0xC0) {
      out[o++] = 0x20;
      out[o++] = c ^ 0x80;
    } else {
      const v = ((c << 8) | src[i++]) & /* prettier-ignore */ 0x3FFF,
        d = v >> 3,
        n = (v & 7) + 3;
      for (let k = 0; k < n; k++, o++) out[o] = out[o - d];
    }
  }
  return out.subarray(0, o);
}
function trailing(rec, flags) {
  let size = rec.length,
    cut = 0;
  const entry = (end) => {
    let bit = 0,
      res = 0,
      s = end;
    while (s > 0) {
      const v = rec[s - 1];
      res |= (v & /* prettier-ignore */ 0x7F) << bit;
      bit += 7;
      s--;
      if (v & 0x80 || bit >= 28) break;
    }
    return res;
  };
  for (let f = flags >> 1; f; f >>= 1) if (f & 1) cut += entry(size - cut);
  if (flags & 1) cut += (rec[size - cut - 1] & 3) + 1;
  return rec.subarray(0, Math.max(0, size - cut));
}
export function readMobi(buf) {
  const b = new Uint8Array(buf),
    v = new DataView(b.buffer, b.byteOffset, b.byteLength);
  if (
    new TextDecoder().decode(b.subarray(60, 68)) !== 'BOOKMOBI'
    && new TextDecoder().decode(b.subarray(60, 68)) !== 'TEXtREAd'
  )
    throw new Error('not mobi');
  const count = v.getUint16(76),
    starts = [];
  for (let i = 0; i < count; i++) starts.push(v.getUint32(78 + i * 8));
  const rec = (i) => b.subarray(starts[i], i + 1 < count ? starts[i + 1] : b.length);
  const r0 = rec(0),
    h = new DataView(r0.buffer, r0.byteOffset, r0.byteLength);
  const comp = h.getUint16(0),
    textRecs = h.getUint16(8);
  if (h.getUint16(12) !== 0) throw new Refused('drm');
  const hasMobi = new TextDecoder().decode(r0.subarray(16, 20)) === 'MOBI';
  const hdrLen = hasMobi ? h.getUint32(20) : 0;
  const utf8 = hasMobi && h.getUint32(28) === 65001;
  const extra = hasMobi && hdrLen >= /* prettier-ignore */ 0xE4 ? h.getUint16(/* prettier-ignore */ 0xF2) : 0;
  let huff = null;
  if (comp === 17480) {
    const first = h.getUint32(0x70),
      n = h.getUint32(0x74);
    huff = new Huff(Array.from({ length: n }, (_, i) => rec(first + i)));
  } else if (comp !== 1 && comp !== 2) throw new Error('compression');
  const parts = [];
  for (let i = 1; i <= textRecs; i++) {
    const raw = trailing(rec(i), extra);
    parts.push(huff ? huff.unpack(raw) : comp === 2 ? palmdoc(raw) : raw);
  }
  const dec = new TextDecoder(utf8 ? 'utf-8' : 'windows-1252');
  const html = dec.decode(concat(parts));
  let title = '',
    author = '';
  if (hasMobi) {
    const o = h.getUint32(0x54),
      l = h.getUint32(0x58);
    title = dec.decode(r0.subarray(o, o + l));
    if (h.getUint32(0x80) & 0x40) {
      const e = 16 + hdrLen,
        n = h.getUint32(e + 8);
      let p = e + 12;
      for (let i = 0; i < n; i++) {
        const t = h.getUint32(p),
          s = h.getUint32(p + 4);
        if (t === 100 && !author) author = dec.decode(r0.subarray(p + 8, p + s));
        p += s;
      }
    }
  } else title = new TextDecoder('latin1').decode(b.subarray(0, 32)).replace(/\0.*$/, '');
  const firstImg = hasMobi ? h.getUint32(/* prettier-ignore */ 0x6C) : /* prettier-ignore */ 0xFFFFFFFF;
  const image = (n) => {
    const idx = parseInt(n.getAttribute('recindex') || '', 10);
    if (!idx || firstImg === /* prettier-ignore */ 0xFFFFFFFF || firstImg + idx - 1 >= count) return null;
    return imageUrl(rec(firstImg + idx - 1));
  };
  const pieces = html.split(/<mbp:pagebreak\s*\/?>(?:<\/mbp:pagebreak>)?/i);
  const chapters = [];
  for (const piece of pieces) {
    const doc = parseHtml(piece),
      body = clean(doc.body, { image });
    if (!plainLen(body) && !/<img/.test(body)) continue;
    const head = doc.querySelector('h1, h2, h3');
    chapters.push({ title: head ? head.textContent.trim() : '', html: body, len: plainLen(body) });
  }
  return {
    type: 'text',
    title,
    author,
    chapters,
    toc: chapters.map((c, i) => ({ title: c.title, ch: i, depth: 0 })),
    notes: {},
  };
}

// ---------- comics: a zip of pictures, in the order a person would count them ----------
export function readCbz(buf, name) {
  const z = unzipBounded(new Uint8Array(buf));
  const names = Object.keys(z)
    .filter((k) => /\.(jpe?g|png|webp|gif|bmp)$/i.test(k) && !/(^|\/)(__MACOSX|\.)/.test(k))
    .sort(natural);
  if (!names.length) throw new Error('no pictures');
  return {
    type: 'pages',
    kind: 'cbz',
    title: name.replace(/\.[^.]+$/, ''),
    author: '',
    pages: names.map((k) => ({ url: imageUrl(z[k], MIME[k.split('.').pop().toLowerCase()]) })),
  };
}

// ---------- scanned books: decoded a page at a time ----------
// the decoder is a large script of its own (vendor/djvujs), fetched from the program the first time a scan opens
let djvuLoading = null;
const djvuReady = () =>
  window.DjVu
    ? Promise.resolve()
    : (djvuLoading =
        djvuLoading
        || new Promise((ok, no) => {
          const s = document.createElement('script');
          s.src = 'djvu.js';
          s.onload = ok;
          s.onerror = () => {
            djvuLoading = null;
            no(new Error('no decoder'));
          };
          document.head.appendChild(s);
        }));
export async function readDjvu(buf, name) {
  await djvuReady();
  const doc = new window.DjVu.Document(buf.slice(0));
  const n = doc.pages.length;
  if (!n) throw new Error('no pages');
  return {
    type: 'pages',
    kind: 'djvu',
    title: name.replace(/\.[^.]+$/, ''),
    author: '',
    pages: Array.from({ length: n }, (_, i) => ({
      draw: async () => {
        const page = await doc.getPage(i + 1),
          data = page.getImageData(),
          c = document.createElement('canvas');
        c.width = data.width;
        c.height = data.height;
        c.getContext('2d').putImageData(data, 0, 0);
        if (page.reset) page.reset();
        return c;
      },
      // the scan's hidden text layer, when it has one: its words with their boxes (y from the bottom), for find
      words: async () => {
        const page = await doc.getPage(i + 1),
          zones = (page.getNormalizedTextZones() || []).map((z) => ({
            x: z.x,
            y: z.y,
            width: z.width,
            height: z.height,
            text: z.text,
          }));
        const size = { w: page.getWidth(), h: page.getHeight() };
        if (page.reset) page.reset();
        return { zones, ...size };
      },
    })),
  };
}

export const noteHtml = (t) => esc(t);
