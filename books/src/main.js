// DotAny Books: the family's window around a book. The reading itself — pages that cover and draw away, scrolling from
// chapter to chapter, the contents on the left with its sliding pill, the layout chips, notes where you are, and the top
// that slides away while you read — is done here. Everything around it, from the top bar to settings and save as, is
// shared/web.
import { createShell } from '@dotany/web/shell.js';
import { el, root, reduce, store, sound, icon } from '@dotany/web/shared.js';
import { chips } from '@dotany/web/parts.js';
import { T, longWords } from '@dotany/web/i18n.js';
import { inkfall } from '@dotany/web/inkfall-input.js';
import { modKey, macEdge, used } from '@dotany/web/keys.js';
import { mac, wheelSource, swipeTurns } from '@dotany/web/gestures.js';
import { LUCIDE } from './icons.js';
import * as own from './backend.js';
import { canOpen as familyCanOpen, on as hear } from '@dotany/web/backend.js';
import {
  Refused,
  decodeText,
  readEpub,
  readTxt,
  readFb2,
  readMobi,
  readCbz,
  readDjvu,
  newImages,
  releaseImages,
} from './readers.js';
import { foldQuery, foldChapter, foldWords, matches, rangeOf, boxesOf } from './find.js';

// the tool's own words in other languages: src/lang/<code>.json, added to the family's (shared/web/lang) by createShell
import en from './lang/en.json';
// English is always here; every other language is loaded when chosen (shared/web/i18n.js LANGS lists them)
const WORDS = {
  en,
  'zh-Hant': () => import('./lang/zh-Hant.json'),
  ja: () => import('./lang/ja.json'),
  ko: () => import('./lang/ko.json'),
  'es-419': () => import('./lang/es-419.json'),
  'pt-BR': () => import('./lang/pt-BR.json'),
  fr: () => import('./lang/fr.json'),
  de: () => import('./lang/de.json'),
};
// the save card keeps a book as it is: one format, the book's own. CBT, CB7 and a zipped FB2 (.fbz, .fb2.zip) too: a
// copy of the file, packed as it was
const FORMATS = {
  epub: { ext: '.epub', label: 'EPUB' },
  fb2: { ext: '.fb2', label: 'FB2' },
  cbz: { ext: '.cbz', label: 'CBZ' },
  djvu: { ext: '.djvu', label: 'DJVU' },
  mobi: { ext: '.mobi', label: 'MOBI' },
  txt: { ext: '.txt', label: 'TXT' },
  cbt: { ext: '.cbt', label: 'CBT' },
  cb7: { ext: '.cb7', label: 'CB7' },
  fbz: { ext: '.fbz', label: 'FBZ' },
  fb2zip: { ext: '.fb2.zip', label: 'FB2' },
};

let book = null,
  pending = null,
  chAt = 0,
  pageAt = 0,
  pages = 1,
  stepPx = 0;

const shell = createShell({
  name: 'DotAny Books',
  icons: LUCIDE,
  words: WORDS,
  notices: () => import('./notices.generated.js'),
  dropTitle: '拖拽电子书至此',
  fileKind: '电子书',
  soundNote: '触发按钮操作时播放提示音，沉浸翻页时保持静默。',
  // Books is under the GPL (books/LICENSE), because DjVu.js inside it is
  licence: 'GPL-3.0-or-later',
  licences: [
    ['Noto Serif SC', '书籍排版字体', 'SIL OFL 1.1'],
    ['fflate', 'ZIP', 'MIT'],
    ['sevenz-rust2', 'CB7', 'Apache-2.0'],
    ['DjVu.js', 'DJVU', 'GPL-2.0-or-later'],
  ],
  formats: FORMATS,
  formatOf: (doc) => doc.kind,
  oneFormat: true,
  cantOpen: { drm: '该电子书受数字版权管理（DRM）保护，无法在此打开。' },
  // a book.fb2.zip is a book, though .zip on its own is DotAny Archive's: the Rust side looks at it as it opens
  canOpen: (path) => (/\.fb2\.zip$/i.test(path) ? 'ok' : familyCanOpen(path)),
  open: openPath,
  show(r) {
    if (pending && pending.path === r.path) showBook(pending.book, r);
  },
  clear: closeBook,
  // a book is only ever read: nothing is unsaved, so closing never asks
  dirty: () => false,
  estimate: () => own.estimateSave(),
  save: (path) => own.saveFile(path),
  keys,
});

// ---------- the window: words alone, with the family's glass only when asked for ----------
const reader = el('main', 'reader'),
  clip = el('div', 'clip'),
  page = el('article', 'page'),
  plate = el('div', 'plate');
clip.appendChild(page);
reader.append(clip, plate);
const foot = el('div', 'foot'),
  footL = el('span', 'foot-l'),
  footR = el('span', 'foot-r');
foot.append(footL, footR);
const notePop = el('div', 'note');
shell.stage.append(reader, foot);
// the contents sit on the left, the layout on the right
const tocPanel = shell.sidePanel('toc', 'toc', '章节目录'),
  typePanel = shell.sidePanel('type', 'type', '阅读排版');
tocPanel.classList.add('left');
typePanel.classList.add('right');
const tocBtn = document.getElementById('tocBtn'),
  typeBtn = document.getElementById('typeBtn');
// find, as DotAny PDF and Text have it: the whole book, not only the chapter on screen
const findBtn = shell.headButton('findBtn', 'search', '查找', '查找 · {Mod+F}');
findBtn.setAttribute('aria-pressed', 'false');
shell.place(notePop);

// ---------- what you're reading, and how you like it ----------
const readJson = (k, d) => {
  try {
    return JSON.parse(store.get(k)) || d;
  } catch {
    return d;
  }
};
const clamp = (v, a, b) => Math.min(b, Math.max(a, v));
const prefs = {
  font: 'serif',
  size: 19,
  lh: 1.85,
  width: 34,
  mode: 'page',
  fit: 'page',
  ...readJson('dotany-books', {}),
};
// comfortable settings are steps you can land on; the middle one is where a book starts
const STEPS = { size: [15, 17, 19, 22, 25], lh: [1.6, 1.85, 2.1], width: [28, 34, 40] };
for (const k in STEPS) prefs[k] = STEPS[k].reduce((b, x) => (Math.abs(x - prefs[k]) < Math.abs(b - prefs[k]) ? x : b));
const savePrefs = () => store.set('dotany-books', JSON.stringify(prefs));
// where each book was left, by its name and size, so a moved copy still opens where you stopped; the newest 300 are kept
let progress = readJson('dotany-books-at', {});
const total = () => (book.type === 'text' ? book.chapters.reduce((a, c) => a + c.len, 0) || 1 : book.pages.length);
const chapterTitle = (i) => {
  const c = book.chapters[i];
  return (
    c.title
    || (c.auto === 'start'
      ? T('扉页与开篇')
      : T(c.auto === 'part' ? '第 {n} 部分' : '第 {n} 节').replace('{n}', c.auto === 'part' ? c.n : i + 1))
  );
};
function percent() {
  if (book.type === 'pages') return (chAt + 1) / book.pages.length;
  let before = 0;
  for (let i = 0; i < chAt; i++) before += book.chapters[i].len;
  if (prefs.mode === 'page' && chAt === book.chapters.length - 1 && pageAt === pages - 1) return 1;
  const inner = prefs.mode === 'page' ? pageAt / pages : scrollShare();
  return clamp((before + book.chapters[chAt].len * inner) / total(), 0, 1);
}
function syncFoot() {
  if (!book) return;
  if (book.type === 'pages') {
    footL.textContent = book.title;
    footR.textContent = `${chAt + 1} / ${book.pages.length}`;
  } else {
    const c = book.chapters[chAt];
    footL.textContent = c.title || (c.auto ? chapterTitle(chAt) : book.title || chapterTitle(chAt));
  }
  markToc();
  syncNav();
}
let rememberTimer = 0;
function remember() {
  if (!book) return;
  const anchor =
    book.type === 'text' && prefs.mode === 'page' && pages > 1
      ? pageAt / (pages - 1)
      : book.type === 'text'
        ? scrollShare()
        : 0;
  delete progress[book.key];
  progress[book.key] = { ch: chAt, at: anchor, pct: Math.round(percent() * 100) };
  const keys = Object.keys(progress);
  if (keys.length > 300) keys.slice(0, keys.length - 300).forEach((k) => delete progress[k]);
  // the disk hears about it once the reading settles
  clearTimeout(rememberTimer);
  rememberTimer = setTimeout(() => store.set('dotany-books-at', JSON.stringify(progress)), 250);
}

// ---------- laying out a chapter: columns of a width a person can read ----------
function applyType() {
  root.style.setProperty('--read-size', prefs.size + 'px');
  root.style.setProperty('--read-lh', prefs.lh);
  root.classList.toggle('sans', prefs.font === 'sans');
  root.classList.toggle('scrolling', prefs.mode === 'scroll');
  root.classList.toggle('fit-width', prefs.fit === 'width');
}
const GAP = 72;
function measure() {
  if (!book || book.type !== 'text') return;
  const W = reader.clientWidth,
    H = reader.clientHeight,
    side = Math.max(24, Math.min(96, W * 0.07));
  const em = book.latin ? prefs.size * 0.5 : prefs.size;
  const col = Math.floor(Math.min(em * (book.latin ? prefs.width * 2 : prefs.width), W - side * 2));
  const two = prefs.mode === 'page' && W >= col * 2 + GAP + side * 2 + 40;
  const width = two ? col * 2 + GAP : col;
  const top = 76,
    bottom = 68,
    height = Math.max(200, H - top - bottom);
  root.style.setProperty('--col', col + 'px');
  root.style.setProperty('--page-h', height + 'px');
  root.style.setProperty('--book-w', width + 'px');
  clip.style.width = width + 'px';
  if (prefs.mode === 'page') {
    clip.style.height = height + 'px';
    clip.style.marginTop = top + 'px';
    page.style.width = width + 'px';
    page.style.height = height + 'px';
    stepPx = width + GAP;
    pages = Math.max(1, Math.ceil((page.scrollWidth + GAP - 1) / stepPx));
    // a chapter that fits one column sits in the middle, not on the left of an empty spread
    if (two && pages === 1) {
      const r = document.createRange();
      r.selectNodeContents(page);
      if (r.getBoundingClientRect().right - page.getBoundingClientRect().left <= col + 1) {
        clip.style.width = col + 'px';
        page.style.width = col + 'px';
        root.style.setProperty('--book-w', col + 'px');
        stepPx = col + GAP;
      }
    }
  } else {
    clip.style.height = '';
    clip.style.marginTop = '';
    page.style.width = '';
    page.style.height = '';
    pages = 1;
  }
}
function place() {
  page.style.transform = prefs.mode === 'page' ? `translateX(${-pageAt * stepPx}px)` : '';
}
// pages are a stack of sheets: going forward lays the next one on top, going back draws the top one away.
// the screen never goes blank
let turning = null;
function endTurn() {
  if (turning) turning();
}
function sweep(d, target, ghost, change) {
  endTurn();
  if (reduce) {
    change();
    return;
  }
  const G = ghost();
  change();
  const sheet = d > 0 ? target : G,
    win = reader.getBoundingClientRect();
  sheet.style.setProperty('--s', 1);
  sheet.style.setProperty('--ext', sheet.getBoundingClientRect().left - win.left + 'px');
  sheet.classList.add('turn-cover');
  const away = win.width + 'px 0',
    a = sheet.animate(
      d > 0 ? [{ translate: away }, { translate: '0 0' }] : [{ translate: '0 0' }, { translate: away }],
      { duration: 420, easing: 'cubic-bezier(.2,.7,.2,1)', fill: 'both' }
    );
  turning = () => {
    turning = null;
    a.cancel();
    sheet.classList.remove('turn-cover');
    sheet.style.removeProperty('--s');
    sheet.style.removeProperty('--ext');
    G.remove();
    if (G.done) G.done();
  };
  a.onfinish = () => {
    if (turning) turning();
  };
}
// a still copy of the page as it stands, in exactly the same place
function textGhost() {
  const g = clip.cloneNode(true);
  g.classList.add('turn-ghost');
  g.setAttribute('aria-hidden', 'true');
  g.style.left = clip.offsetLeft + 'px';
  g.style.top = clip.offsetTop + 'px';
  g.style.margin = '0';
  reader.appendChild(g);
  return g;
}
function plateGhost() {
  const old = plate.firstElementChild,
    g = el('div', 'turn-ghost plate-ghost');
  g.style.top = -reader.scrollTop + 'px';
  old.style.left = old.offsetLeft + 'px';
  old.style.top = old.offsetTop + 'px';
  old.style.position = 'absolute';
  g.appendChild(old);
  reader.appendChild(g);
  g.done = () => {
    old.remove();
    old.style.position = old.style.left = old.style.top = '';
  };
  return g;
}
function glide(change, d = 1) {
  if (prefs.mode === 'page') sweep(d, clip, textGhost, change);
  else change();
}
const chapterHtml = (i) => `<section class="ch" data-ch="${i}">${book.chapters[i].html}</section>`;
function showChapter(i, where = 0, fade = true) {
  chAt = clamp(i, 0, book.chapters.length - 1);
  page.innerHTML = chapterHtml(chAt);
  // a picture changes how many pages the chapter takes once it has loaded
  page.querySelectorAll('img').forEach((im) => {
    if (!im.complete) im.addEventListener('load', relayout, { once: true });
  });
  measure();
  if (prefs.mode === 'page') {
    pageAt = where === 'end' ? pages - 1 : clamp(Math.round(where * (pages - 1)), 0, pages - 1);
    place();
  } else {
    place();
    fillScroll();
    reader.scrollTop = where === 'end' ? reader.scrollHeight : where * (page.offsetHeight - reader.clientHeight);
  }
  if (fade && !reduce)
    page.animate([{ opacity: 0 }, { opacity: 1 }], { duration: 240, easing: 'cubic-bezier(.2,.7,.2,1)' });
  syncFoot();
  remember();
}
function turn(d) {
  if (!book) return;
  closeNote();
  endTurn();
  if (book.type === 'pages') {
    if (chAt + d >= 0 && chAt + d < book.pages.length) showPlate(chAt + d);
    return;
  }
  if (prefs.mode === 'scroll') {
    reader.scrollBy({ top: d * (reader.clientHeight - 96), behavior: reduce ? 'auto' : 'smooth' });
    return;
  }
  measure();
  const n = pageAt + d;
  if (n < 0) {
    if (chAt > 0) glide(() => showChapter(chAt - 1, 'end', false), -1);
    return;
  }
  if (n >= pages) {
    if (chAt < book.chapters.length - 1) glide(() => showChapter(chAt + 1, 0, false));
    return;
  }
  glide(() => {
    pageAt = n;
    place();
    syncFoot();
    remember();
  }, d);
}
// scrolling flows from chapter to chapter; a short chapter (a title page) brings the next ones along, or there would be
// nothing to scroll and no way on
function fillScroll() {
  for (
    let last = page.lastElementChild;
    last && +last.dataset.ch < book.chapters.length - 1 && page.offsetHeight < reader.clientHeight * 2;
    last = page.lastElementChild
  )
    page.insertAdjacentHTML('beforeend', chapterHtml(+last.dataset.ch + 1));
}
function scrollShare() {
  const s = page.querySelector(`[data-ch="${chAt}"]`);
  if (!s) return 0;
  const r = s.getBoundingClientRect(),
    box = reader.getBoundingClientRect();
  return clamp((box.top + 80 - r.top) / Math.max(1, r.height - box.height + 160), 0, 1);
}
reader.addEventListener(
  'scroll',
  () => {
    if (!book || book.type !== 'text' || prefs.mode !== 'scroll') return;
    const last = page.lastElementChild;
    if (
      last
      && +last.dataset.ch < book.chapters.length - 1
      && reader.scrollTop + reader.clientHeight * 2 > reader.scrollHeight
    )
      page.insertAdjacentHTML('beforeend', chapterHtml(+last.dataset.ch + 1));
    const first = page.firstElementChild;
    if (first && +first.dataset.ch > 0 && reader.scrollTop < 40) {
      const h = reader.scrollHeight;
      page.insertAdjacentHTML('afterbegin', chapterHtml(+first.dataset.ch - 1));
      reader.scrollTop += reader.scrollHeight - h;
    }
    const mid = reader.getBoundingClientRect().top + 90;
    for (const sec of page.children) {
      const r = sec.getBoundingClientRect();
      if (r.top <= mid && r.bottom > mid) {
        chAt = +sec.dataset.ch;
        break;
      }
    }
    syncFoot();
    remember();
  },
  { passive: true }
);
// any change of layout keeps you on the sentence you were reading
function firstVisible() {
  const nodes = page.querySelectorAll('p, h1, h2, h3, li, blockquote, img, td');
  if (prefs.mode !== 'page') {
    const top = reader.getBoundingClientRect().top + 70;
    for (const n of nodes) if (n.getBoundingClientRect().bottom > top) return n;
    return null;
  }
  const left = clip.getBoundingClientRect().left;
  for (const n of nodes) {
    const r = n.getClientRects()[0];
    if (r && r.left >= left - 2 && r.right > left) return n;
  }
  return null;
}
// call before changing anything; the function it returns puts you back on the same sentence
function holdPlace() {
  if (!book || book.type !== 'text') return () => {};
  const n = firstVisible(),
    share = pages > 1 ? pageAt / (pages - 1) : 0,
    top = n && prefs.mode !== 'page' ? n.getBoundingClientRect().top : 0;
  return () => {
    measure();
    if (prefs.mode === 'page') {
      const r = n && n.getClientRects()[0];
      pageAt = r
        ? clamp(Math.floor((r.left - page.getBoundingClientRect().left + 4) / stepPx), 0, pages - 1)
        : clamp(Math.round(share * (pages - 1)), 0, pages - 1);
      place();
    } else if (n) reader.scrollTop += n.getBoundingClientRect().top - top;
    syncFoot();
    remember();
  };
}
function relayout() {
  holdPlace()();
}
// size, width and typeface rebreak the lines, so the new layout arrives the same way a new page does:
// growing comes in from the right, shrinking from the left
function retype(d) {
  const change = () => {
    const back = holdPlace();
    applyType();
    back();
  };
  if (book && book.type === 'text') sweep(d, clip, textGhost, change);
  else change();
}
function setSize(v) {
  const d = v > prefs.size ? 1 : -1;
  prefs.size = v;
  retype(d);
}
function setWidth(v) {
  const d = v > prefs.width ? 1 : -1;
  prefs.width = v;
  retype(d);
}
// line spacing doesn't rebreak anything, so it can simply breathe
let lhBack = null,
  lhEnd = 0;
function setLh(v) {
  lhBack = lhBack || holdPlace();
  prefs.lh = v;
  applyType();
  const back = lhBack,
    until = (lhEnd = performance.now() + 440);
  const follow = () => {
    if (until !== lhEnd) return;
    if (performance.now() < until) {
      if (prefs.mode !== 'page') back();
      requestAnimationFrame(follow);
      return;
    }
    lhBack = null;
    back();
    remember();
  };
  if (reduce) {
    lhEnd = 0;
    lhBack = null;
    back();
  } else requestAnimationFrame(follow);
}
new ResizeObserver(() => {
  if (!book) return;
  if (book.type === 'text') relayout();
  else if (prefs.fit === 'page') fitPlate();
  placeRing();
}).observe(reader);

// ---------- comics and scans: the page is the thing ----------
let plateToken = 0,
  plateCache = new Map(),
  plateWas = 0;
const asElement = async (p) => {
  if (plateCache.has(p)) return plateCache.get(p);
  let node;
  if (p.url) {
    node = new Image();
    node.src = p.url;
    node.alt = '';
    await node.decode().catch(() => {});
  } else node = await p.draw();
  plateCache.set(p, node);
  return node;
};
const plateNode = () => plate.firstElementChild;
function fitPlate() {
  const n = plateNode();
  if (!n || n.classList.contains('plate-hits')) return;
  const nw = n.naturalWidth || n.width,
    nh = n.naturalHeight || n.height,
    W = reader.clientWidth - 48,
    H = reader.clientHeight - 88;
  const z = prefs.fit === 'width' ? Math.min(W, Math.max(nw, 1100)) / nw : Math.min(W / nw, H / nh);
  n.style.width = Math.round(nw * z) + 'px';
  n.style.height = Math.round(nh * z) + 'px';
  paintPlate(false);
}
async function showPlate(i) {
  const token = ++plateToken;
  chAt = clamp(i, 0, book.pages.length - 1);
  syncFoot();
  remember();
  let node;
  // a page that can't be drawn is a blank sheet, not a stopped reader
  try {
    node = await asElement(book.pages[chAt]);
  } catch {
    node = el('canvas');
  }
  if (token !== plateToken || !book) return;
  const swap = () => {
    plate.replaceChildren(node);
    fitPlate();
    reader.scrollTop = 0;
    paintPlate(true);
  };
  endTurn();
  if (plate.firstElementChild && plate.firstElementChild !== node)
    sweep(chAt >= plateWas ? 1 : -1, plate, plateGhost, swap);
  else swap();
  plateWas = chAt;
  // the next page should already be there when you reach it
  if (chAt + 1 < book.pages.length) asElement(book.pages[chAt + 1]).catch(() => {});
  if (plateCache.size > 8)
    [...plateCache.keys()].slice(0, plateCache.size - 8).forEach((k) => {
      if (k !== book.pages[chAt]) plateCache.delete(k);
    });
}

// ---------- opening: the Rust side checks the file and its lock, the page lays it out ----------
async function readAny(buf, kind, name) {
  if (kind === 'epub') return readEpub(buf);
  if (kind === 'txt') return readTxt(decodeText(buf), name);
  if (kind === 'fb2' || kind === 'fbz' || kind === 'fb2zip') return readFb2(buf);
  if (kind === 'mobi') return readMobi(buf);
  // a CBT's or CB7's pages come from the Rust side as a plain ZIP (src-tauri/src/comics.rs)
  if (kind === 'cbz' || kind === 'cbt' || kind === 'cb7') return readCbz(buf, name);
  if (kind === 'djvu') return readDjvu(buf, name);
  throw new Refused('type');
}
// the serif is Books' own font, in pieces by character (books/scripts/split-serif.py, books/fonts/serif): nothing of it
// is read for the empty window. When a text book opens, the pieces its characters are in are read before it is laid
// out, so the page a book reopens on is counted in the face it is read in, and no page waits on a piece later. A
// Chinese book needs two of the eleven pieces, sometimes three. Read even when the sans is chosen, so switching faces
// later never waits on it
const words = (html) =>
  html.replace(/<[^>]*>/g, '').replace(/&#(x[0-9a-f]+|\d+);/gi, (m, n) => {
    try {
      return String.fromCodePoint(n[0] === 'x' || n[0] === 'X' ? parseInt(n.slice(1), 16) : +n);
    } catch {
      return '';
    }
  });
function bookChars(b) {
  const seen = new Set();
  const add = (text) => {
    for (const ch of text) if (ch > '~') seen.add(ch);
  };
  add(b.title || '');
  add(b.author || '');
  for (const t of b.toc || []) add(t.title || '');
  for (const c of b.chapters) {
    add(c.title || '');
    add(words(c.html));
  }
  for (const n of Object.values(b.notes || {})) add(words(String(n)));
  return 'Aa1' + [...seen].join('');
}
const serif = (b) =>
  document.fonts ? document.fonts.load('1em "Noto Serif SC"', bookChars(b)).catch(() => {}) : Promise.resolve();
async function openPath(path) {
  const r = await own.openFile(path);
  if (r.status !== 'ok') return r;
  const images = newImages();
  try {
    const b = await readAny(await own.bookBytes(), r.kind, r.name);
    b.images = images;
    if (b.type === 'text' ? !b.chapters.length : !b.pages.length) throw new Error('empty');
    if (b.type === 'text') await serif(b);
    // whether it has words to find is known before it is shown, so the top bar never changes under you
    b.searchable = b.type === 'text' || (b.kind === 'djvu' && (await hasWords(b)));
    pending = { path: r.path, book: b };
    return r;
  } catch (e) {
    own.bookFailed();
    releaseImages(images);
    const kind = e instanceof Refused ? e.kind : 'read';
    return { status: kind === 'drm' ? 'drm' : kind === 'type' ? 'refused' : 'damaged' };
  }
}
function showBook(b, r) {
  pending = null;
  shell.closeAll();
  closeFind();
  closeNote();
  endTurn();
  if (book) {
    remember();
    releaseImages(book.images);
  }
  b.key = r.name + ':' + r.size;
  b.file = r.name;
  book = b;
  plateCache = new Map();
  root.classList.toggle('book-pages', b.type === 'pages');
  if (b.type === 'text') {
    const sample = b.chapters
      .slice(0, 3)
      .map((c) => c.html)
      .join('')
      .replace(/<[^>]+>/g, '')
      .slice(0, 4000);
    b.latin = (sample.match(/[㐀-鿿]/g) || []).length < sample.length * 0.15;
  }
  root.classList.toggle('latin', !!b.latin);
  // the book's own language, for hyphens and line breaks; the window keeps its own
  page.lang = b.latin ? 'en' : 'zh-CN';
  page.innerHTML = '';
  plate.replaceChildren();
  const seen = progress[b.key];
  tab = 'toc';
  pinned = 0;
  closeGoto();
  drawToc();
  drawType();
  applyType();
  if (b.type === 'text') showChapter(seen ? seen.ch : 0, seen ? seen.at : 0, true);
  else showPlate(seen ? seen.ch : 0);
  own.bookOpened(r.path);
  canFind = !!b.searchable;
  syncFindable();
  showChrome(2400);
}
function closeBook() {
  if (book) {
    remember();
    releaseImages(book.images);
  }
  endTurn();
  closeNote();
  closeFind();
  closeGoto();
  book = null;
  pending = null;
  plateCache = new Map();
  plateToken++;
  root.classList.remove('book-pages', 'latin');
  page.innerHTML = '';
  plate.replaceChildren();
  footL.textContent = '';
  footR.textContent = '';
  root.classList.remove('marked');
  tocPanel.textContent = '';
  typePanel.textContent = '';
  tocRows = [];
  showChrome(0);
}

// ---------- the top only comes when wanted ----------
let hideTimer = 0,
  overTop = false;
// anything of the family's that's open keeps the top where it is, and so does find
const FAMILY_OPEN = '.side.on, .set.on, .pop.on, .dlg.on, .notice.on, .ask-wrap.on, .drop-view.on';
const familyOpen = () => !!document.querySelector(FAMILY_OPEN);
const anyOpen = () => !!document.querySelector(FAMILY_OPEN + ', .cfind.on');
// a timer that has run, or was cleared, is 0, so moving away from the top can start the next one
function showChrome(ms) {
  root.classList.add('chrome-on');
  clearTimeout(hideTimer);
  hideTimer = ms
    ? setTimeout(() => {
        hideTimer = 0;
        hideChrome();
      }, ms)
    : 0;
}
function hideChrome() {
  if (!book || anyOpen() || overTop) return;
  root.classList.remove('chrome-on');
}
addEventListener('mousemove', (e) => {
  // the top edge brings the top; the bottom edge brings it too, with the page dock under the book
  overTop = e.clientY < 64 || (!!book && e.clientY > innerHeight - 90);
  if (overTop) showChrome(0);
  else if (root.classList.contains('chrome-on') && !hideTimer)
    hideTimer = setTimeout(() => {
      hideTimer = 0;
      hideChrome();
    }, 900);
});
// into full screen with the top showing for a moment, as when a book opens (on the Mac the top only steps aside there)
let wasFull = false;
hear('frame-state', ({ full }) => {
  if (mac && full && !wasFull && book) showChrome(2400);
  wasFull = !!full;
});
// a notice from outside, or a save, has to be seen
// settings, the save card, the file menu or a dropped file put find away, as in PDF
new MutationObserver(() => {
  if (document.querySelector('.set.on, .pop.on, .dlg.on, .ask-wrap.on, .drop-view.on')) closeFind();
  if (anyOpen()) showChrome(0);
}).observe(document.body, { subtree: true, attributes: true, attributeFilter: ['class'] });

// ---------- turning by hand: the sides turn, the middle shows the top, a selection is left alone ----------
let downAt = null;
reader.addEventListener('pointerdown', (e) => {
  downAt = [e.clientX, e.clientY];
});
reader.addEventListener('click', (e) => {
  if (!book) return;
  const a = e.target.closest('a.noteref');
  if (a) {
    e.preventDefault();
    showNote(a);
    return;
  }
  if (notePop.classList.contains('on')) {
    closeNote();
    return;
  }
  if (familyOpen()) {
    shell.closeAll();
    return;
  }
  if (String(getSelection()).length || (downAt && Math.hypot(e.clientX - downAt[0], e.clientY - downAt[1]) > 6)) return;
  const x = e.clientX / reader.clientWidth;
  if (x < 0.3 && (prefs.mode === 'page' || book.type === 'pages')) turn(-1);
  else if (x > 0.7 && (prefs.mode === 'page' || book.type === 'pages')) turn(1);
  else if (findOn()) showChrome(0);
  else if (root.classList.contains('chrome-on')) {
    clearTimeout(hideTimer);
    hideTimer = 0;
    root.classList.remove('chrome-on');
  } else showChrome(0);
});
let wheelAt = 0;
const fromTrackpad = mac ? wheelSource() : null,
  swipe = swipeTurns({ axis: 'any' });
reader.addEventListener(
  'wheel',
  (e) => {
    if (!book) return;
    if (book.type === 'pages' && prefs.fit === 'width') return;
    if (book.type === 'text' && prefs.mode === 'scroll') return;
    e.preventDefault();
    // the Mac's trackpad: one page per swipe, however long its momentum lasts (gestures.js); a mouse's wheel as below
    if (fromTrackpad && fromTrackpad(e) === 'trackpad') {
      const d = swipe(e.deltaX, e.deltaY, e.timeStamp);
      if (d) turn(d);
      return;
    }
    const now = performance.now();
    if (now - wheelAt < 280 || Math.abs(e.deltaY) + Math.abs(e.deltaX) < 8) return;
    wheelAt = now;
    turn(Math.sign(e.deltaY || e.deltaX));
  },
  { passive: false }
);
// after the family's own keys (open, save as, Escape closing what's open)
function keys(e) {
  const k = e.key;
  const mod = modKey(e) && !e.altKey;
  if (mod && k.toLowerCase() === 'f') {
    e.preventDefault();
    openFind();
    return;
  }
  if (mod && k.toLowerCase() === 'd' && !e.shiftKey) {
    e.preventDefault();
    toggleMark();
    return;
  }
  if (mod && k.toLowerCase() === 'g' && !e.shiftKey) {
    e.preventDefault();
    openGoto();
    return;
  }
  // the Mac's ⌘↑ ⌘↓: the book's start and end, as Home and End (fn ← →) are
  const edge = macEdge(e);
  if (!book || ((e.ctrlKey || e.metaKey || e.altKey) && !edge)) return;
  if (k === 'Escape') {
    // the Mac: an ⎋ that puts something away is used up here; one with nothing to put away goes on to the system, whose
    // ⎋ leaves full screen
    if (closeGoto()) return used(e);
    if (!notePop.classList.contains('on') && closeFind()) return used(e);
    if (notePop.classList.contains('on')) used(e);
    closeNote();
    return;
  }
  // the side panels stay open while you read on; the family's dialogs and menus keep the keys
  if (
    /INPUT|TEXTAREA/.test(document.activeElement.tagName)
    || document.querySelector('.set.on, .pop.on, .dlg.on, .ask-wrap.on')
  )
    return;
  if (edge) {
    e.preventDefault();
    toEdge(edge === 'end');
  } else if (['ArrowRight', 'ArrowDown', 'PageDown', ' '].includes(k)) {
    e.preventDefault();
    turn(e.shiftKey && k === ' ' ? -1 : 1);
  } else if (['ArrowLeft', 'ArrowUp', 'PageUp'].includes(k)) {
    e.preventDefault();
    turn(-1);
  } else if (k === 'Home' || k === 'End') {
    e.preventDefault();
    toEdge(k === 'End');
  }
}
function toEdge(end) {
  if (book.type === 'pages') showPlate(end ? book.pages.length - 1 : 0);
  else if (end) showChapter(book.chapters.length - 1, 'end');
  else showChapter(0, 0);
}

// ---------- notes open where you are ----------
function showNote(a) {
  const html = book.notes[a.dataset.note];
  if (html == null) return;
  notePop.innerHTML = `<p class="note-h"></p><div class="note-b">${html}</div>`;
  notePop.firstChild.textContent = T('脚注与注释');
  const r = a.getBoundingClientRect(),
    box = shell.app.getBoundingClientRect(),
    w = Math.min(360, box.width - 32);
  notePop.style.width = w + 'px';
  notePop.style.left = clamp(r.left + r.width / 2 - w / 2, 16, box.width - w - 16) + 'px';
  notePop.classList.add('on');
  const h = notePop.offsetHeight;
  notePop.style.top = (r.bottom + 12 + h > box.height - 16 ? r.top - h - 12 : r.bottom + 12) + 'px';
}
function closeNote() {
  notePop.classList.remove('on');
}

// ---------- contents: the family's sliding pill ----------
let tocRows = [],
  ring = null;
function drawToc() {
  tocPanel.textContent = '';
  const h = el('h3', '', book.title || book.file);
  const by = el('span', 'side-label', book.author || '');
  by.hidden = !book.author;
  const list = el('div', 'toc-list');
  // the panel's two pages, Slides' two tabs: the contents, and this book's bookmarks
  tabBox = tabs();
  marksBox = el('div', 'mark-list');
  marksBox.id = 'marks';
  tocPanel.classList.toggle('show-marks', tab === 'marks');
  ring = el('i', 'toc-pl');
  list.appendChild(ring);
  const rows = book.type === 'text' ? book.toc : book.pages.map((_, i) => ({ title: String(i + 1), ch: i, depth: 0 }));
  tocRows = rows.map((t) => {
    const b = el('button', 'toc-row');
    b.type = 'button';
    b.style.paddingLeft = 12 + (t.depth || 0) * 14 + 'px';
    b.textContent = t.title || chapterTitle(t.ch);
    b.dataset.ch = t.ch;
    b.addEventListener('mousedown', (e) => e.preventDefault());
    b.addEventListener('click', () => {
      sound();
      if (book.type === 'pages') showPlate(t.ch);
      else if (prefs.mode === 'page') glide(() => showChapter(t.ch, 0, false), t.ch < chAt ? -1 : 1);
      else showChapter(t.ch, 0);
      if (innerWidth < 720) shell.closeAll();
    });
    list.appendChild(b);
    return b;
  });
  tocPanel.append(h, by, tabBox, list, marksBox);
  tabBox.sync();
  drawMarks();
}
function markToc() {
  if (!tocRows.length) return;
  let on = null;
  for (const b of tocRows) if (+b.dataset.ch <= chAt) on = b;
  tocRows.forEach((b) => b.setAttribute('aria-current', String(b === on)));
  placeRing();
}
function placeRing() {
  const on = tocRows.find((b) => b.getAttribute('aria-current') === 'true');
  if (!on || !ring) return;
  ring.style.transform = `translateY(${on.offsetTop}px)`;
  ring.style.height = on.offsetHeight + 'px';
  if (tocPanel.classList.contains('on')) {
    const list = on.parentElement;
    if (on.offsetTop < list.scrollTop || on.offsetTop > list.scrollTop + list.clientHeight - 60)
      list.scrollTop = on.offsetTop - list.clientHeight / 3;
  }
}

// ---------- layout: what makes reading comfortable, and only that ----------
// the family's chips, as many to a row as the words allow
function choice(id, label, opts, get, set, columns = opts.length) {
  const box = el('div'),
    row = chips({
      id,
      options: opts,
      get: () => String(get()),
      pick: (v) => {
        set(v);
        savePrefs();
      },
    });
  row.style.gridTemplateColumns = `repeat(${columns}, minmax(0, 1fr))`;
  box.append(el('span', 'side-label', T(label)), row);
  return box;
}
const steps = (values, names) => values.map((v, i) => [String(v), names[i]]);
function drawType() {
  typePanel.textContent = '';
  const body = el('div', 'side-body');
  typePanel.append(el('h3', '', T('阅读排版')), body);
  if (book.type === 'pages') {
    body.append(
      choice(
        'fitChips',
        '页面缩放',
        [
          ['page', '适合整页'],
          ['width', '适合宽度'],
        ],
        () => prefs.fit,
        (v) => {
          prefs.fit = v;
          applyType();
          fitPlate();
        }
      )
    );
  } else {
    body.append(
      choice(
        'fontChips',
        '字体风格',
        [
          ['serif', '衬线（宋体）'],
          ['sans', '无衬线（黑体）'],
        ],
        () => prefs.font,
        (v) => {
          if (v === prefs.font) return;
          prefs.font = v;
          retype(v === 'sans' ? 1 : -1);
        }
      ),
      // the longer English names need a second row
      choice(
        'sizeChips',
        '字号',
        steps(STEPS.size, ['极小', '较小', '适中', '较大', '极大']),
        () => prefs.size,
        (v) => {
          if (+v !== prefs.size) setSize(+v);
        },
        longWords() ? 3 : 5
      ),
      choice(
        'lhChips',
        '行距',
        steps(STEPS.lh, ['紧凑', '适中', '舒展']),
        () => prefs.lh,
        (v) => setLh(+v)
      ),
      choice(
        'widthChips',
        '版心宽度',
        steps(STEPS.width, ['窄版心', '适中', '宽版心']),
        () => prefs.width,
        (v) => {
          if (+v !== prefs.width) setWidth(+v);
        }
      ),
      choice(
        'modeChips',
        '阅览模式',
        [
          ['page', '翻页模式'],
          ['scroll', '连续滚动'],
        ],
        () => prefs.mode,
        (v) => {
          if (v === prefs.mode) return;
          const at = share();
          prefs.mode = v;
          applyType();
          reader.scrollTop = 0;
          placeAt(chAt, at, true);
        }
      )
    );
  }
}
// the family's panel button opens and closes it; the top stays while a panel is open, and the contents find their pill
for (const [p, b] of [
  [tocPanel, tocBtn],
  [typePanel, typeBtn],
])
  b.addEventListener('click', () => {
    if (p.classList.contains('on')) {
      closeNote();
      closeFind();
      closeGoto();
      showChrome(0);
      requestAnimationFrame(placeRing);
      if (p === tocPanel && tabBox) tabBox.sync();
    } else showChrome(1200);
  });

// ---------- find: DotAny PDF's glass, its keys and its two highlights, across the whole book ----------
const findBox = el('div', 'cfind');
findBox.setAttribute('role', 'search');
const findWrap = el('span', 'fi'),
  findIn = document.createElement('input');
findIn.type = 'text';
findIn.id = 'findText';
findIn.placeholder = '查找';
findIn.spellcheck = false;
findIn.autocomplete = 'off';
findWrap.appendChild(findIn);
const findCount = el('span', 'f-count dyn');
const fBtn = (id, n, tip, run) => {
  const b = el('button', 'f-b');
  b.type = 'button';
  b.id = id;
  b.innerHTML = icon(n);
  b.setAttribute('aria-label', tip);
  b.dataset.tip = tip;
  b.addEventListener('mousedown', (e) => e.preventDefault());
  b.addEventListener('click', () => {
    sound();
    run();
  });
  return b;
};
const fRow = el('div', 'f-row');
fRow.append(
  findWrap,
  findCount,
  fBtn('findPrev', 'up', '上一个', () => stepFind(-1)),
  fBtn('findNext', 'down', '下一个', () => stepFind(1)),
  fBtn('findClose', 'close', '关闭', () => closeFind())
);
findBox.appendChild(fRow);
shell.place(findBox);
shell.translates(findBox);
inkfall(findIn);
// the glass sits under the top, and the notice row when there is one
new ResizeObserver(() => {
  root.style.setProperty('--chrome-now', shell.chrome.offsetHeight + 'px');
}).observe(shell.chrome);
const findOn = () => findBox.classList.contains('on');

// a book of pictures has nothing to find, so there is no find button; a scan with a text layer can be searched. Reading
// a scan's text layer, without its pictures, is quick enough to do before the book is shown
let canFind = false;
function syncFindable() {
  findBtn.hidden = !canFind;
}
async function hasWords(b) {
  for (const p of b.pages) {
    try {
      const w = await p.words();
      if (w.zones.some((z) => z.text.trim())) return true;
    } catch {
      /* a page that can't be read has no words */
    }
  }
  return false;
}
// the work is done in small pieces, so turning pages and typing never wait for a long book
// (a plain task, not scheduler.yield(), whose turn comes before everything else that waits)
const breathe = () => new Promise((r) => setTimeout(r));

// what was found: for each chapter (or each page of a scan), where the folded query starts in its folded text
let found = [],
  foundN = 0,
  at = null,
  q = '',
  findToken = 0,
  searching = false;
// each chapter's folded text, made the first time it is searched and let go when find closes
let index = [];
const inert = document.implementation.createHTMLDocument(''),
  holder = inert.createElement('article');
const units = () => (book.type === 'text' ? book.chapters.length : book.pages.length);
async function unitText(i) {
  if (index[i] == null) {
    if (book.type === 'text') {
      // read as the page reads it (an <article>'s HTML), in a document that loads nothing
      holder.innerHTML = chapterHtml(i);
      index[i] = foldChapter(holder.firstElementChild).text;
      holder.textContent = '';
    } else {
      let p = { text: '' };
      try {
        const w = await book.pages[i].words();
        p = foldWords(w.zones);
        p.w = w.w;
        p.h = w.h;
      } catch {
        /* no words */
      }
      index[i] = p;
    }
  }
  return book.type === 'text' ? index[i] : index[i].text;
}
const numberOf = (h) => {
  let n = h.k + 1;
  for (let i = 0; i < h.i; i++) n += found[i] ? found[i].length : 0;
  return n;
};

// ---------- the marks: the family's two highlights, on the words themselves ----------
const marks = typeof Highlight === 'function' && window.CSS && CSS.highlights;
let maps = new WeakMap();
const mapOf = (sec) => {
  let m = maps.get(sec);
  if (!m) {
    m = foldChapter(sec, true);
    maps.set(sec, m);
  }
  return m;
};
const sectionOf = (i) => page.querySelector(`:scope > [data-ch="${i}"]`);
function hitRange(i, k) {
  const sec = sectionOf(i);
  return sec && found[i] ? rangeOf(mapOf(sec), found[i][k], q.length) : null;
}
function paintHits() {
  if (!marks) return;
  const all = new Highlight(),
    cur = new Highlight();
  if (findOn() && q && book && book.type === 'text') {
    for (const sec of page.children) {
      const i = +sec.dataset.ch,
        list = found[i];
      if (!list) continue;
      const m = mapOf(sec);
      list.forEach((s, k) => {
        const r = rangeOf(m, s, q.length);
        if (r) (at && at.i === i && at.k === k ? cur : all).add(r);
      });
    }
  }
  CSS.highlights.set('find-hit', all);
  CSS.highlights.set('find-cur', cur);
}
// a chapter put on the page (turning, scrolling on, a new layout) gets its marks
new MutationObserver(() => {
  if (findOn()) paintHits();
}).observe(page, { childList: true });
// on a scan the marks are boxes over the words of its text layer, as PDF draws them
function paintPlate(reveal) {
  const old = plate.querySelector(':scope > .plate-hits');
  if (old) old.remove();
  const n = plateNode(),
    p = book && book.type === 'pages' ? index[chAt] : null,
    list = found[chAt];
  if (!findOn() || !q || !n || !p || !p.zones || !list) return;
  const layer = el('div', 'plate-hits');
  Object.assign(layer.style, {
    left: n.offsetLeft + 'px',
    top: n.offsetTop + 'px',
    width: n.offsetWidth + 'px',
    height: n.offsetHeight + 'px',
  });
  list.forEach((s, k) =>
    boxesOf(p, s, q.length, p.h).forEach(([x, y, w, h]) => {
      const b = el('i', at && at.i === chAt && at.k === k ? 'p-hit cur' : 'p-hit');
      Object.assign(b.style, {
        left: (x / p.w) * 100 + '%',
        top: (y / p.h) * 100 + '%',
        width: (w / p.w) * 100 + '%',
        height: (h / p.h) * 100 + '%',
      });
      layer.appendChild(b);
    })
  );
  plate.appendChild(layer);
  const c = reveal && layer.querySelector('.cur');
  if (c) {
    const r = c.getBoundingClientRect(),
      box = reader.getBoundingClientRect();
    if (r.top < box.top + 90 || r.bottom > box.bottom - 120)
      reader.scrollTop += r.top - box.top - reader.clientHeight * 0.36;
  }
}

// ---------- going to a match: its page, in either reading mode; where you are is where you stay ----------
const pageOfRange = (r) => {
  const c = r.getClientRects()[0] || r.getBoundingClientRect();
  return clamp(Math.floor((c.left - page.getBoundingClientRect().left + 4) / stepPx), 0, pages - 1);
};
// a match from where you're reading on, not from the start of the book
function fromHere(i, k) {
  if (i !== chAt) return i > chAt;
  if (book.type === 'pages') return true;
  const r = hitRange(i, k);
  if (!r) return true;
  if (prefs.mode === 'page') return pageOfRange(r) >= pageAt;
  return r.getBoundingClientRect().bottom > reader.getBoundingClientRect().top + 70;
}
function showHit() {
  syncCount();
  if (!at || !book) return;
  if (book.type === 'pages') {
    if (at.i !== chAt) showPlate(at.i);
    else paintPlate(true);
    return;
  }
  const { i, k } = at;
  if (prefs.mode === 'page') {
    const onto = () => {
      const r = hitRange(i, k);
      if (r) {
        pageAt = pageOfRange(r);
        place();
      }
      syncFoot();
      remember();
      paintHits();
    };
    if (i !== chAt) {
      glide(
        () => {
          showChapter(i, 0, false);
          onto();
        },
        i < chAt ? -1 : 1
      );
      return;
    }
    const r = hitRange(i, k),
      n = r ? pageOfRange(r) : pageAt;
    if (n !== pageAt) glide(onto, n < pageAt ? -1 : 1);
    else paintHits();
    return;
  }
  // scrolling: a chapter far away is put on the page first; a match already there is scrolled to
  const fresh = !sectionOf(i);
  if (fresh) showChapter(i, 0, false);
  paintHits();
  const r = hitRange(i, k);
  if (!r) return;
  const c = r.getBoundingClientRect(),
    box = reader.getBoundingClientRect();
  if (fresh || c.top < box.top + 90 || c.bottom > box.bottom - 120)
    reader.scrollTo({
      top: reader.scrollTop + c.top - box.top - reader.clientHeight * 0.36,
      behavior: fresh || reduce ? 'auto' : 'smooth',
    });
}
function syncCount() {
  findCount.textContent = !q
    ? ''
    : at
      ? `${numberOf(at)}/${foundN}`
      : foundN
        ? `0/${foundN}`
        : searching
          ? ''
          : T('无匹配结果');
  findBox.classList.toggle('none', !!q && !searching && !foundN);
}

// ---------- searching: from the chapter you're in to the end, then from the start; the count grows as it goes ----------
async function runFind() {
  const token = ++findToken;
  q = book ? foldQuery(findIn.value) : '';
  found = [];
  foundN = 0;
  at = null;
  searching = !!q;
  paintHits();
  paintPlate(false);
  syncCount();
  if (!q) return;
  const n = units(),
    from = clamp(chAt, 0, n - 1);
  let t = performance.now();
  for (let s = 0; s < n; s++) {
    const i = (from + s) % n;
    if (performance.now() - t > 12) {
      syncCount();
      await breathe();
      if (token !== findToken || !book) return;
      t = performance.now();
    }
    const text = await unitText(i);
    if (token !== findToken || !book) return;
    const list = matches(text, q);
    if (!list.length) continue;
    found[i] = list;
    foundN += list.length;
    if (!at) {
      // after the end the search starts again at the beginning, so the first one found there is the book's first
      const k = i < from ? 0 : list.findIndex((_, x) => fromHere(i, x));
      if (k >= 0) {
        at = { i, k };
        showHit();
        continue;
      }
    }
    if (book.type === 'text' ? sectionOf(i) : i === chAt) {
      paintHits();
      paintPlate(false);
    }
  }
  searching = false;
  // every match was before where you are: the first one in the book
  if (!at && foundN) {
    at = { i: found.findIndex(Boolean), k: 0 };
    showHit();
  }
  syncCount();
}
function stepFind(d) {
  if (!foundN || !book) return;
  if (!at) at = { i: found.findIndex(Boolean), k: 0 };
  else {
    const n = units();
    let i = at.i,
      k = at.k + d;
    for (let guard = 0; !(found[i] && k >= 0 && k < found[i].length) && guard <= n; guard++) {
      i = (i + d + n) % n;
      k = d > 0 ? 0 : (found[i] || []).length - 1;
    }
    at = { i, k };
  }
  showHit();
}
function openFind() {
  if (!book || !canFind) return;
  if (!findOn()) {
    shell.closeAll();
    closeNote();
    const s = String(getSelection()).trim();
    if (s && s.length < 80 && !/\n/.test(s)) findIn.value = s;
    findBox.classList.add('on');
    findBtn.setAttribute('aria-pressed', 'true');
    showChrome(0);
    runFind();
  }
  setTimeout(() => {
    findIn.focus();
    findIn.select();
  }, 30);
}
// closing leaves you where you are now: on the last match you went to, not where you started
function closeFind() {
  if (!findOn()) return false;
  findBox.classList.remove('on');
  findBtn.setAttribute('aria-pressed', 'false');
  findToken++;
  found = [];
  foundN = 0;
  at = null;
  q = '';
  searching = false;
  index = [];
  maps = new WeakMap();
  if (findBox.contains(document.activeElement)) findIn.blur();
  paintHits();
  paintPlate(false);
  if (book) {
    remember();
    showChrome(1200);
  }
  return true;
}
findIn.addEventListener('input', () => {
  clearTimeout(runFind.t);
  runFind.t = setTimeout(runFind, 140);
});
findIn.addEventListener('keydown', (e) => {
  e.stopPropagation();
  if (e.key === 'Enter') {
    e.preventDefault();
    // typed and Enter at once: the new words are looked for first
    if (q !== foldQuery(findIn.value)) {
      clearTimeout(runFind.t);
      runFind();
    } else stepFind(e.shiftKey ? -1 : 1);
  } else if (e.key === 'Escape') {
    // (the field keeps the focus once the find is put away: an ⎋ then has nothing here to put away)
    if (closeFind()) used(e);
  } else if (modKey(e) && e.key.toLowerCase() === 'f') {
    e.preventDefault();
    findIn.select();
  }
});
findBtn.addEventListener('click', () => {
  sound();
  if (!closeFind()) openFind();
});

// ---------- page numbers that stay put: a fixed amount of text, about a printed page ----------
// A screen's worth changes with the window, the size and the typeface, so it can't be a page number. A page here is
// about 500 Chinese characters or 1,600 letters of a Western book, roughly a paperback page, and every chapter starts
// on a new page, as in print. The number depends on the book's words only: not the window, the type, or pages and
// scrolling. Comics and scans have real pages.
const UNIT_ZH = 500,
  UNIT_LATIN = 1600;
function pageMap() {
  const u = book.latin ? UNIT_LATIN : UNIT_ZH;
  if (book.pageMap && book.pageMap.u === u) return book.pageMap;
  const starts = [],
    counts = [];
  let n = 0;
  book.chapters.forEach((c) => {
    starts.push(n);
    const k = Math.max(1, Math.ceil(c.len / u));
    counts.push(k);
    n += k;
  });
  return (book.pageMap = { u, starts, counts, total: n });
}
// the stretch of the chapter's words on screen, as shares of its text from 0 to 1: [where the screen starts, where it
// ends]. Read from the words themselves (the character at the screen's first and last corner), so a new size or window
// that puts the same words on screen gives the same page
const secOf = (i) => page.querySelector(`:scope > [data-ch="${i}"]`);
function offsetAt(sec, x, y) {
  // only the page counts: the glass over it (the top, the dock, panels) and the still copy a turn leaves on top for a
  // moment are put out of the way while it is read, and back before anything is drawn
  root.classList.add('reading-words');
  const r = document.caretRangeFromPoint && document.caretRangeFromPoint(x, y);
  root.classList.remove('reading-words');
  if (!r || !sec.contains(r.startContainer)) return null;
  const q = document.createRange();
  q.setStart(sec, 0);
  q.setEnd(r.startContainer, r.startOffset);
  return q.toString().length;
}
function onScreen() {
  const sec = secOf(chAt);
  if (!sec) return [0, 1];
  const len = Math.max(1, sec.textContent.length),
    r = sec.getBoundingClientRect();
  let a, z;
  if (prefs.mode === 'page') {
    const c = clip.getBoundingClientRect();
    a = offsetAt(sec, c.left + 2, c.top + 2);
    z = offsetAt(sec, c.right - 2, c.bottom - 2);
    if (a == null) a = (pageAt / pages) * len;
    if (z == null) z = pageAt === pages - 1 ? len : ((pageAt + 1) / pages) * len;
  } else {
    const box = reader.getBoundingClientRect(),
      top = box.top + 76,
      bottom = box.bottom - 68;
    a = top <= r.top ? 0 : top >= r.bottom ? len : offsetAt(sec, r.left + 2, top);
    z = bottom >= r.bottom ? len : bottom <= r.top ? 0 : offsetAt(sec, r.right - 2, bottom);
    if (a == null) a = clamp((top - r.top) / Math.max(1, r.height), 0, 1) * len;
    if (z == null) z = clamp((bottom - r.top) / Math.max(1, r.height), 0, 1) * len;
  }
  return [clamp(a / len, 0, 1), clamp(Math.max(a + 1, z) / len, 0, 1)];
}
// the place of a share of a chapter's words, as a rectangle on the page
function rectAt(sec, at) {
  let left = Math.round(clamp(at, 0, 1) * sec.textContent.length);
  const w = document.createTreeWalker(sec, NodeFilter.SHOW_TEXT);
  for (let n = w.nextNode(); n; n = w.nextNode()) {
    if (left <= n.nodeValue.length) {
      // a run of spaces between blocks has no box of its own: the next visible character does
      if (!n.nodeValue.slice(left).trim()) {
        left = 0;
        continue;
      }
      const q = document.createRange(),
        o = left + (n.nodeValue.slice(left).length - n.nodeValue.slice(left).trimStart().length);
      q.setStart(n, o);
      q.setEnd(n, Math.min(n.nodeValue.length, o + 1));
      const c = q.getClientRects()[0];
      if (c) return c;
      left = 0;
      continue;
    }
    left -= n.nodeValue.length;
  }
  return null;
}
// where the screen starts, for a bookmark
const share = () => onScreen()[0];
// a page you went to stays the page shown while it is on screen; otherwise it is the page the screen starts on
let pinned = 0;
function nowPage() {
  if (book.type === 'pages') return chAt + 1;
  const m = pageMap(),
    k = m.counts[chAt],
    [s0, s1] = onScreen();
  if (pinned) {
    const i = pinned - 1 - m.starts[chAt];
    if (i >= 0 && i < k && i / k < s1 && (i + 1) / k > s0) return pinned;
  }
  return m.starts[chAt] + clamp(Math.floor(s0 * k + 1e-6), 0, k - 1) + 1;
}
const allPages = () => (book.type === 'pages' ? book.pages.length : pageMap().total);
// a page as a chapter and the share of it where the page starts
function placeOf(p) {
  if (book.type === 'pages') return { ch: clamp(p - 1, 0, book.pages.length - 1), at: 0 };
  const m = pageMap();
  p = clamp(p, 1, m.total) - 1;
  let i = 0;
  while (i + 1 < m.starts.length && m.starts[i + 1] <= p) i++;
  return { ch: i, at: (p - m.starts[i]) / m.counts[i] };
}
// a place in the book, in either reading mode: the screen with that share of the chapter on it. Pages turn over to it;
// scrolling lays it out, a little below the top
function goTo(ch, at, back) {
  if (book.type === 'pages') {
    showPlate(ch);
    return;
  }
  closeNote();
  if (prefs.mode === 'page') glide(() => placeAt(ch, at), back ? -1 : 1);
  else placeAt(ch, at);
}
// the chapter laid out with that share of its words at the top of the screen
function placeAt(ch, at, fade = false) {
  showChapter(ch, 0, fade);
  const sec = secOf(ch),
    c = sec && at > 0 ? rectAt(sec, at) : null;
  if (prefs.mode === 'page') {
    pageAt = c
      ? clamp(Math.floor((c.left - page.getBoundingClientRect().left + 4) / stepPx), 0, pages - 1)
      : clamp(Math.floor(at * pages + 1e-6), 0, pages - 1);
    place();
  } else if (sec)
    reader.scrollTop +=
      (c ? c.top : sec.getBoundingClientRect().top + at * sec.offsetHeight)
      - reader.getBoundingClientRect().top
      - 76
      - (c ? 2 : 0);
  syncFoot();
  remember();
}
function goPage(p) {
  if (!book) return;
  p = clamp(p, 1, allPages());
  const now = nowPage();
  if (p === now) return;
  const to = placeOf(p);
  pinned = book.type === 'pages' ? 0 : p;
  goTo(to.ch, to.at, p < now);
}

// ---------- the dock: DotAny Video's, coming and going with the top ----------
const dock = el('div', 'dock-row'),
  dockPill = el('div', 'pill'),
  dockMain = el('div', 'pill-main');
const markBtn = el('button', 'tb');
markBtn.type = 'button';
markBtn.id = 'markBtn';
markBtn.innerHTML = icon('bookmark');
markBtn.setAttribute('aria-pressed', 'false');
const clock = el('button', 'clock dyn');
clock.type = 'button';
clock.id = 'pageClock';
// Video's bar, with where each chapter starts and each bookmark marked on its line
const bar = el('div', 'bar'),
  barTicks = el('div', 'bar-ticks'),
  barFill = el('i', 'bar-fill'),
  barNow = el('i', 'bar-now'),
  barTip = el('span', 'bar-tip dyn');
bar.id = 'pageBar';
bar.append(barTicks, barFill, barNow, barTip);
dockMain.append(markBtn, clock, bar);
dockPill.appendChild(dockMain);
dock.appendChild(dockPill);
// after the panels, so the family's `.side.on ~` moves it aside when one is open, as Video's does
shell.place(dock);
[markBtn, clock].forEach((b) => b.addEventListener('mousedown', (e) => e.preventDefault()));
markBtn.addEventListener('click', () => toggleMark());
clock.addEventListener('click', () => {
  if (!closeGoto()) openGoto();
});
const barPage = (x) => {
  const r = bar.getBoundingClientRect();
  return clamp(Math.round(((x - r.left) / r.width) * (allPages() - 1)) + 1, 1, allPages());
};
function barTipAt(p) {
  const { ch } = placeOf(p);
  barTip.textContent = (book.type === 'pages' ? '' : chapterTitle(ch) + ' · ') + p + ' / ' + allPages();
  barTip.style.left = ((p - 1) / Math.max(1, allPages() - 1)) * 100 + '%';
}
let barDrag = false;
bar.addEventListener('pointermove', (e) => {
  if (book) barTipAt(barPage(e.clientX));
});
bar.addEventListener('pointerdown', (e) => {
  if (!book || e.button) return;
  barDrag = true;
  bar.setPointerCapture(e.pointerId);
  bar.classList.add('drag');
  barTipAt(barPage(e.clientX));
});
bar.addEventListener('pointerup', (e) => {
  if (!barDrag) return;
  barDrag = false;
  bar.classList.remove('drag');
  sound();
  goPage(barPage(e.clientX));
});
bar.addEventListener('pointercancel', () => {
  barDrag = false;
  bar.classList.remove('drag');
});
function syncBar() {
  const n = allPages(),
    f = (nowPage() - 1) / Math.max(1, n - 1);
  barFill.style.width = f * 100 + '%';
  barNow.style.left = f * 100 + '%';
  const key =
    n
    + ':'
    + marksOf()
      .map((m) => m.page)
      .join(',');
  if (barTicks.dataset.k === key) return;
  barTicks.dataset.k = key;
  barTicks.textContent = '';
  if (book.type === 'text')
    pageMap()
      .starts.slice(1)
      .forEach((s) => {
        const t = el('i', 'bar-ch');
        t.style.left = (s / Math.max(1, n - 1)) * 100 + '%';
        barTicks.appendChild(t);
      });
  marksOf().forEach((m) => {
    const t = el('i', 'bar-mark');
    t.style.left = ((m.page - 1) / Math.max(1, n - 1)) * 100 + '%';
    barTicks.appendChild(t);
  });
}
// the line under the book says the page, and whether it has a bookmark (a 14px hint, not a button)
const footMark = el('span', 'foot-mark');
footMark.innerHTML = icon('bookmark');
foot.insertBefore(footMark, footR);
const pageText = (p, n) => {
  const f = document.createDocumentFragment(),
    i = el('i', '', ' / ' + n);
  f.append(String(p), i);
  return f;
};
function syncNav() {
  if (!book) return;
  const p = nowPage(),
    n = allPages(),
    here = !!markHere();
  footR.replaceChildren(pageText(p, n));
  clock.replaceChildren(pageText(p, n));
  root.classList.toggle('marked', here);
  markBtn.setAttribute('aria-pressed', String(here));
  markBtn.setAttribute('aria-label', T(here ? '移除书签' : '添加书签'));
  markBtn.dataset.tip = T(here ? '移除书签 · {Mod+D}' : '添加书签 · {Mod+D}');
  clock.setAttribute('aria-label', T('跳转至页'));
  clock.dataset.tip = T('跳转至页 · {Mod+G}');
  syncBar();
  if (marksBox)
    marksBox.querySelectorAll('.mark-row').forEach((r) => r.toggleAttribute('aria-current', +r.dataset.page === p));
}

// ---------- Go to Page (跳转至页): DotAny Text's Go to Line, opening above the page number ----------
const go = el('div', 'cgoto');
go.setAttribute('role', 'dialog');
go.setAttribute('aria-label', '跳转至页');
const goWrap = el('span', 'fi'),
  goIn = document.createElement('input');
goIn.type = 'text';
goIn.inputMode = 'numeric';
goIn.id = 'gotoPage';
goIn.placeholder = '页码';
goIn.spellcheck = false;
goIn.autocomplete = 'off';
goWrap.appendChild(goIn);
const goHint = el('span', 'go-hint dyn');
go.append(goWrap, goHint);
shell.place(go);
shell.translates(go);
inkfall(goIn);
function openGoto() {
  if (!book) return;
  sound();
  closeFind();
  showChrome(0);
  goIn.value = String(nowPage());
  goHint.textContent = T('共 {n} 页', { n: allPages() });
  go.classList.add('on');
  // it opens where the number is, once the dock has come up
  requestAnimationFrame(() => {
    const r = clock.getBoundingClientRect(),
      box = shell.app.getBoundingClientRect();
    go.style.left = clamp(r.left + r.width / 2 - go.offsetWidth / 2, 12, box.width - go.offsetWidth - 12) + 'px';
  });
  goIn.focus();
  goIn.select();
}
function closeGoto() {
  if (!go.classList.contains('on')) return false;
  go.classList.remove('on');
  if (go.contains(document.activeElement)) goIn.blur();
  return true;
}
goIn.addEventListener('keydown', (e) => {
  e.stopPropagation();
  if (e.key === 'Escape') {
    e.preventDefault();
    sound();
    closeGoto();
  } else if (e.key === 'Enter') {
    e.preventDefault();
    sound();
    const n = parseInt(goIn.value.replace(/[^\d]/g, ''), 10);
    closeGoto();
    if (n) goPage(n);
  } else if (modKey(e) && e.key.toLowerCase() === 'g') {
    e.preventDefault();
    goIn.select();
  }
});
document.addEventListener('pointerdown', (e) => {
  if (go.classList.contains('on') && !go.contains(e.target) && !clock.contains(e.target)) closeGoto();
});

// ---------- bookmarks: a place in the words, kept per book (by its name and size), the newest 300 books ----------
let bookmarks = readJson('dotany-books-marks', {}),
  tab = 'toc',
  tabBox = null,
  marksBox = null;
const marksOf = () => (book && bookmarks[book.key]) || [];
const markHere = () => (book ? marksOf().find((m) => m.page === nowPage()) : null);
function saveMarks(list) {
  delete bookmarks[book.key];
  if (list.length) bookmarks[book.key] = list;
  const keys = Object.keys(bookmarks);
  if (keys.length > 300) keys.slice(0, keys.length - 300).forEach((k) => delete bookmarks[k]);
  store.set('dotany-books-marks', JSON.stringify(bookmarks));
}
// another window of this program (the Mac: a window per book) read on in its book or marked it. Each window writes the
// whole list, so it takes the other's into its own copy first, and keeps its own open book's entry as it has it
const thisWindow = window.__TAURI_INTERNALS__.metadata.currentWindow.label;
function tookList(list, value) {
  let theirs = {};
  try {
    theirs = JSON.parse(value) || {};
  } catch {
    /* none */
  }
  if (book) {
    const mine = list[book.key];
    delete theirs[book.key];
    if (mine) theirs[book.key] = mine;
  }
  return theirs;
}
hear('setting', ({ from, key, value }) => {
  if (from === thisWindow) return;
  store.took(key, value);
  if (key === 'dotany-books-at') progress = tookList(progress, value);
  else if (key === 'dotany-books-marks') bookmarks = tookList(bookmarks, value);
});
// the words a bookmark was put at, as it is listed
function snippet() {
  if (book.type === 'pages') return '';
  const n = firstVisible();
  return n ? n.textContent.replace(/\s+/g, ' ').trim().slice(0, 80) : '';
}
function toggleMark() {
  if (!book) return;
  sound();
  const list = marksOf().slice(),
    was = markHere();
  if (was) list.splice(list.indexOf(was), 1);
  else {
    list.push({ page: nowPage(), ch: chAt, at: share(), text: snippet() });
    list.sort((a, b) => a.page - b.page);
  }
  saveMarks(list);
  syncNav();
  drawMarks();
}
function tabs() {
  const box = el('div', 'seg ax-pick'),
    pl = el('i', 'pl');
  box.id = 'tocTabs';
  box.style.setProperty('--c', 2);
  box.appendChild(pl);
  const btns = [
    ['toc', '目录'],
    ['marks', '书签'],
  ].map(([k, t]) => {
    const b = el('button', 'sg', T(t));
    b.type = 'button';
    b.dataset.k = k;
    b.addEventListener('mousedown', (e) => e.preventDefault());
    b.addEventListener('click', () => {
      if (tab === k) return;
      sound();
      setTab(k);
    });
    box.appendChild(b);
    return b;
  });
  // Slides' pickGroup: the pill is put in place without sliding the first time, and slides after that
  let seen = false;
  const place = () => {
    const on = btns.find((b) => b.dataset.k === tab);
    if (!on.offsetWidth) return;
    if (!seen) {
      pl.style.transition = 'none';
      seen = true;
    }
    pl.style.transform = `translateX(${on.offsetLeft}px)`;
    pl.style.width = on.offsetWidth + 'px';
    if (pl.style.transition === 'none') {
      void pl.offsetWidth;
      pl.style.transition = '';
    }
  };
  box.sync = () => {
    btns.forEach((b) => b.setAttribute('aria-pressed', String(b.dataset.k === tab)));
    place();
    requestAnimationFrame(place);
  };
  return box;
}
function setTab(k) {
  tab = k;
  if (!tabBox) return;
  tabBox.sync();
  tocPanel.classList.toggle('show-marks', k === 'marks');
  if (k === 'toc') requestAnimationFrame(placeRing);
}
function drawMarks() {
  if (!marksBox || !book) return;
  marksBox.textContent = '';
  const list = marksOf(),
    p = nowPage();
  if (!list.length) {
    marksBox.appendChild(el('p', 'mark-none', T('暂无书签。按 {Mod+D} 在当前位置添加。')));
    return;
  }
  list.forEach((m) => {
    const row = el('div', 'mark-row'),
      go_ = el('button', 'mark-go');
    go_.type = 'button';
    row.dataset.page = m.page;
    if (m.page === p) row.setAttribute('aria-current', '');
    const name = book.type === 'pages' ? '' : chapterTitle(clamp(m.ch, 0, book.chapters.length - 1));
    const s = el('span', 'mark-s');
    s.append(el('span', 'mark-ch', name), el('span', 'mark-p', T('第 {n} 页', { n: m.page })));
    go_.append(el('span', 'mark-t', m.text || T('第 {n} 页', { n: m.page })), s);
    go_.addEventListener('mousedown', (e) => e.preventDefault());
    go_.addEventListener('click', () => {
      sound();
      const now = nowPage();
      if (m.page !== now) {
        if (book.type === 'text') pinned = m.page;
        goTo(m.ch, m.at, m.page < now);
      }
      if (innerWidth < 720) shell.closeAll();
    });
    const del = el('button', 'f-b mark-del');
    del.type = 'button';
    del.innerHTML = icon('close');
    del.setAttribute('aria-label', T('删除'));
    del.dataset.tip = T('删除');
    del.addEventListener('mousedown', (e) => e.preventDefault());
    del.addEventListener('click', () => {
      sound();
      const l = marksOf().filter((x) => x !== m);
      saveMarks(l);
      drawMarks();
      syncNav();
    });
    row.append(go_, del);
    marksBox.appendChild(row);
  });
}

shell.onLang(() => {
  goIn.placeholder = T('页码');
  if (go.classList.contains('on')) goHint.textContent = T('共 {n} 页', { n: allPages() });
  if (book) {
    drawType();
    drawToc();
    syncFoot();
  }
  if (notePop.classList.contains('on')) notePop.firstChild.textContent = T('脚注与注释');
  findIn.placeholder = T('查找');
  syncCount();
});
applyType();
if (document.fonts && document.fonts.ready)
  document.fonts.ready.then(() => {
    if (book) relayout();
  });

// with DOTANY_TEST set, shell.test.books exposes the open book and its actions, so that an automated test can read and drive it
shell.test.books = {
  get book() {
    return book;
  },
  get ch() {
    return chAt;
  },
  get page() {
    return pageAt;
  },
  get pages() {
    return pages;
  },
  prefs,
  turn,
  percent,
  get progress() {
    return progress;
  },
  // the book's own page numbers, and its bookmarks
  nav: {
    now: () => nowPage(),
    all: () => allPages(),
    onScreen,
    share,
    goTo,
    get pinned() {
      return pinned;
    },
    goPage,
    toggleMark,
    openGoto,
    closeGoto,
    setTab,
    get marks() {
      return marksOf();
    },
    get stored() {
      return bookmarks;
    },
    placeOf,
  },
  find: {
    get on() {
      return findOn();
    },
    get can() {
      return canFind;
    },
    get total() {
      return foundN;
    },
    get number() {
      return at ? numberOf(at) : 0;
    },
    get at() {
      return at && { ch: at.i, k: at.k };
    },
    get searching() {
      return searching;
    },
    get folded() {
      return q;
    },
    open: openFind,
    close: closeFind,
    step: stepFind,
    // the page and the search read each chapter alike: the chapters on screen, folded from the page, are what was searched
    agree() {
      return [...page.children]
        .filter((s) => index[+s.dataset.ch] != null)
        .map((s) => [+s.dataset.ch, foldChapter(s).text === index[+s.dataset.ch]]);
    },
    // the find marks on the page now: [all, current]
    marks() {
      return marks ? [CSS.highlights.get('find-hit')?.size || 0, CSS.highlights.get('find-cur')?.size || 0] : null;
    },
    get current() {
      const r = at && book && book.type === 'text' ? hitRange(at.i, at.k) : null;
      return r ? String(r) : null;
    },
  },
};
shell.begin();
