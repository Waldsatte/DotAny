// DotAny PDF: the page of the program. The top bar, file menu, settings, save card, close prompt, notice row, empty
// window and drop sheet come from shared/web; the pages, the tools on the left, the dock, the panels' insides, find, the
// contents and the pages view are written here. PDF.js reads and draws the pages; the Rust side (src-tauri/src/save.rs)
// writes the file.
import { createShell } from '@dotany/web/shell.js';
import { el, root, reduce, icon, sound, EASE, store } from '@dotany/web/shared.js';
import { T, size as sizeText } from '@dotany/web/i18n.js';
import { inkfall } from '@dotany/web/inkfall-input.js';
import { slider as familySlider } from '@dotany/web/parts.js';
import { fileSrc, on as hear } from '@dotany/web/backend.js';
import { LUCIDE } from './icons.js';
import * as own from './backend.js';
import { isWebAddress } from './links.js';
import * as pdfjs from 'pdfjs-dist/build/pdf.mjs';
import { modKey, os, yRedoes, macEdge } from '@dotany/web/keys.js';
import { mac, pinch } from '@dotany/web/gestures.js';

// PDF.js reads in a thread of its own: the worker built beside this page (scripts/build.mjs), with the HTJ2K guard in it
pdfjs.GlobalWorkerOptions.workerSrc = new URL('./pdf.worker.js', import.meta.url).href;

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
const FORMATS = {
  pdf: { ext: '.pdf', label: 'PDF' },
  docx: { ext: '.docx', label: 'Word' },
  png: { ext: '.png', label: 'PNG' },
  jpg: { ext: '.jpg', label: 'JPG' },
};

// words with a number in them, said in the window's language
const clamp = (v, a, b) => Math.min(b, Math.max(a, v));
const MOVE = { duration: 240, easing: EASE },
  GROW = { duration: 420, easing: EASE };
const money = (b) => sizeText(b, { gb: true });

// ---------- what's open ----------
let pages = [],
  fileName = '',
  fileSize = 0,
  values = {},
  outline = [],
  meta_ = {},
  opened = false,
  password = null;
let zoom = 1,
  fitted = true,
  nowAt = 0,
  pickedPages = new Set(),
  pagesMode = false;
// what the file allows (an encrypted file's permissions); everything, for a file without them
const ALL = { encrypted: false, modify: true, copy: true, annotate: true, fill: true, assemble: true, writable: true };
let rights = ALL;
// a file that has been read but not yet shown: the shell shows it once Rust has adopted the file
let pending = null;
// the PDFs and pictures this window holds, by the Rust side's id: 0 is the open file
let docs = new Map();

const shell = createShell({
  name: 'DotAny PDF',
  icons: LUCIDE,
  words: WORDS,
  notices: () => import('./notices.generated.js'),
  dropTitle: '拖拽 PDF 至此',
  fileKind: 'PDF 文档',
  soundNote: '触发按钮操作时播放提示音。',
  licences: [['PDF.js', 'PDF 渲染引擎', 'Apache-2.0']],
  formats: FORMATS,
  // the card always opens on PDF, at the size the file has
  formatOf: () => {
    sizeKey = 'same';
    return 'pdf';
  },
  cantOpen: { damaged: '文件已损坏，或并非有效的 PDF 文档。' },
  failWords: { unreadable: '无法解析该文档的底层结构，未能保存修改。当前编辑仍完整保留在此窗口中。' },
  open: openPdf,
  show() {
    if (pending) {
      const p = pending;
      pending = null;
      hold(p);
    }
  },
  clear: goEmpty,
  dirty: () => dirty(),
  discard() {
    savedSnap = history[hAt];
  },
  estimate: (fmt) => estimateFor(fmt),
  saveRows: (box, fmt, again) => drawRows(box, fmt, again),
  saveNote: (fmt) => noteFor(fmt),
  save: (path, fmt) => saveTo(path, fmt),
  saved: (r, fmt) => afterSave(r, fmt),
  keys: (e) => keyDown(e),
  // in the page view, dropped PDFs and pictures are added; anything else opens as a file
  drop: (paths) => {
    const add = pagesMode && rights.assemble ? paths.filter((x) => /\.(pdf|jpe?g|png)$/i.test(x)) : [];
    if (add.length) addPaths(add);
    else shell.openRequest(paths[0]);
  },
  dropLabel: () => (pagesMode && rights.assemble ? '松开以添加文件' : '松开以打开文件'),
});
const { stage, head } = shell;
const say = (text) => shell.say(T(text));
// how tall the top is now: the bar, and the notice row when there is one
new ResizeObserver(() => {
  root.style.setProperty('--chrome-now', shell.chrome.offsetHeight + 'px');
}).observe(shell.chrome);

// ---------- the window: the pages, tools and dock inside the family's stage ----------
const list = el('div', 'pages');
list.id = 'pages';
stage.appendChild(list);
stage.tabIndex = -1;
const countEl = el('span', 'count dyn');
shell.metaEl.prepend(countEl);
const pagesBtn = shell.headButton('pagesBtn', 'grid', '页面缩略图'),
  tocBtn = shell.headButton('tocBtn', 'toc', '目录'),
  findBtn = shell.headButton('findBtn', 'search', '查找', '查找 · {Mod+F}'),
  infoBtn = shell.headButton('infoBtn', 'info', '信息');
[tocBtn, findBtn, pagesBtn, infoBtn].forEach((b) => b.setAttribute('aria-pressed', 'false'));

const barBtn = (id, n, tip, run) => {
  const b = el('button', 'tb');
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
const seg = el('div', 'tools'),
  segPill = el('i', 'tool-pl');
seg.setAttribute('role', 'toolbar');
seg.appendChild(segPill);
const TOOLS = [
  ['edit', 'edit', '编辑'],
  ['hl', 'highlight', '高亮'],
  ['brush', 'brush', '画笔'],
  ['text', 'type', '笔注'],
  ['sign', 'sign', '签名'],
  ['redact', 'redact', '密文涂黑'],
];
let tool = '';
const toolBtns = TOOLS.map(([k, ic, label]) => {
  const b = el('button', 'tool');
  b.type = 'button';
  b.dataset.k = k;
  b.id = 'tool-' + k;
  b.setAttribute('aria-pressed', 'false');
  b.setAttribute('aria-label', label);
  b.innerHTML = icon(ic) + '<span>' + label + '</span>';
  b.addEventListener('mousedown', (e) => e.preventDefault());
  b.addEventListener('click', () => {
    sound();
    setTool(tool === k ? '' : k);
  });
  seg.appendChild(b);
  return b;
});
const group = (items, cls) => {
  const g = el('div', 'grp ' + (cls || ''));
  g.append(...items);
  return g;
};
const undoBtn = barBtn('undo', 'undo', '撤销 · {Mod+Z}', () => undo()),
  redoBtn = barBtn('redo', 'redo', '重做 · {Mod+Y}', () => redo());
const pageNo = el('button', 'tb pct page-no read-only dyn', '1 / 1');
pageNo.type = 'button';
pageNo.id = 'pageNo';
pageNo.setAttribute('aria-label', '跳转至页码');
pageNo.addEventListener('mousedown', (e) => e.preventDefault());
const pageBox = el('span', 'fi page-in read-only'),
  pageIn = document.createElement('input');
pageIn.type = 'text';
pageIn.id = 'pageIn';
pageIn.inputMode = 'numeric';
pageIn.setAttribute('aria-label', '跳转至页码');
pageBox.appendChild(pageIn);
pageBox.hidden = true;
const pct = el('button', 'tb pct dyn', '100%');
pct.type = 'button';
pct.id = 'zoomFit';
pct.addEventListener('mousedown', (e) => e.preventDefault());
pct.addEventListener('click', () => {
  sound();
  if (fitted) zoomTo(4 / 3);
  else fitNow();
});
const zoomIsle = group(
  [
    barBtn('zoomOut', 'minus', '缩小', () => zoomBy(1 / 1.25)),
    pct,
    barBtn('zoomIn', 'plus', '放大', () => zoomBy(1.25)),
  ],
  'zoom-grp read-only'
);
const pickInfo = el('span', 'pick-info dyn');
const rotL = barBtn('pageRotL', 'rotateL', '向左旋转', () => rotatePicked(-90)),
  rotR = barBtn('pageRotR', 'rotateR', '向右旋转', () => rotatePicked(90)),
  delPages = barBtn('pageDelete', 'trash', '删除', () => deletePicked()),
  extract = barBtn('pageExtract', 'extract', '另存为新文档', () => openExtract());
const addTile = el('div', 'add-tile'),
  addBlank = el('button', 'add-blank'),
  addFile = el('button', 'btn soft add-file dyn', '从文件导入页面');
addBlank.type = 'button';
addBlank.id = 'pageAdd';
addBlank.innerHTML = icon('plus') + '<span class="dyn">插入空白页</span>';
addFile.type = 'button';
addFile.id = 'pageAddFile';
[addBlank, addFile].forEach((b) => b.addEventListener('mousedown', (e) => e.preventDefault()));
addBlank.addEventListener('click', () => {
  sound();
  const last = pages[pages.length - 1];
  insert([makePage({ blank: true, sid: -1 }, last.w, last.h)]);
});
addFile.addEventListener('click', async () => {
  sound();
  addFiles(await own.pickSources(T('PDF 与图像')));
});
addTile.append(addBlank, addFile);
const pagesDone = el('button', 'btn primary', '完成');
pagesDone.type = 'button';
pagesDone.id = 'pagesDone';
pagesDone.addEventListener('mousedown', (e) => e.preventDefault());
pagesDone.addEventListener('click', () => {
  sound();
  leavePages();
});
const named = (b, text) => {
  b.classList.add('has-lab');
  b.appendChild(el('span', 'stack-lab', text));
  return b;
};
[
  [rotL, '左转'],
  [rotR, '右转'],
  [delPages, '删除'],
  [extract, '提取页面'],
].forEach(([b, w]) => named(b, w));
const pagesAll = el('button', 'btn soft', '全选');
pagesAll.type = 'button';
pagesAll.id = 'pagesAll';
pagesAll.addEventListener('mousedown', (e) => e.preventDefault());
pagesAll.addEventListener('click', () => {
  sound();
  pickedPages = pickedPages.size === pages.length ? new Set() : new Set(pages);
  syncPicked();
});
const pagesHead = el('div', 'pages-head pages-only');
pagesHead.append(pickInfo, el('i', 'pages-gap'), pagesAll, pagesDone);
const pill = el('div', 'pill'),
  pillRow = el('div', 'pill-row'),
  sepTools = el('i', 'sep sep-tools');
pillRow.append(
  seg,
  sepTools,
  group([undoBtn, redoBtn]),
  el('i', 'sep read-only'),
  pageNo,
  pageBox,
  el('i', 'sep read-only'),
  zoomIsle,
  el('i', 'sep pages-only'),
  group([rotL, rotR, delPages, extract], 'pages-only')
);
pill.append(pagesHead, pillRow);
const navPrev = barBtn('prevPage', 'prev', '上一页', () => goPage(nowAt - 1)),
  navNext = barBtn('nextPage', 'next', '下一页', () => goPage(nowAt + 1));
navPrev.classList.add('pic-nav');
navNext.classList.add('pic-nav');
const dockRow = el('div', 'dock-row');
dockRow.append(navPrev, pill, navNext);
const rail = el('div', 'rail'),
  side = el('aside', 'side right'),
  tocPanel = el('aside', 'side left'),
  ring = el('div', 'ring');
side.id = 'sidePanel';
tocPanel.id = 'tocPanel';
const selBar = el('div', 'sel-bar');
selBar.id = 'selBar';
shell.place(rail, tocPanel, side, dockRow, ring, selBar);
shell.translates(dockRow, rail);

// ---------- pages: each sits at its own size, and everything on it scales together ----------
const svgNS = 'http://www.w3.org/2000/svg',
  THUMB = 176;
const zFor = (pg) => (pagesMode ? THUMB / (((pg.rot % 360) + 360) % 180 ? pg.h : pg.w) : zoom);
function place(pg, z = zFor(pg)) {
  const r = ((pg.rot % 360) + 360) % 360,
    sw = r % 180 ? pg.h : pg.w,
    sh = r % 180 ? pg.w : pg.h;
  pg.el.style.width = sw * z + 'px';
  pg.el.style.height = sh * z + 'px';
  pg.inner.style.transform = turned(pg, z);
}
function turned(pg, z, rot = pg.rot) {
  const r = ((rot % 360) + 360) % 360,
    [x, y] = r === 0 ? [0, 0] : r === 90 ? [pg.h * z, 0] : r === 180 ? [pg.w * z, pg.h * z] : [0, pg.w * z];
  return `translate(${x}px, ${y}px) rotate(${rot}deg) scale(${z})`;
}
const seenIO = new IntersectionObserver(
  (list_) => {
    list_.forEach((e) => {
      e.target.__pg.seen = e.isIntersecting;
    });
    pump();
  },
  { root: stage, rootMargin: '700px' }
);
// src: { doc, n, sid } for a PDF's page, { img, sid } for a picture, { blank } for an empty page; vt is PDF.js's viewport
// transform, the page's own coordinates for the Rust side
function makePage(src, w, h, vt) {
  const pg = {
    src,
    w,
    h,
    vt: vt || [1, 0, 0, -1, 0, h],
    rot: 0,
    marks: [],
    el: el('div', 'pg'),
    inner: el('div', 'pg-in'),
    canvas: null,
    canvasZ: 0,
    canvasKey: '',
    seen: false,
    tlDone: false,
    fields: [],
    thumbUrl: '',
    thumbKey: '',
    wasZ: 0,
    wasDay: false,
    failed: '',
  };
  pg.el.__pg = pg;
  pg.inner.style.width = w + 'px';
  pg.inner.style.height = h + 'px';
  pg.tl = el('div', 'textLayer');
  pg.tl.style.setProperty('--scale-factor', 1);
  pg.svg = document.createElementNS(svgNS, 'svg');
  pg.svg.setAttribute('class', 'marks');
  pg.svg.setAttribute('viewBox', `0 0 ${w} ${h}`);
  pg.flds = el('div', 'flds');
  pg.no = el('span', 'pg-no');
  pg.inner.append(pg.tl, pg.svg, pg.flds);
  pg.el.append(pg.inner, pg.no);
  seenIO.observe(pg.el);
  // a file opened right after this one ends it: what its fields were is no longer asked for (the document is destroyed)
  if (src.doc)
    readFields(pg).catch((e) => {
      if (!pg.gone) throw e;
    });
  return pg;
}
const nightDim = () => isNight() && dimPaper;
// off unless turned on: a PDF's page is the file's own, shown as it is; dimming it is a reading aid
let dimPaper = store.get('dotany-pdf-dim') === 'on';
const isNight = () => {
  const t = root.dataset.theme;
  return t === 'dark' || (t !== 'light' && matchMedia('(prefers-color-scheme: dark)').matches);
};
// only one page is drawn at a time, nearest first; the old drawing stays until the new one is ready, so nothing flashes
// a new file starts a new round: a drawing still waiting on the last file's pages never holds up this one's
let pumping = false,
  pumpGen = 0;
const restartPump = () => {
  pumpGen++;
  pumping = false;
};
const needZ = (pg) => Math.min(zFor(pg) * (devicePixelRatio || 1), 4);
const paperKey = () => (nightDim() ? 'dim' : 'day');
function wants(pg) {
  const key = paperKey();
  return pg.failed !== key && (!pg.canvas || pg.canvasKey !== key || pg.canvasZ < needZ(pg) * 0.9);
}
// A drawn page holds width × height × 4 bytes of graphics memory (about 34 MB at 150 % on a 200 % screen; on a laptop
// with integrated graphics that is ordinary memory), so only the pages near the view stay drawn: the ones on screen, and
// the nearest of the rest up to KEEP_PX. A page further away gives its canvas back, and PDF.js its pictures and drawing
// list, and its words (see buildText), and keeps a small picture of itself (a few kB) to show for the moment it takes
// to be drawn sharp again.
const KEEP_PX = 16e6,
  THUMB_PX = 280;
const drawn = new Set(),
  thumbed = new Set(),
  texted = new Set();
function nearby() {
  const box = stage.getBoundingClientRect(),
    mid = (box.top + box.bottom) / 2,
    out = [];
  let px = 0;
  pages
    .filter((p) => p.seen)
    .map((p) => {
      const r = p.el.getBoundingClientRect();
      return { p, r, d: Math.max(r.top - mid, mid - r.bottom, 0) };
    })
    .sort((a, b) => a.d - b.d)
    .forEach(({ p, r }) => {
      const z = needZ(p),
        n = p.w * p.h * z * z;
      if ((r.bottom > box.top && r.top < box.bottom) || px + n <= KEEP_PX) {
        out.push(p);
        px += n;
      }
    });
  return out;
}
async function pump() {
  if (pumping) return;
  pumping = true;
  const gen = pumpGen;
  try {
    while (gen === pumpGen) {
      const near = nearby(),
        keep = new Set(near);
      for (const p of drawn) if (!keep.has(p)) letGo(p);
      for (const p of texted) if (!keep.has(p) && !p.seen) dropText(p);
      const todo = near.filter(wants);
      if (!todo.length) break;
      // a page that can't be drawn (a damaged file) is given up on after a while, so the others still are
      try {
        await Promise.race([draw(todo[0]), new Promise((_, no) => setTimeout(() => no(new Error('slow')), 20000))]);
      } catch {
        todo[0].failed = paperKey();
      }
    }
  } finally {
    if (gen === pumpGen) pumping = false;
  }
}
async function draw(pg) {
  const z = needZ(pg),
    key = paperKey(),
    c = el('canvas', 'pg-c');
  c.width = Math.max(1, Math.round(pg.w * z));
  c.height = Math.max(1, Math.round(pg.h * z));
  if (pg.src.blank) {
    const g = c.getContext('2d');
    g.fillStyle = key === 'dim' ? '#232323' : '#FFFFFF';
    g.fillRect(0, 0, c.width, c.height);
  } else if (pg.src.img) {
    const g = c.getContext('2d');
    g.drawImage(pg.src.img, 0, 0, c.width, c.height);
  } else {
    const page = await pg.src.doc.getPage(pg.src.n),
      vp = page.getViewport({ scale: z });
    // annotations are drawn with the page (highlights and notes other programs made); form fields are the window's own
    await page.render({
      canvasContext: c.getContext('2d'),
      viewport: vp,
      annotationMode: pdfjs.AnnotationMode.ENABLE_FORMS,
      // PDF.js gives the page these colours through the canvas's filter, which WebKit's canvas doesn't have: on the
      // Mac the page is drawn as it is and given the same colours afterwards (nightPaper)
      pageColors: key === 'dim' && !mac ? NIGHT_PAPER : null,
    }).promise;
    if (key === 'dim' && mac) nightPaper(c);
    if (!pg.tlDone) await buildText(pg, page);
  }
  // another file was opened while this page was being drawn: its drawing is dropped, the page and its file are gone
  if (pg.gone) {
    c.width = c.height = 0;
    return;
  }
  // the fade is for a page seen for the first time; one coming back over its small picture just sharpens
  const first = !pg.canvas && !pg.inner.style.backgroundImage;
  if (pg.canvas) {
    const old = pg.canvas;
    old.replaceWith(c);
    old.width = old.height = 0;
  } else pg.inner.prepend(c);
  pg.canvas = c;
  pg.canvasZ = z;
  pg.canvasKey = pg.drawnKey = key;
  pg.failed = '';
  drawn.add(pg);
  standIn(pg);
  if (first && !reduce) c.animate([{ opacity: 0 }, { opacity: 1 }], MOVE);
}
const NIGHT_PAPER = { background: '#232323', foreground: '#E6E6E6' };
// PDF.js's page colours (DOMFilterFactory.addHCMFilter, as the browser's filter tables run it), worked on the pixels for
// WebKit: each colour to its linear light, to grey (Rec. 709), and to one of six steps from the words' colour to the paper's
let nightSteps = null;
function nightPaper(c) {
  if (!nightSteps) {
    const hex = (s) => [1, 3, 5].map((i) => parseInt(s.slice(i, i + 2), 16)),
      fg = hex(NIGHT_PAPER.foreground),
      bg = hex(NIGHT_PAPER.background),
      lin = new Float64Array(256),
      step = new Uint8Array(256 * 3);
    for (let i = 0; i < 256; i++) {
      const x = i / 255;
      lin[i] = Math.trunc(255 * (x <= 0.03928 ? x / 12.92 : ((x + 0.055) / 1.055) ** 2.4));
      const k = Math.min(Math.floor((i * 6) / 255), 5);
      for (let j = 0; j < 3; j++) step[i * 3 + j] = Math.round(fg[j] + (k / 5) * (bg[j] - fg[j]));
    }
    nightSteps = { lin, step };
  }
  const { lin, step } = nightSteps,
    g = c.getContext('2d'),
    im = g.getImageData(0, 0, c.width, c.height),
    d = im.data;
  for (let i = 0; i < d.length; i += 4) {
    const s = Math.min(255, Math.round(0.2126 * lin[d[i]] + 0.7152 * lin[d[i + 1]] + 0.0722 * lin[d[i + 2]])) * 3;
    d[i] = step[s];
    d[i + 1] = step[s + 1];
    d[i + 2] = step[s + 2];
  }
  g.putImageData(im, 0, 0);
}
// a page far from the view: its small picture is kept, its canvas shrunk to nothing so the graphics memory goes at once
function letGo(pg) {
  drawn.delete(pg);
  const c = pg.canvas;
  if (!c) return;
  // what it was drawn in: canvasKey is wiped when the paper's colours change, to ask for a new drawing
  if (!pg.thumbUrl || pg.thumbKey !== pg.drawnKey) keepThumb(pg, c, pg.drawnKey);
  pg.wasZ = pg.canvasZ;
  pg.wasDay = pg.drawnKey === 'day';
  c.width = c.height = 0;
  c.remove();
  pg.canvas = null;
  pg.canvasZ = 0;
  pg.canvasKey = '';
  standIn(pg);
  dropText(pg);
  if (pg.src.doc)
    pg.src.doc.getPage(pg.src.n).then(
      (page) => page.cleanup(),
      () => {}
    );
}
function keepThumb(pg, c, key) {
  const k = Math.min(1, THUMB_PX / Math.max(c.width, c.height)),
    t = document.createElement('canvas');
  t.width = Math.max(1, Math.round(c.width * k));
  t.height = Math.max(1, Math.round(c.height * k));
  t.getContext('2d').drawImage(c, 0, 0, t.width, t.height);
  t.toBlob(
    (b) => {
      t.width = t.height = 0;
      if (!b || pg.gone) return;
      if (pg.thumbUrl) URL.revokeObjectURL(pg.thumbUrl);
      pg.thumbUrl = URL.createObjectURL(b);
      pg.thumbKey = key;
      thumbed.add(pg);
      standIn(pg);
    },
    'image/webp',
    0.8
  );
}
// the small picture shows only while the page isn't drawn, and only in the paper's colours of the moment
function standIn(pg) {
  const show = !pg.canvas && pg.thumbUrl && pg.thumbKey === paperKey() ? `url("${pg.thumbUrl}")` : '';
  if (pg.inner.style.backgroundImage === show) return;
  pg.inner.style.backgroundImage = show;
  pg.inner.style.backgroundSize = show ? '100% 100%' : '';
}
// another file: every canvas and small picture of the last one goes now, not when the garbage is next collected
function forgetPages() {
  for (const p of drawn)
    if (p.canvas) {
      p.canvas.width = p.canvas.height = 0;
    }
  for (const p of thumbed) {
    p.gone = true;
    URL.revokeObjectURL(p.thumbUrl);
  }
  for (const p of pages) p.gone = true;
  for (const s of history) for (const p of s.order) p.gone = true;
  drawn.clear();
  thumbed.clear();
  texted.clear();
  seenIO.disconnect();
}
// The words over a page, for selecting, copying and find. A page's words are a few thousand elements (about 2 MB), so a
// page far from the view lets them go too, unless the selection runs through them, and builds them again when it's back.
async function buildText(pg, page) {
  if (pg.tlDone) return;
  pg.tlDone = true;
  page = page || (await pg.src.doc.getPage(pg.src.n));
  const t = new pdfjs.TextLayer({
    textContentSource: page.streamTextContent({ includeMarkedContent: true, disableNormalization: true }),
    container: pg.tl,
    viewport: page.getViewport({ scale: 1 }),
  });
  await t.render();
  pg.tl.appendChild(el('div', 'endOfContent'));
  pg.tl.style.width = pg.w + 'px';
  pg.tl.style.height = pg.h + 'px';
  if (!pg.tlHooked) {
    pg.tlHooked = true;
    pg.tl.addEventListener('mousedown', () => pg.tl.classList.add('selecting'));
    addEventListener('pointerup', () => pg.tl.classList.remove('selecting'));
  }
  texted.add(pg);
  // words a replaced paragraph covers stay unselectable when the words are built again
  syncGone(pg);
}
const inSelection = (pg) => {
  const s = getSelection();
  return s.rangeCount > 0 && !s.isCollapsed && s.getRangeAt(0).intersectsNode(pg.tl);
};
function dropText(pg) {
  if (!texted.has(pg) || inSelection(pg)) return;
  texted.delete(pg);
  pg.tlDone = false;
  pg.hadCuts = false;
  pg.tl.replaceChildren();
  pg.tl.classList.remove('selecting');
}
function layoutAll() {
  pages.forEach((p, i) => {
    place(p);
    p.no.textContent = i + 1;
  });
  clearTimeout(layoutAll.t);
  layoutAll.t = setTimeout(pump, 160);
}
function setList() {
  list.replaceChildren(...pages.map((p) => p.el), addTile);
  layoutAll();
}

// ---------- zoom: around the pointer, easing, and sharp again when it settles ----------
const gutter = () => stage.offsetWidth - stage.clientWidth;
const padL = () =>
  tocPanel.classList.contains('on') ? 280 : root.classList.contains('has-rail') && !pagesMode ? 86 : 0;
const padR = () => (side.classList.contains('on') ? Math.max(0, 280 - gutter()) : 0);
function fitZ() {
  const big = Math.max(...pages.map((p) => (((p.rot % 360) + 360) % 180 ? p.h : p.w)), 1);
  return clamp((stage.clientWidth - padL() - padR() - 48) / big, 0.25, 1.5);
}
const syncPct = () => {
  pct.textContent = Math.round(zoom * 0.75 * 100) + '%';
};
let zoomRaf = 0;
// `now`: at once, with no easing (the Mac's pinch, which the pages follow as the fingers move)
function zoomTo(z, cx = stage.clientWidth / 2, cy = stage.clientHeight / 2, keepFit = false, now = false) {
  z = clamp(z, 0.25, 6);
  fitted = keepFit;
  cancelAnimationFrame(zoomRaf);
  const box = stage.getBoundingClientRect(),
    x = box.left + cx,
    y = box.top + cy;
  const under =
    pages.find((p) => {
      const r = p.el.getBoundingClientRect();
      return y >= r.top - 8 && y <= r.bottom + 8;
    }) || pages[nowAt];
  if (!under) return;
  const r0 = under.el.getBoundingClientRect(),
    ax = (x - r0.left) / r0.width,
    ay = (y - r0.top) / r0.height,
    from = zoom,
    t0 = performance.now(),
    dur = reduce || now ? 0 : 240;
  const frame = (t) => {
    const k = dur ? Math.min(1, (t - t0) / dur) : 1,
      e = 1 - Math.pow(1 - k, 3);
    zoom = from * Math.pow(z / from, e);
    pages.forEach((p) => place(p));
    const r = under.el.getBoundingClientRect();
    stage.scrollTop += r.top + ay * r.height - y;
    stage.scrollLeft += r.left + ax * r.width - x;
    syncPct();
    if (k < 1) zoomRaf = requestAnimationFrame(frame);
    else {
      layoutAll();
    }
  };
  if (now) frame(t0);
  else zoomRaf = requestAnimationFrame(frame);
}
const zoomBy = (k) => zoomTo(zoom * k);
function fitNow() {
  zoomTo(fitZ(), undefined, undefined, true);
}
stage.addEventListener(
  'wheel',
  (e) => {
    if (!(e.ctrlKey || e.metaKey) || pagesMode || !pages.length) return;
    e.preventDefault();
    const b = stage.getBoundingClientRect();
    zoomTo(zoom * Math.exp(-e.deltaY * 0.002), e.clientX - b.left, e.clientY - b.top);
  },
  { passive: false }
);
// The Mac's trackpad (gestures.js). Two fingers scroll the pages: that is the stage's own scrolling, with the system's
// momentum and WebKit's bounce at its ends, as in Preview; nothing here touches it. The pinch zooms about the fingers, the pages
// following them; the smart zoom (a double tap with two fingers) fits the page's width, and from there goes back to
// the zoom it came from.
pinch(stage, {
  zoom: (k, x, y) => {
    if (pagesMode || !pages.length) return;
    const b = stage.getBoundingClientRect();
    zoomTo(zoom * k, x - b.left, y - b.top, false, true);
  },
});
let beforeFit = 0;
if (mac)
  hear('smart-zoom', ({ x, y }) => {
    if (pagesMode || !pages.length) return;
    const b = stage.getBoundingClientRect();
    if (!fitted) {
      beforeFit = zoom;
      fitNow();
    } else if (beforeFit) {
      zoomTo(beforeFit, x - b.left, y - b.top);
      beforeFit = 0;
    } else zoomTo(zoom * 2, x - b.left, y - b.top);
  });
function syncPads() {
  syncPadsOnly();
  if (fitted && pages.length && !pagesMode) fitNow();
}
new ResizeObserver(() => {
  const on = innerWidth > 720;
  if (on !== root.classList.contains('has-rail')) placeTools();
  if (fitted && pages.length && !pagesMode) {
    zoom = fitZ();
    pages.forEach((p) => place(p));
    syncPct();
    layoutAll();
  }
  syncSeg();
}).observe(shell.app);
function placeTools() {
  const on = innerWidth > 720;
  root.classList.toggle('has-rail', on);
  if (on) rail.appendChild(seg);
  else pillRow.prepend(seg);
  syncPads();
  requestAnimationFrame(syncSeg);
}
let placedAt = '';
function syncSeg() {
  const on = toolBtns.find((b) => b.dataset.k === tool);
  segPill.style.opacity = on ? 1 : 0;
  const at = on ? [on.offsetLeft, on.offsetTop, on.offsetWidth, on.offsetHeight].join() : '';
  if (on && at !== placedAt) {
    placedAt = at;
    segPill.style.transform = `translate(${on.offsetLeft}px, ${on.offsetTop}px)`;
    segPill.style.width = on.offsetWidth + 'px';
    segPill.style.height = on.offsetHeight + 'px';
  }
}

// ---------- where you are ----------
const syncPageNo = () => {
  pageNo.textContent = `${nowAt + 1} / ${pages.length}`;
};
function whereAmI() {
  if (!pages.length || pagesMode) return;
  const mid = stage.getBoundingClientRect().top + stage.clientHeight / 2;
  let best = 0,
    gap = Infinity;
  pages.forEach((p, i) => {
    const r = p.el.getBoundingClientRect(),
      d = mid < r.top ? r.top - mid : mid > r.bottom ? mid - r.bottom : 0;
    if (d < gap) {
      gap = d;
      best = i;
    }
  });
  if (best !== nowAt) {
    nowAt = best;
    syncPageNo();
    markToc();
  }
  rememberSoon();
}
stage.addEventListener(
  'scroll',
  () => {
    whereAmI();
    hideSelBar();
    pump();
  },
  { passive: true }
);
let scrollRaf = 0;
function glideTo(top) {
  cancelAnimationFrame(scrollRaf);
  if (reduce) {
    stage.scrollTop = top;
    return;
  }
  const from = stage.scrollTop,
    t0 = performance.now();
  const frame = (t) => {
    const k = Math.min(1, (t - t0) / 420),
      e = 1 - Math.pow(1 - k, 3);
    stage.scrollTop = from + (top - from) * e;
    if (k < 1) scrollRaf = requestAnimationFrame(frame);
  };
  scrollRaf = requestAnimationFrame(frame);
}
stage.addEventListener('wheel', () => cancelAnimationFrame(scrollRaf), { passive: true });
const topGap = () => (shell.chrome.offsetHeight || 53) + 16;
function goPage(i, y = 0) {
  i = clamp(i, 0, pages.length - 1);
  const p = pages[i];
  if (!p) return;
  nowAt = i;
  syncPageNo();
  markToc();
  glideTo(p.el.offsetTop - topGap() + y * zFor(p));
}
pageNo.addEventListener('click', () => {
  sound();
  pageNo.hidden = true;
  pageBox.hidden = false;
  pageIn.value = nowAt + 1;
  pageIn.focus();
  pageIn.select();
});
const closePageIn = () => {
  pageBox.hidden = true;
  pageNo.hidden = false;
};
pageIn.addEventListener('blur', closePageIn);
pageIn.addEventListener('keydown', (e) => {
  e.stopPropagation();
  if (e.key === 'Enter') {
    const n = parseInt(pageIn.value, 10);
    if (n) goPage(n - 1);
    pageIn.blur();
  } else if (e.key === 'Escape') pageIn.blur();
});
inkfall(pageIn);
// a file reopens where you left it
const posKey = () => 'dotany-pdf-pos:' + fileName + ':' + fileSize;
const rememberSoon = () => {
  clearTimeout(rememberSoon.t);
  rememberSoon.t = setTimeout(() => {
    if (opened && store.get(posKey()) !== String(nowAt)) store.set(posKey(), String(nowAt));
  }, 500);
};

// ---------- forms: real fields, the family's caret ----------
const fieldKey = (pg, name) => pg.src.sid + '|' + name;
async function readFields(pg) {
  const page = await pg.src.doc.getPage(pg.src.n),
    vp = page.getViewport({ scale: 1 }),
    notes = await page.getAnnotations();
  for (const a of notes) {
    const [x1, y1, x2, y2] = vp.convertToViewportRectangle(a.rect),
      x = Math.min(x1, x2),
      y = Math.min(y1, y2),
      w = Math.abs(x2 - x1),
      h = Math.abs(y2 - y1);
    if (a.subtype === 'Link' && (a.url || a.dest)) {
      const l = el('a', 'lnk');
      l.style.cssText = `left:${x}px;top:${y}px;width:${w}px;height:${h}px`;
      l.href = '#';
      if (a.url) l.title = a.url;
      // a web address is offered, not followed: the bar over the link says where it goes and Open hands it to the
      // system's browser (http and https only; any other address only shows where it points). A link inside the file
      // goes to its page
      const web = a.url && isWebAddress(a.url);
      if (web) l.classList.add('web');
      l.addEventListener('click', async (e) => {
        e.preventDefault();
        if (a.url) {
          if (web) offerLink(a.url, l);
          return;
        }
        const d = typeof a.dest === 'string' ? await pg.src.doc.getDestination(a.dest) : a.dest;
        if (!d) return;
        const n = (await pg.src.doc.getPageIndex(d[0])) + 1,
          i = pages.findIndex((p) => p.src.doc === pg.src.doc && p.src.n === n);
        if (i >= 0) goPage(i);
      });
      pg.flds.appendChild(l);
      continue;
    }
    if (a.subtype !== 'Widget' || !a.fieldName) continue;
    const key = fieldKey(pg, a.fieldName),
      info = { key, kind: '', x, y, w, h, a };
    const lockedField = () => a.readOnly || !rights.fill;
    if (a.fieldType === 'Tx') {
      const box = el('span', 'fld'),
        i = document.createElement(a.multiLine ? 'textarea' : 'input');
      if (!a.multiLine) i.type = a.password ? 'password' : 'text';
      i.id = 'fld-' + a.id;
      i.spellcheck = false;
      i.autocomplete = 'off';
      i.setAttribute('aria-label', a.alternativeText || a.fieldName);
      i.value = key in values ? values[key] : a.fieldValue || '';
      box.style.cssText = `left:${x}px;top:${y}px;width:${w}px;height:${h}px;font-size:${Math.min(h * 0.62, 12)}px`;
      box.appendChild(i);
      if (a.maxLen) i.maxLength = a.maxLen;
      i.addEventListener('change', () => {
        values[key] = i.value;
        commit();
      });
      i.addEventListener('keydown', (e) => e.stopPropagation());
      pg.flds.appendChild(box);
      info.kind = 'text';
      info.value = () => i.value;
      info.sync = () => {
        i.readOnly = lockedField();
      };
      pg.fields.push({
        key,
        set: (v) => {
          i.value = v == null ? a.fieldValue || '' : v;
        },
      });
      if (!a.multiLine) inkfall(i);
    } else if (a.fieldType === 'Btn' && (a.checkBox || a.radioButton)) {
      const b = el('button', 'fld-c' + (a.radioButton ? ' radio' : ''));
      b.type = 'button';
      b.id = 'fld-' + a.id;
      b.setAttribute('role', a.radioButton ? 'radio' : 'checkbox');
      b.setAttribute('aria-label', a.alternativeText || a.fieldName);
      const was = a.radioButton
        ? a.fieldValue === a.buttonValue
          ? a.buttonValue
          : null
        : !!(a.fieldValue && a.fieldValue !== 'Off');
      const now = () => (key in values ? values[key] : was);
      const isOn = () => (a.radioButton ? now() === a.buttonValue : !!now());
      b.style.cssText = `left:${x}px;top:${y}px;width:${w}px;height:${h}px`;
      b.innerHTML =
        '<svg viewBox="0 0 24 24" aria-hidden="true"><path class="tick" pathLength="1" d="M4 12l5 5L20 6"/></svg>';
      b.addEventListener('mousedown', (e) => e.preventDefault());
      b.addEventListener('click', () => {
        if (lockedField()) return;
        sound();
        values[key] = a.radioButton ? a.buttonValue : !isOn();
        pages.forEach((p) =>
          p.fields.forEach((f) => {
            if (f.key === key) f.paint();
          })
        );
        commit();
      });
      const paint = () => b.setAttribute('aria-checked', String(isOn()));
      paint();
      pg.flds.appendChild(b);
      info.kind = 'check';
      info.value = isOn;
      info.sync = () => {
        b.disabled = lockedField();
      };
      pg.fields.push({ key, set: paint, paint });
    } else if (a.fieldType === 'Ch') {
      const opts = (a.options || []).map((o) => ({ value: o.exportValue, label: o.displayValue || o.exportValue }));
      const first = Array.isArray(a.fieldValue) ? a.fieldValue[0] : a.fieldValue;
      const now = () => (key in values ? values[key] : first || '');
      const box = el('button', 'fld fld-ch');
      box.type = 'button';
      box.id = 'fld-' + a.id;
      box.setAttribute('aria-label', a.alternativeText || a.fieldName);
      box.dataset.file = '';
      box.style.cssText = `left:${x}px;top:${y}px;width:${w}px;height:${h}px;font-size:${Math.min(h * 0.62, 12)}px`;
      const paint = () => {
        const o = opts.find((p) => p.value === now());
        box.textContent = o ? o.label : now();
      };
      paint();
      box.addEventListener('mousedown', (e) => e.preventDefault());
      box.addEventListener('click', () => {
        if (lockedField()) return;
        sound();
        shell.openPop(
          box,
          opts.map((o) => ({ value: o.value, label: o.label, raw: true, on: o.value === now() })),
          (v) => {
            values[key] = v;
            pages.forEach((p) =>
              p.fields.forEach((f) => {
                if (f.key === key) f.paint();
              })
            );
            commit();
          }
        );
      });
      pg.flds.appendChild(box);
      info.kind = 'text';
      info.value = () => box.textContent;
      info.sync = () => {
        box.disabled = lockedField();
      };
      pg.fields.push({ key, set: paint, paint });
    }
    if (info.kind) {
      (pg.fieldsInfo || (pg.fieldsInfo = [])).push(info);
      info.sync();
    }
  }
}
// ---------- marks: everything is kept in the page's own coordinates ----------
const toPage = (pg, cx, cy) => {
  const m = pg.svg.getScreenCTM().inverse(),
    p = new DOMPoint(cx, cy).matrixTransform(m);
  return { x: p.x, y: p.y };
};
const pageAt = (e) => {
  const c = e.target.closest ? e.target.closest('.pg') : null;
  return c ? c.__pg : null;
};
const INK = '#151413';
const HL = { red: 'var(--c-red)', blue: 'var(--c-blue)', green: 'var(--c-green)', gray: 'var(--c-gray)' };
// the same four, as the day's colours, for the file: a highlight on paper is the day's
const HL_HEX = { red: '#B3261E', blue: '#1F4FA8', green: '#2C6E49', gray: '#75716B' };
const smooth = (a) => {
  if (a.length < 4) return `M${a[0]} ${a[1]}h.01`;
  let d = `M${a[0]} ${a[1]}`;
  for (let i = 2; i < a.length - 2; i += 2)
    d += `Q${a[i]} ${a[i + 1]} ${(a[i] + a[i + 2]) / 2} ${(a[i + 1] + a[i + 3]) / 2}`;
  return d + `L${a[a.length - 2]} ${a[a.length - 1]}`;
};
const svgEl = (t, attrs, parent) => {
  const e = document.createElementNS(svgNS, t);
  for (const k in attrs) e.setAttribute(k, attrs[k]);
  if (parent) parent.appendChild(e);
  return e;
};
function drawMarks(pg) {
  pg.svg.textContent = '';
  for (const m of pg.marks) {
    if (m === editing) continue;
    if (m.t === 'edit') {
      svgEl('rect', { x: m.ox - 1, y: m.oy - 1, width: m.ow + 2, height: m.oh + 3, class: 'm-cover' }, pg.svg);
      if (m !== openMark) {
        const f = svgEl('foreignObject', { x: m.x, y: m.y, width: m.w + 2, height: m.h + m.lh }, pg.svg),
          d = el('div', 'm-edit');
        d.textContent = m.text;
        Object.assign(d.style, {
          fontSize: m.size + 'px',
          lineHeight: m.lh + 'px',
          fontFamily: blockFont(m),
          fontWeight: m.bold ? 700 : 400,
          color: m.color,
          textAlign: m.justify ? 'justify' : 'left',
        });
        f.appendChild(d);
      }
      continue;
    }
    if (m.t === 'hl')
      m.rects.forEach((r) =>
        svgEl(
          'rect',
          { x: r[0], y: r[1], width: r[2], height: r[3], rx: 1, class: 'm-hl', style: `fill:${HL[m.color]}` },
          pg.svg
        )
      );
    else if (m.t === 'brush')
      svgEl(
        'path',
        {
          d: smooth(m.pts),
          fill: 'none',
          stroke: m.color,
          'stroke-width': m.size,
          'stroke-linecap': 'round',
          'stroke-linejoin': 'round',
          'data-ink': m.color,
        },
        pg.svg
      );
    else if (m.t === 'text') {
      const t = svgEl(
        'text',
        {
          x: m.x,
          y: m.y + m.size * 0.6,
          fill: m.color,
          'font-size': m.size,
          'font-weight': m.bold ? 700 : 400,
          class: 'm-text',
          'data-ink': m.color,
        },
        pg.svg
      );
      t.textContent = m.words;
    } else if (m.t === 'redact')
      svgEl('rect', { x: m.x, y: m.y, width: m.w, height: m.h, fill: INK, class: 'm-redact' }, pg.svg);
    else if (m.t === 'sign') {
      const s = svgEl('svg', { x: m.x, y: m.y, width: m.w, height: m.h, viewBox: `0 0 ${m.sig.w} ${m.sig.h}` }, pg.svg);
      if (m.sig.img) svgEl('image', { href: m.sig.img, width: m.sig.w, height: m.sig.h }, s);
      else
        m.sig.strokes.forEach((a) =>
          svgEl(
            'path',
            {
              d: smooth(a),
              fill: 'none',
              stroke: INK,
              'stroke-width': 2.4,
              'stroke-linecap': 'round',
              'stroke-linejoin': 'round',
              'data-ink': INK,
            },
            s
          )
        );
    }
  }
  paintSel(pg);
  syncGone(pg);
  if (hoverBlock && hoverBlock.pg === pg && hoverBlock.src !== openMark) {
    const k = hoverBlock.src;
    svgEl('rect', { x: k.x - 3, y: k.y - 2, width: k.w + 6, height: k.h + 4, rx: 2, class: 'm-pick' }, pg.svg);
  }
  const hits = findHits.filter((h) => h.pg === pg);
  hits.forEach((h) =>
    h.rects.forEach((r) =>
      svgEl(
        'rect',
        { x: r[0], y: r[1], width: r[2], height: r[3], rx: 1, class: h === findHits[findAt] ? 'm-hit cur' : 'm-hit' },
        pg.svg
      )
    )
  );
  if (picked && picked.pg === pg) {
    const b = boxOf(picked.m);
    svgEl('rect', { x: b.x - 2, y: b.y - 2, width: b.w + 4, height: b.h + 4, rx: 2, class: 'm-pick' }, pg.svg);
    if (picked.m.t === 'sign' || picked.m.t === 'redact')
      svgEl('rect', { x: b.x + b.w - 3, y: b.y + b.h - 3, width: 7, height: 7, rx: 1, class: 'm-grip' }, pg.svg);
  }
}
const textMeter = document.createElement('canvas').getContext('2d');
const fontText = () => getComputedStyle(root).getPropertyValue('--font-text');
function boxOf(m) {
  if (m.t === 'text') {
    textMeter.font = `${m.bold ? 700 : 400} ${m.size}px ${fontText()}`;
    return { x: m.x, y: m.y, w: textMeter.measureText(m.words).width, h: m.size * 1.2 };
  }
  return { x: m.x, y: m.y, w: m.w, h: m.h };
}
function markAt(pg, p, kinds) {
  for (let i = pg.marks.length - 1; i >= 0; i--) {
    const m = pg.marks[i];
    if (!kinds.includes(m.t)) continue;
    const b = boxOf(m);
    if (p.x >= b.x - 3 && p.x <= b.x + b.w + 3 && p.y >= b.y - 3 && p.y <= b.y + b.h + 3) return m;
  }
  return null;
}
const hlAt = (pg, p) =>
  pg.marks.find(
    (m) => m.t === 'hl' && m.rects.some((r) => p.x >= r[0] && p.x <= r[0] + r[2] && p.y >= r[1] && p.y <= r[1] + r[3])
  );

let picked = null,
  editing = null,
  editingPg = null,
  drag = null;
// a PDF only knows where each word sits, so paragraphs are worked out: same size, close together, overlapping sideways
async function readBlocks(pg) {
  if (pg.blocks || !pg.src.doc) return pg.blocks || [];
  pg.blocks = [];
  const page = await pg.src.doc.getPage(pg.src.n),
    vp = page.getViewport({ scale: 1 }),
    tc = await page.getTextContent({ disableNormalization: true });
  await page.getOperatorList();
  const items = tc.items
    .filter((i) => i.str)
    .map((i) => {
      const a = pdfjs.Util.transform(vp.transform, i.transform),
        h = Math.hypot(a[2], a[3]);
      return { s: i.str, x: a[4], base: a[5], w: i.width, h, f: i.fontName };
    })
    .sort((p, q) => (Math.abs(p.base - q.base) < p.h * 0.5 ? p.x - q.x : p.base - q.base));
  const lines = [];
  for (const it of items) {
    const l = lines[lines.length - 1];
    if (!it.s.trim()) {
      if (l && Math.abs(l.base - it.base) < it.h * 0.5 && it.x - l.right < it.h) l.s += it.s;
      continue;
    }
    if (l && Math.abs(l.base - it.base) < it.h * 0.5 && it.x - l.right < it.h * 3) {
      if (it.x - l.right > it.h * 0.3 && /[\w,.;:]$/.test(l.s)) l.s += ' ';
      l.s += it.s;
      l.right = Math.max(l.right, it.x + it.w);
    } else lines.push({ s: it.s, x: it.x, right: it.x + it.w, base: it.base, h: it.h, f: it.f });
  }
  const blocks = [];
  for (const l of lines) {
    const home = blocks.find((k) => {
      const gap = l.base - k.lastBase;
      return (
        gap > 0
        && gap < l.h * 2.3
        && Math.abs(k.size - l.h) < l.h * 0.15
        && l.x < k.x + k.w
        && l.right > k.x
        && Math.abs(l.x - k.x) < l.h * 2.5
        && !blocks.some((o) => o !== k && o.lastBase > k.lastBase && o.lastBase < l.base)
      );
    });
    l.s = l.s.trimEnd();
    if (home) {
      home.lh = l.base - home.lastBase;
      if (/[A-Za-z0-9]$/.test(home.text) && /^[A-Za-z0-9]/.test(l.s)) home.text += ' ';
      home.text += l.s;
      home.lastBase = l.base;
      home.x = Math.min(home.x, l.x);
      home.w = Math.max(home.w, l.right - home.x);
      home.h = l.base + l.h * 0.25 - home.y;
      home.lines++;
    } else
      blocks.push({
        id: blocks.length,
        text: l.s,
        x: l.x,
        y: l.base - l.h * 0.95,
        w: l.right - l.x,
        h: l.h * 1.2,
        size: l.h,
        lh: l.h * 1.5,
        lastBase: l.base,
        firstBase: l.base,
        f: l.f,
        lines: 1,
      });
  }
  for (const k of blocks) {
    const font = page.commonObjs.has(k.f) ? page.commonObjs.get(k.f) : null,
      name = (font && font.name) || '';
    k.serif = /serif|song|ming|times|georgia|garamond/i.test(name) && !/sans/i.test(name);
    k.bold = /bold|black|heavy/i.test(name);
    k.color = INK;
  }
  // the words' colour is read off the page as drawn by day: the canvas on screen, or, for a page that was drawn and has
  // since given its canvas back, the same page drawn again for the moment it takes to read it
  const from =
    pg.canvas && pg.canvasKey === 'day'
      ? { c: pg.canvas, z: pg.canvasZ }
      : !pg.canvas && pg.wasDay && blocks.length
        ? await dayCopy(pg)
        : null;
  if (from) {
    const g = from.c.getContext('2d', { willReadFrequently: !!from.own }),
      z = from.z;
    for (const k of blocks) {
      const d = g.getImageData(k.x * z, k.y * z, Math.max(1, k.w * z), Math.max(1, Math.min(k.size, k.h) * z)).data;
      let low = 999,
        at = 0;
      for (let i = 0; i < d.length; i += 4) {
        const v = d[i] + d[i + 1] + d[i + 2];
        if (v < low) {
          low = v;
          at = i;
        }
      }
      if (low < 450 && Math.abs(d[at] - d[at + 2]) > 40)
        k.color =
          '#'
          + [d[at], d[at + 1], d[at + 2]]
            .map((n) => n.toString(16).padStart(2, '0'))
            .join('')
            .toUpperCase();
    }
    if (from.own) from.c.width = from.c.height = 0;
  }
  pg.blocks = blocks;
  return blocks;
}
// one at a time, so asking for every page's paragraphs at once (the edit tool, a Word export) never draws them all at once
let copying = Promise.resolve();
function dayCopy(pg) {
  const z = Math.min(pg.wasZ || 2, 2),
    run = copying
      .then(async () => {
        const c = document.createElement('canvas');
        c.width = Math.max(1, Math.round(pg.w * z));
        c.height = Math.max(1, Math.round(pg.h * z));
        const page = await pg.src.doc.getPage(pg.src.n);
        await page.render({
          canvasContext: c.getContext('2d', { willReadFrequently: true }),
          viewport: page.getViewport({ scale: z }),
          annotationMode: pdfjs.AnnotationMode.ENABLE_FORMS,
        }).promise;
        if (!pg.canvas) page.cleanup();
        return { c, z, own: true };
      })
      .catch(() => null);
  copying = run;
  return run;
}
const editMark = (pg, id) => pg.marks.find((x) => x.t === 'edit' && x.bid === id);
function blockAt(pg, p) {
  const hit = (x, y, w, h) => p.x >= x - 3 && p.x <= x + w + 3 && p.y >= y - 3 && p.y <= y + h + 3;
  for (const x of pg.marks) if (x.t === 'edit' && hit(x.x, x.y, x.w, x.h)) return x;
  return (pg.blocks || []).find((k) => !editMark(pg, k.id) && hit(k.x, k.y, k.w, k.h)) || null;
}
function markFor(pg, src) {
  if (src.t === 'edit') return src;
  const x = {
    t: 'edit',
    bid: src.id,
    ox: src.x,
    oy: src.y,
    ow: src.w,
    oh: src.h,
    x: src.x,
    y: src.firstBase - src.lh / 2 - src.size * 0.43,
    w: src.w,
    h: src.h,
    size: src.size,
    lh: src.lh,
    text: src.text,
    serif: src.serif,
    bold: src.bold,
    color: src.color,
    justify: src.lines > 1,
  };
  pg.marks.unshift(x);
  return x;
}
const blockFont = (x) => (x.serif ? "'Noto Serif SC', serif" : 'var(--font-text)');
// the words a block replaced can't be selected or found any more
function syncGone(pg) {
  const cuts = pg.marks.filter((x) => x.t === 'edit');
  if (!pg.tlDone || (!cuts.length && !pg.hadCuts)) return;
  pg.hadCuts = cuts.length > 0;
  pg.tl.querySelectorAll('span:not(.markedContent)').forEach((s) => {
    const r = s.getBoundingClientRect(),
      c = toPage(pg, r.left + r.width / 2, r.top + r.height / 2);
    s.classList.toggle(
      'gone',
      cuts.some((x) => c.x >= x.ox && c.x <= x.ox + x.ow && c.y >= x.oy && c.y <= x.oy + x.oh)
    );
  });
}
const blockBar = el('div', 'sel-bar block-bar');
blockBar.id = 'blockBar';
shell.place(blockBar);
function placeBlockBar() {
  if (!openMark) return;
  const r = blockBox.getBoundingClientRect(),
    w = blockBar.offsetWidth,
    h = blockBar.offsetHeight,
    top = r.top - h - 10;
  blockBar.style.left = clamp(r.left + r.width / 2 - w / 2, 8, innerWidth - w - 8) + 'px';
  blockBar.style.top = (top < topGap() - 8 ? r.bottom + 10 : top) + 'px';
}
function drawBlockBar() {
  const x = openMark;
  blockBar.textContent = '';
  const tweak = () => {
    fitBlock();
    placeBlockBar();
  };
  const btn = (id, n, tip, run) => {
    const c = el('button', 'tb');
    c.type = 'button';
    c.id = id;
    c.innerHTML = icon(n);
    c.setAttribute('aria-label', T(tip));
    c.dataset.tip = T(tip);
    c.addEventListener('mousedown', (e) => e.preventDefault());
    c.addEventListener('click', () => {
      sound();
      run(c);
    });
    return c;
  };
  const size = el('span', 'block-size dyn');
  const setSize = (v) => {
    v = clamp(v, 6, 48);
    x.lh = (x.lh * v) / x.size;
    x.size = v;
    size.textContent = Math.round(v) + ' pt';
    tweak();
  };
  const bold = btn('blockBold', 'bold', '加粗', (c) => {
    x.bold = !x.bold;
    c.setAttribute('aria-pressed', String(x.bold));
    tweak();
  });
  bold.setAttribute('aria-pressed', String(!!x.bold));
  const row = el('div', 'sw-row');
  const dots = SWATCHES.map((v) => {
    const c = el('button', 'swatch');
    c.type = 'button';
    c.dataset.c = v;
    c.style.setProperty('--c', v);
    c.setAttribute('aria-label', v);
    c.setAttribute('aria-pressed', String(v === x.color));
    c.addEventListener('mousedown', (e) => e.preventDefault());
    c.addEventListener('click', () => {
      sound();
      x.color = v;
      dots.forEach((d) => d.setAttribute('aria-pressed', String(d === c)));
      tweak();
    });
    row.appendChild(c);
    return c;
  });
  const back = el('button', 'tb', T('还原'));
  back.type = 'button';
  back.id = 'blockRestore';
  back.addEventListener('mousedown', (e) => e.preventDefault());
  back.addEventListener('click', () => {
    sound();
    const pg = openPg,
      was = !openNew;
    openMark = null;
    openPg = null;
    blockBox.remove();
    blockBar.classList.remove('on');
    pg.marks = pg.marks.filter((y) => y !== x);
    drawMarks(pg);
    if (was) commit();
  });
  blockBar.append(
    btn('blockSmaller', 'minus', '缩小', () => setSize(x.size - 1)),
    size,
    btn('blockBigger', 'plus', '放大', () => setSize(x.size + 1)),
    el('i', 'sep'),
    bold,
    el('i', 'sep'),
    row,
    el('i', 'sep'),
    back
  );
  size.textContent = Math.round(x.size) + ' pt';
}
stage.addEventListener('scroll', placeBlockBar, { passive: true });
let hoverBlock = null,
  openMark = null,
  openPg = null,
  openWas = '',
  openNew = false;
const blockBox = document.createElement('textarea');
blockBox.className = 'block-edit';
blockBox.id = 'blockText';
blockBox.spellcheck = false;
function fitBlock() {
  const x = openMark;
  if (!x) return;
  Object.assign(blockBox.style, {
    left: x.x + 'px',
    top: x.y + 'px',
    width: x.w + 2 + 'px',
    fontSize: x.size + 'px',
    lineHeight: x.lh + 'px',
    fontFamily: blockFont(x),
    fontWeight: x.bold ? 700 : 400,
    color: x.color,
    textAlign: x.justify ? 'justify' : 'left',
    height: 'auto',
  });
  blockBox.style.height = blockBox.scrollHeight + 'px';
  x.text = blockBox.value;
  x.h = Math.max(x.lh, blockBox.scrollHeight);
}
const blockSig = (x) => JSON.stringify([x.text, x.size, x.bold, x.color, x.x, x.y]);
function openBlock(pg, src) {
  hoverBlock = null;
  finishBlock();
  openNew = src.t !== 'edit' && !editMark(pg, src.id);
  openMark = markFor(pg, src);
  openPg = pg;
  openWas = openNew ? '' : blockSig(openMark);
  if (openNew) openWas = blockSig(openMark);
  blockBox.value = openMark.text;
  pg.inner.appendChild(blockBox);
  fitBlock();
  drawMarks(pg);
  drawBlockBar();
  placeBlockBar();
  blockBar.classList.add('on');
  setTimeout(() => {
    blockBox.focus();
    blockBox.setSelectionRange(blockBox.value.length, blockBox.value.length);
  }, 20);
}
function finishBlock() {
  if (!openMark) return;
  const x = openMark,
    pg = openPg,
    same = blockSig(x) === openWas,
    isNew = openNew;
  fitBlock();
  openMark = null;
  openPg = null;
  blockBox.remove();
  blockBar.classList.remove('on');
  if (same && isNew) pg.marks = pg.marks.filter((y) => y !== x);
  drawMarks(pg);
  if (!same) commit();
}
blockBox.addEventListener('input', () => {
  fitBlock();
  placeBlockBar();
});
blockBox.addEventListener('keydown', (e) => {
  e.stopPropagation();
  if (e.key === 'Escape') {
    e.preventDefault();
    finishBlock();
  }
});
const brush = { color: '#B3261E', size: 2 },
  words = { color: INK, size: 12, bold: false };
let hlColor = 'green',
  signature = null;
try {
  signature = JSON.parse(store.get('dotany-pdf-sign') || 'null');
} catch {
  signature = null;
}
function pick(pg, m) {
  if (!m) hideSelBar();
  const was = picked && picked.pg;
  picked = m ? { pg, m } : null;
  if (was && was !== pg) drawMarks(was);
  if (pg) drawMarks(pg);
}
function removePicked() {
  if (!picked) return;
  const { pg, m } = picked;
  pg.marks = pg.marks.filter((x) => x !== m);
  picked = null;
  drawMarks(pg);
  commit();
  if (tool) renderPanel();
}

list.addEventListener('pointerdown', (e) => {
  if (e.button || pagesMode) return;
  if (e.target.closest('.fld, .fld-c, .lnk, .txt-edit')) return;
  const pg = pageAt(e);
  if (editing) finishEdit();
  if (e.target === blockBox) return;
  if (tool === 'edit') {
    finishBlock();
    if (!pg) return;
    e.preventDefault();
    const q = toPage(pg, e.clientX, e.clientY),
      src = blockAt(pg, q);
    if (src) {
      drag = { t: 'block', pg, src, m: null, sx: q.x, sy: q.y, ox: src.x, oy: src.y, moved: false };
      list.setPointerCapture(e.pointerId);
    }
    return;
  }
  if (!pg) {
    pick(null);
    return;
  }
  const p = toPage(pg, e.clientX, e.clientY),
    kinds =
      tool === 'text'
        ? ['text']
        : tool === 'sign'
          ? ['sign']
          : tool === 'redact'
            ? ['redact']
            : tool
              ? []
              : ['text', 'sign', 'redact'];
  const hit = markAt(pg, p, kinds);
  if (hit) {
    e.preventDefault();
    hideSelBar();
    const was = picked && picked.m === hit;
    pick(pg, hit);
    if (tool) renderPanel();
    const b = boxOf(hit),
      corner = (hit.t === 'sign' || hit.t === 'redact') && p.x > b.x + b.w - 7 && p.y > b.y + b.h - 7;
    drag = {
      t: corner ? 'size' : 'move',
      pg,
      m: hit,
      sx: p.x,
      sy: p.y,
      ox: hit.x,
      oy: hit.y,
      ow: hit.w,
      oh: hit.h,
      moved: false,
      was,
    };
    list.setPointerCapture(e.pointerId);
    return;
  }
  pick(null);
  if (tool === 'brush') {
    e.preventDefault();
    const m = { t: 'brush', color: brush.color, size: brush.size, pts: [p.x, p.y] };
    pg.marks.push(m);
    drag = { t: 'brush', pg, m };
    drawMarks(pg);
    list.setPointerCapture(e.pointerId);
  } else if (tool === 'redact') {
    e.preventDefault();
    const m = { t: 'redact', x: p.x, y: p.y, w: 0, h: 0 };
    pg.marks.push(m);
    drag = { t: 'redact', pg, m, sx: p.x, sy: p.y };
    list.setPointerCapture(e.pointerId);
  } else if (tool === 'text') {
    e.preventDefault();
    startEdit(
      pg,
      {
        t: 'text',
        x: p.x,
        y: p.y - words.size * 0.6,
        words: '',
        color: words.color,
        size: words.size,
        bold: words.bold,
      },
      true
    );
  } else if (tool === 'sign') {
    e.preventDefault();
    if (!signature) {
      say('请先在右侧完成签名绘制。');
      return;
    }
    const w = 130,
      h = (w * signature.h) / signature.w,
      m = { t: 'sign', x: p.x - w / 2, y: p.y - h / 2, w, h, sig: signature };
    pg.marks.push(m);
    pick(pg, m);
    commit();
    drag = { t: 'move', pg, m, sx: p.x, sy: p.y, ox: m.x, oy: m.y, moved: true };
    list.setPointerCapture(e.pointerId);
    renderPanel();
  }
});
list.addEventListener('pointermove', (e) => {
  if (tool === 'brush') {
    ring.classList.add('on');
    ring.style.left = e.clientX + 'px';
    ring.style.top = e.clientY + 'px';
    ring.style.width = ring.style.height = Math.max(4, brush.size * zoom) + 'px';
  }
  if (!drag && tool === 'edit') {
    const pg = pageAt(e),
      now = pg ? blockAt(pg, toPage(pg, e.clientX, e.clientY)) : null,
      was = hoverBlock;
    if (!was || was.src !== now) {
      hoverBlock = now ? { pg, src: now } : null;
      if (was) drawMarks(was.pg);
      if (now && (!was || was.pg !== pg)) drawMarks(pg);
      else if (now) drawMarks(pg);
    }
    return;
  }
  if (!drag) {
    const pg = pageAt(e);
    if (pg && !tool && !pagesMode) {
      const p = toPage(pg, e.clientX, e.clientY);
      stage.classList.toggle('over-mark', !!markAt(pg, p, ['text', 'sign', 'redact']));
    }
    return;
  }
  const p = toPage(drag.pg, e.clientX, e.clientY),
    m = drag.m;
  if (drag.t === 'block') {
    if (!drag.moved && Math.hypot(p.x - drag.sx, p.y - drag.sy) < 4) return;
    drag.moved = true;
    hoverBlock = null;
    if (!drag.m) {
      drag.m = markFor(drag.pg, drag.src);
      drag.ox = drag.m.x;
      drag.oy = drag.m.y;
    }
    drag.m.x = drag.ox + p.x - drag.sx;
    drag.m.y = drag.oy + p.y - drag.sy;
    drawMarks(drag.pg);
    return;
  }
  if (drag.t === 'brush') {
    m.pts.push(p.x, p.y);
  } else if (drag.t === 'redact') {
    m.x = Math.min(p.x, drag.sx);
    m.y = Math.min(p.y, drag.sy);
    m.w = Math.abs(p.x - drag.sx);
    m.h = Math.abs(p.y - drag.sy);
  } else if (drag.t === 'move') {
    m.x = drag.ox + p.x - drag.sx;
    m.y = drag.oy + p.y - drag.sy;
    drag.moved = true;
  } else if (drag.t === 'size') {
    const k = m.t === 'sign' ? m.h / m.w : 0;
    m.w = Math.max(12, drag.ow + p.x - drag.sx);
    m.h = k ? m.w * k : Math.max(6, drag.oh + p.y - drag.sy);
    drag.moved = true;
  }
  drawMarks(drag.pg);
});
list.addEventListener('pointerleave', () => ring.classList.remove('on'));
function endDrag(e) {
  if (!drag) return;
  const d = drag;
  drag = null;
  if (d.t === 'move' && !d.moved && d.m.t === 'redact' && e) {
    showSelBar(e.clientX, e.clientY, [['redactDelete', 'trash', '删除', () => removePicked()]]);
    return;
  }
  if (d.t === 'block') {
    if (d.moved) commit();
    else openBlock(d.pg, d.src);
    return;
  }
  if (d.t === 'brush') commit();
  else if (d.t === 'redact') {
    if (d.m.w < 4 || d.m.h < 4) {
      d.pg.marks = d.pg.marks.filter((x) => x !== d.m);
      drawMarks(d.pg);
    } else commit();
  } else if (d.moved) commit();
  else if (d.m.t === 'text' && (tool === 'text' || d.was)) {
    const m = d.m;
    pick(null);
    startEdit(d.pg, m, false);
  }
}
list.addEventListener('pointerup', endDrag);
list.addEventListener('pointercancel', endDrag);

// words on the page: typed in place, with the family's caret
const editBox = el('div', 'txt-edit'),
  editIn = document.createElement('input');
editIn.type = 'text';
editIn.id = 'markText';
editIn.spellcheck = false;
editIn.autocomplete = 'off';
editBox.appendChild(editIn);
inkfall(editIn);
let editIsNew = false;
function fitEdit() {
  const m = editing;
  textMeter.font = `${m.bold ? 700 : 400} ${m.size}px ${fontText()}`;
  editBox.style.width = Math.ceil(textMeter.measureText(editIn.value || ' ').width) + 6 + 'px';
}
function syncEditStyle() {
  const m = editing;
  if (!m) return;
  Object.assign(editBox.style, {
    left: m.x + 'px',
    top: m.y + 'px',
    fontSize: m.size + 'px',
    fontWeight: m.bold ? 700 : 400,
    color: m.color,
  });
  fitEdit();
}
function startEdit(pg, m, isNew) {
  editing = m;
  editingPg = pg;
  editIsNew = isNew;
  if (isNew) pg.marks.push(m);
  editIn.value = m.words;
  pg.inner.appendChild(editBox);
  syncEditStyle();
  drawMarks(pg);
  setTimeout(() => {
    editIn.focus();
    editIn.select();
  }, 20);
}
function finishEdit() {
  if (!editing) return;
  const m = editing,
    pg = editingPg,
    had = m.words,
    now = editIn.value.trim();
  editing = null;
  editingPg = null;
  editBox.remove();
  if (!now) pg.marks = pg.marks.filter((x) => x !== m);
  else m.words = now;
  drawMarks(pg);
  if ((editIsNew && now) || (!editIsNew && had !== now)) commit();
}
editIn.addEventListener('input', fitEdit);
editIn.addEventListener('keydown', (e) => {
  e.stopPropagation();
  if (e.key === 'Enter' || e.key === 'Escape') {
    e.preventDefault();
    finishEdit();
  }
});
editIn.addEventListener('blur', () =>
  setTimeout(() => {
    if (document.activeElement !== editIn) finishEdit();
  }, 0)
);

// ---------- selecting text: a small sheet appears only when there is something to do ----------
function textRects(range, pg) {
  const out = [],
    spans = pg.tl.querySelectorAll('span:not(.markedContent)');
  for (const s of spans) {
    if (!range.intersectsNode(s) || !s.firstChild) continue;
    const r = document.createRange();
    r.selectNodeContents(s);
    if (range.compareBoundaryPoints(Range.START_TO_START, r) > 0) r.setStart(range.startContainer, range.startOffset);
    if (range.compareBoundaryPoints(Range.END_TO_END, r) < 0) r.setEnd(range.endContainer, range.endOffset);
    for (const c of r.getClientRects()) {
      if (c.width < 0.5) continue;
      const a = toPage(pg, c.left, c.top),
        b = toPage(pg, c.right, c.bottom),
        x = Math.min(a.x, b.x),
        y = Math.min(a.y, b.y),
        w = Math.abs(b.x - a.x),
        h = Math.abs(b.y - a.y),
        last = out[out.length - 1];
      if (last && Math.abs(last[1] - y) < h * 0.5 && x - (last[0] + last[2]) < h) {
        const end = Math.max(last[0] + last[2], x + w);
        last[1] = Math.min(last[1], y);
        last[3] = Math.max(last[3], h);
        last[2] = end - last[0];
      } else out.push([x, y, w, h]);
    }
  }
  return out;
}
function selected() {
  const s = getSelection();
  if (!s.rangeCount || s.isCollapsed) return null;
  const r = s.getRangeAt(0);
  if (!list.contains(r.commonAncestorContainer) || !s.toString().trim()) return null;
  return { s, r, text: s.toString() };
}
function highlightSelection() {
  const got = selected();
  if (!got) return false;
  if (!rights.annotate) {
    got.s.removeAllRanges();
    return false;
  }
  let any = false;
  pages.forEach((pg) => {
    if (!got.r.intersectsNode(pg.tl)) return;
    const rects = textRects(got.r, pg);
    if (rects.length) {
      pg.marks.push({ t: 'hl', color: hlColor, rects });
      drawMarks(pg);
      any = true;
    }
  });
  got.s.removeAllRanges();
  hideSelBar();
  if (any) commit();
  return any;
}
const hideSelBar = () => {
  selBar.classList.remove('on');
};
let selNow = new Map();
function paintSel(pg) {
  const g = pg.selG || (pg.selG = document.createElementNS(svgNS, 'g'));
  g.textContent = '';
  (selNow.get(pg) || []).forEach((r) =>
    svgEl('rect', { x: r[0], y: r[1], width: r[2], height: r[3], class: 'm-sel' }, g)
  );
  pg.svg.appendChild(g);
}
document.addEventListener('selectionchange', () => {
  const got = selected(),
    was = selNow;
  selNow = new Map();
  if (got)
    pages.forEach((pg) => {
      if (got.r.intersectsNode(pg.tl)) {
        const rects = textRects(got.r, pg);
        if (rects.length) selNow.set(pg, rects);
      }
    });
  new Set([...was.keys(), ...selNow.keys()]).forEach(paintSel);
});
function showSelBar(x, y, items, address) {
  selBar.textContent = '';
  // a link's address, before what can be done with it
  if (address) {
    const a = el('span', 'sel-addr dyn', address);
    a.title = address;
    selBar.appendChild(a);
  }
  items.forEach(([id, n, label, run]) => {
    const b = el('button', 'tb');
    b.type = 'button';
    b.id = id;
    b.innerHTML = icon(n) + `<span class="lab">${T(label)}</span>`;
    b.addEventListener('mousedown', (e) => e.preventDefault());
    b.addEventListener('click', () => {
      sound();
      run();
      hideSelBar();
    });
    selBar.appendChild(b);
  });
  if (!selBar.querySelector('button')) return;
  const w = selBar.offsetWidth,
    h = selBar.offsetHeight;
  selBar.style.left = clamp(x - w / 2, 8, innerWidth - w - 8) + 'px';
  selBar.style.top = clamp(y - h - 8, topGap() - 8, innerHeight - h - 8) + 'px';
  selBar.classList.add('on');
}
// http and https only: nothing else a file names is ever opened (the Rust side checks again, links.rs)
function offerLink(url, l) {
  const r = l.getBoundingClientRect();
  showSelBar(
    r.left + r.width / 2,
    r.top,
    [['linkOpen', 'external', '打开', () => own.openWebLink(url.trim())]],
    url.trim()
  );
}
addEventListener('pointerup', (e) => {
  // a click on a web link opens its bar: the click comes after this
  if (pagesMode || selBar.contains(e.target) || !pages.length || (e.target.closest && e.target.closest('.lnk.web')))
    return;
  setTimeout(() => {
    if (picked && picked.m.t === 'redact') return;
    const got = selected();
    if (got) {
      if (tool === 'hl') {
        highlightSelection();
        return;
      }
      if (tool) return;
      const c = got.r.getBoundingClientRect();
      showSelBar(c.left + c.width / 2, c.top, [
        ...(rights.copy
          ? [
              [
                'selCopy',
                'copy',
                // to the clipboard is 拷贝 on a Chinese Mac
                os({ windows: '复制', mac: '拷贝' }),
                () => {
                  copyText(got.text);
                  got.s.removeAllRanges();
                },
              ],
            ]
          : []),
        ...(rights.annotate ? [['selHl', 'highlight', '高亮', () => highlightSelection()]] : []),
      ]);
      return;
    }
    const pg = stage.contains(e.target) ? pageAt(e) : null;
    const m = pg && !tool ? hlAt(pg, toPage(pg, e.clientX, e.clientY)) : null;
    if (m && rights.annotate)
      showSelBar(e.clientX, e.clientY, [
        [
          'selDelete',
          'trash',
          '删除',
          () => {
            pg.marks = pg.marks.filter((x) => x !== m);
            drawMarks(pg);
            commit();
          },
        ],
      ]);
    else if (!selBar.contains(e.target)) hideSelBar();
  }, 0);
});
async function copyText(t) {
  if (!rights.copy) return;
  try {
    await navigator.clipboard.writeText(t);
  } catch {
    const a = document.createElement('textarea');
    a.value = t;
    document.body.appendChild(a);
    a.select();
    document.execCommand('copy');
    a.remove();
  }
}
// a file that says it mustn't be copied isn't, by any keyboard either
document.addEventListener('copy', (e) => {
  if (opened && !rights.copy && list.contains(e.target)) e.preventDefault();
});

// ---------- undo: one list for everything that changes the file ----------
let history = [],
  hAt = 0,
  savedSnap = null;
const snap = () => ({
  order: pages.slice(),
  rots: pages.map((p) => p.rot),
  marks: pages.map((p) => structuredClone(p.marks.filter((m) => m !== editing))),
  vals: { ...values },
});
function commit() {
  history = history.slice(0, hAt + 1);
  history.push(snap());
  hAt = history.length - 1;
  syncHist();
  syncCount();
  shell.changed();
}
function restore(s) {
  const change = () => {
    pages = s.order.slice();
    pages.forEach((p, i) => {
      p.rot = s.rots[i];
      p.marks = structuredClone(s.marks[i]);
    });
    values = { ...s.vals };
    picked = null;
    setList();
    pages.forEach((p) => {
      drawMarks(p);
      p.fields.forEach((f) => f.set(values[f.key]));
    });
    pickedPages = new Set([...pickedPages].filter((p) => pages.includes(p)));
    syncPicked();
    nowAt = clamp(nowAt, 0, pages.length - 1);
    syncPageNo();
  };
  if (pagesMode) flip(change, MOVE);
  else change();
  syncHist();
  syncCount();
  shell.changed();
}
function undo() {
  if (editing) finishEdit();
  finishBlock();
  if (hAt > 0) {
    hAt--;
    restore(history[hAt]);
  }
}
function redo() {
  if (hAt < history.length - 1) {
    hAt++;
    restore(history[hAt]);
  }
}
function syncHist() {
  undoBtn.disabled = hAt <= 0;
  redoBtn.disabled = hAt >= history.length - 1;
}
const dirty = () => opened && history[hAt] !== savedSnap;

// ---------- the pages view: the same sheets of paper draw back into a grid ----------
const settleFlip = () =>
  requestAnimationFrame(() => requestAnimationFrame(() => root.classList.remove('flipping', 'flip-out')));
function ghostOf(e, r) {
  const g = e.cloneNode(true);
  g.classList.add('pg-ghost');
  g.style.cssText += `;left:${r.left}px;top:${r.top}px;width:${r.width}px;height:${r.height}px;margin:0`;
  const from = e.querySelectorAll('canvas'),
    to = g.querySelectorAll('canvas');
  from.forEach((c, i) => {
    to[i].width = c.width;
    to[i].height = c.height;
    to[i].getContext('2d').drawImage(c, 0, 0);
  });
  return g;
}
function fadeGhost(g) {
  stage.prepend(g);
  const a = g.animate([{ opacity: 1 }, { opacity: 0 }], MOVE);
  a.onfinish = a.oncancel = () => g.remove();
}
function flip(change, timing, skip, fadeLeavers) {
  const box = stage.getBoundingClientRect(),
    near = (r) => r.bottom > box.top - 60 && r.top < box.bottom + 60;
  const first = new Map(pages.map((p) => [p, p.el.getBoundingClientRect()])),
    tile0 = addTile.getBoundingClientRect();
  const ghosts =
    fadeLeavers && !reduce
      ? new Map(pages.filter((p) => near(first.get(p))).map((p) => [p, ghostOf(p.el, first.get(p))]))
      : null;
  change();
  if (reduce) return;
  const tile1 = addTile.getBoundingClientRect();
  if (pagesMode && tile1.width) {
    if (!tile0.width || Math.abs(tile0.top - tile1.top) > 1) addTile.animate([{ opacity: 0 }, { opacity: 1 }], MOVE);
    else if (tile0.left !== tile1.left)
      addTile.animate([{ transform: `translate(${tile0.left - tile1.left}px, 0)` }, { transform: 'none' }], timing);
  }
  for (const p of pages) {
    if (p === skip) continue;
    const a = first.get(p),
      b = p.el.getBoundingClientRect();
    if (ghosts && !near(b)) {
      if (ghosts.has(p)) fadeGhost(ghosts.get(p));
      continue;
    }
    if (!near(b) && !(a && near(a))) continue;
    if (!a || !near(a)) {
      p.el.animate([{ opacity: 0 }, { opacity: 1 }], timing);
      continue;
    }
    if (Math.abs(a.left - b.left) < 0.5 && Math.abs(a.top - b.top) < 0.5 && Math.abs(a.width - b.width) < 0.5) continue;
    p.el.animate(
      [
        {
          transformOrigin: '0 0',
          transform: `translate(${a.left - b.left}px, ${a.top - b.top}px) scale(${a.width / b.width})`,
        },
        { transformOrigin: '0 0', transform: 'none' },
      ],
      timing
    );
  }
}
// the dock rises the way DotAny Video's does when it starts trimming: taller from the bottom up, the new things fading in
function morphDock(change, timing = GROW) {
  if (reduce) {
    change();
    return;
  }
  pill.getAnimations().forEach((a) => a.cancel());
  const w0 = pill.offsetWidth,
    h0 = pill.offsetHeight;
  change();
  const w1 = pill.offsetWidth,
    h1 = pill.offsetHeight;
  pill.style.overflow = 'hidden';
  const a = pill.animate(
    [
      { width: w0 + 'px', height: h0 + 'px' },
      { width: w1 + 'px', height: h1 + 'px' },
    ],
    timing
  );
  a.onfinish = a.oncancel = () => {
    pill.style.overflow = '';
  };
  [...pill.querySelectorAll('.pages-head, .pill-row > *')].forEach((e) => {
    if (e.offsetParent) e.animate([{ opacity: 0 }, { opacity: 1 }], { ...MOVE, delay: 120, fill: 'backwards' });
  });
}
function enterPages() {
  if (pagesMode || !pages.length) return;
  closeAll();
  setTool('');
  getSelection().removeAllRanges();
  hideSelBar();
  pick(null);
  pagesBtn.setAttribute('aria-pressed', 'true');
  const at = pages[nowAt];
  pickedPages = new Set();
  syncPicked();
  root.classList.add('flipping');
  morphDock(() => {
    pagesMode = true;
    root.classList.add('pages-mode');
    syncPadsOnly();
  });
  flip(() => {
    pages.forEach((p) => place(p));
    const r = at.el.offsetTop - stage.clientHeight / 2 + at.el.offsetHeight / 2;
    stage.scrollTop = Math.max(0, r);
  }, GROW);
  settleFlip();
  layoutAll();
}
function leavePages(toPageNo) {
  if (!pagesMode) return;
  pagesBtn.setAttribute('aria-pressed', 'false');
  const at =
    toPageNo != null
      ? toPageNo
      : pages.findIndex((p) => pickedPages.has(p)) >= 0
        ? pages.findIndex((p) => pickedPages.has(p))
        : nowAt;
  nowAt = clamp(at, 0, pages.length - 1);
  syncPageNo();
  root.classList.add('flipping', 'flip-out');
  const tileWas = addTile.getBoundingClientRect(),
    tileGhost = reduce || !tileWas.width ? null : ghostOf(addTile, tileWas);
  morphDock(() => {
    pagesMode = false;
    root.classList.remove('pages-mode');
    syncPadsOnly();
  }, MOVE);
  if (tileGhost) {
    tileGhost.style.display = 'flex';
    fadeGhost(tileGhost);
  }
  if (fitted) zoom = fitZ();
  flip(
    () => {
      pages.forEach((p) => place(p));
      stage.scrollTop = pages[nowAt].el.offsetTop - topGap();
    },
    MOVE,
    null,
    true
  );
  settleFlip();
  pickedPages = new Set();
  syncPicked();
  layoutAll();
  syncPct();
}
function syncPadsOnly() {
  root.style.setProperty('--pad-l', padL() + 'px');
  root.style.setProperty('--pad-r', padR() + 'px');
  root.style.setProperty('--gut', gutter() + 'px');
}
function syncPicked() {
  pages.forEach((p) => p.el.classList.toggle('on', pickedPages.has(p)));
  const n = pickedPages.size;
  pickInfo.textContent = n ? T('已选 {n} 页', { n }) : T('选择页面');
  [rotL, rotR, delPages].forEach((b) => {
    b.disabled = !n || !rights.assemble;
  });
  extract.disabled = !n || !rights.assemble || !rights.copy;
  [addBlank, addFile].forEach((b) => {
    b.disabled = !rights.assemble;
  });
}
function rotatePicked(d) {
  const sel = [...pickedPages];
  if (!sel.length) return;
  flip(() => {
    sel.forEach((p) => {
      const was = p.rot;
      p.rot += d;
      const z = zFor(p);
      place(p);
      if (!reduce) p.inner.animate([{ transform: turned(p, z, was) }, { transform: turned(p, z) }], MOVE);
    });
  }, MOVE);
  commit();
}
function deletePicked() {
  if (!pickedPages.size || !rights.assemble) return;
  if (pickedPages.size >= pages.length) {
    say('文档至少需保留一页。');
    return;
  }
  const gone = pages.filter((p) => pickedPages.has(p));
  gone.forEach((p) => {
    const r = p.el.getBoundingClientRect(),
      g = p.el.cloneNode(true);
    g.className = 'pg-ghost';
    g.style.cssText = `left:${r.left}px;top:${r.top}px;width:${r.width}px;height:${r.height}px`;
    const src = p.canvas,
      c = g.querySelector('canvas');
    if (src && c) c.getContext('2d').drawImage(src, 0, 0);
    document.body.appendChild(g);
    const a = g.animate(
      [
        { opacity: 1, transform: 'none' },
        { opacity: 0, transform: 'scale(.92)' },
      ],
      MOVE
    );
    a.onfinish = () => g.remove();
  });
  flip(() => {
    pages = pages.filter((p) => !pickedPages.has(p));
    pickedPages = new Set();
    setList();
  }, MOVE);
  nowAt = clamp(nowAt, 0, pages.length - 1);
  syncPageNo();
  syncPicked();
  commit();
}
// dragging: the paper follows the pointer, the rest make room
let pageDrag = null;
list.addEventListener(
  'pointerdown',
  (e) => {
    if (!pagesMode || e.button) return;
    if (e.target.closest('.add-tile')) return;
    const pg = pageAt(e);
    if (!pg) {
      pickedPages = new Set();
      syncPicked();
      return;
    }
    e.preventDefault();
    pageDrag = { pg, sx: e.clientX, sy: e.clientY, moved: false, id: e.pointerId };
  },
  true
);
addEventListener('pointermove', (e) => {
  const d = pageDrag;
  if (!d) return;
  const dx = e.clientX - d.sx,
    dy = e.clientY - d.sy;
  if (!d.moved) {
    if (Math.hypot(dx, dy) < 5 || !rights.assemble) return;
    d.moved = true;
    d.pg.el.classList.add('lifted');
    d.group = pickedPages.has(d.pg) && pickedPages.size > 1 ? pages.filter((p) => pickedPages.has(p)) : [d.pg];
  }
  d.pg.el.style.transform = `translate(${dx}px, ${dy}px)`;
  const rest = pages.filter((p) => !d.group.includes(p));
  let at = rest.length;
  for (let i = 0; i < rest.length; i++) {
    const r = rest[i].el.getBoundingClientRect();
    if ((e.clientY < r.bottom + 20 && e.clientX < r.left + r.width / 2) || e.clientY < r.top) {
      at = i;
      break;
    }
  }
  const next = [...rest.slice(0, at), ...d.group, ...rest.slice(at)];
  if (next.some((p, i) => p !== pages[i])) {
    const before = d.pg.el.getBoundingClientRect();
    flip(
      () => {
        pages = next;
        setList();
      },
      MOVE,
      d.pg
    );
    const after = d.pg.el.getBoundingClientRect();
    d.sx += after.left - before.left;
    d.sy += after.top - before.top;
    d.pg.el.style.transform = `translate(${e.clientX - d.sx}px, ${e.clientY - d.sy}px)`;
    d.changed = true;
  }
});
addEventListener('pointerup', () => {
  const d = pageDrag;
  if (!d) return;
  pageDrag = null;
  if (!d.moved) {
    if (pickedPages.has(d.pg)) pickedPages.delete(d.pg);
    else pickedPages.add(d.pg);
    syncPicked();
    return;
  }
  d.pg.el.classList.remove('lifted');
  const t = d.pg.el.style.transform;
  d.pg.el.style.transform = '';
  if (!reduce && t) d.pg.el.animate([{ transform: t }, { transform: 'none' }], MOVE);
  if (d.changed) {
    commit();
    pages.forEach((p, i) => {
      p.no.textContent = i + 1;
    });
  }
});
list.addEventListener('dblclick', (e) => {
  if (!pagesMode) return;
  const pg = pageAt(e);
  if (pg) leavePages(pages.indexOf(pg));
});

// adding: a PDF's pages, or a picture as a page of its own
async function pagesFrom(data, sid, name) {
  const got = await loadPdf(data, name);
  if (!got) return null;
  const { doc } = got,
    out = [];
  for (let n = 1; n <= doc.numPages; n++) {
    const v = (await doc.getPage(n)).getViewport({ scale: 1 });
    out.push(makePage({ doc, n, sid }, v.width, v.height, v.transform));
  }
  return { doc, out, password: got.password };
}
function insert(added) {
  flip(() => {
    pages = [...pages, ...added];
    setList();
  }, MOVE);
  added.forEach((p) => drawMarks(p));
  commit();
  syncPageNo();
  if (pagesMode) {
    pickedPages = new Set(added);
    syncPicked();
    const r = addTile.getBoundingClientRect(),
      s = stage.getBoundingClientRect(),
      over = r.bottom - (s.bottom - 124);
    if (over > 0) glideTo(stage.scrollTop + over);
  }
}
// files from outside (a drop on the page view), read the way Import from File reads what's picked
const addPaths = async (paths) => addFiles(await own.addSources(paths));
async function addFiles(added) {
  for (const f of added || []) {
    if (f.kind === 'refused') {
      say('暂不支持该文件格式。');
      continue;
    }
    let bytes;
    try {
      bytes = new Uint8Array(await own.readSource(f.id));
    } catch {
      say('文件已损坏，或并非有效的 PDF 文档。');
      continue;
    }
    if (f.kind === 'pdf') {
      let got;
      try {
        got = await pagesFrom(bytes, f.id, f.name);
      } catch {
        say('文件已损坏，或并非有效的 PDF 文档。');
        continue;
      }
      if (!got) continue;
      if (got.password) await own.sourcePassword(f.id, got.password);
      docs.set(f.id, got.doc);
      fileSize += f.size;
      syncCount();
      insert(got.out);
    } else {
      let img;
      try {
        img = await createImageBitmap(new Blob([bytes]));
      } catch {
        say('文件已损坏，或并非有效的 PDF 文档。');
        continue;
      }
      const k = Math.min(595 / img.width, 842 / img.height, 1);
      fileSize += f.size;
      syncCount();
      insert([makePage({ img, sid: f.id }, img.width * k, img.height * k)]);
    }
  }
}

// ---------- side panels: the same ones as DotAny Image's ----------
let colorOpen = false,
  customColor = null;
const toHex = (r, g, b) =>
  '#'
  + [r, g, b]
    .map((v) => clamp(Math.round(v), 0, 255).toString(16).padStart(2, '0'))
    .join('')
    .toUpperCase();
const fromHex = (h) => [1, 3, 5].map((i) => parseInt(h.slice(i, i + 2), 16));
function toHsv(r, g, b) {
  r /= 255;
  g /= 255;
  b /= 255;
  const x = Math.max(r, g, b),
    d = x - Math.min(r, g, b);
  let h = 0;
  if (d) h = x === r ? ((g - b) / d + 6) % 6 : x === g ? (b - r) / d + 2 : (r - g) / d + 4;
  return [h * 60, x ? d / x : 0, x];
}
function fromHsv(h, s, v) {
  const c = v * s,
    x = c * (1 - Math.abs(((h / 60) % 2) - 1)),
    m = v - c,
    t = [
      [c, x, 0],
      [x, c, 0],
      [0, c, x],
      [0, x, c],
      [x, 0, c],
      [c, 0, x],
    ][Math.floor(h / 60) % 6];
  return t.map((n) => (n + m) * 255);
}
function swatches(cur, pickColor) {
  const wrap = el('div'),
    row = el('div', 'sw-row'),
    fixed = [];
  if (!SWATCHES.includes(cur)) customColor = cur;
  const mark = (c) => {
    fixed.forEach((b) => b.setAttribute('aria-pressed', String(b.dataset.c === c)));
    more.setAttribute('aria-pressed', String(!SWATCHES.includes(c)));
  };
  SWATCHES.forEach((c) => {
    const b = el('button', 'swatch');
    b.type = 'button';
    b.dataset.c = c;
    b.style.setProperty('--c', c);
    b.setAttribute('aria-label', c);
    b.addEventListener('mousedown', (e) => e.preventDefault());
    b.addEventListener('click', () => {
      sound();
      mark(c);
      setFrom(c);
      pickColor(c, true);
    });
    fixed.push(b);
    row.appendChild(b);
  });
  const more = el('button', 'swatch custom');
  more.type = 'button';
  more.id = 'colorMore';
  more.setAttribute('aria-label', '#');
  more.setAttribute('aria-expanded', String(colorOpen));
  const paintMore = () => {
    if (customColor) {
      more.style.setProperty('--c', customColor);
      more.classList.add('has');
    }
  };
  paintMore();
  row.appendChild(more);
  const box = el('div', 'cp' + (colorOpen ? ' on' : '')),
    inner = el('div', 'cp-in');
  const sq = el('div', 'cp-sq'),
    sqKnob = el('i', 'cp-knob'),
    hue = el('div', 'cp-hue'),
    hueKnob = el('i', 'cp-knob');
  sq.appendChild(sqKnob);
  hue.appendChild(hueKnob);
  const nums = el('div', 'cp-nums');
  const fields = ['R', 'G', 'B'].map((n) => {
    const w = el('label', 'cp-num'),
      i = document.createElement('input');
    i.type = 'number';
    i.min = 0;
    i.max = 255;
    i.id = 'color' + n;
    i.setAttribute('aria-label', n);
    w.append(el('span', '', n), i);
    nums.appendChild(w);
    return i;
  });
  inner.append(sq, hue, nums);
  box.appendChild(inner);
  let H = 0,
    S = 0,
    V = 0;
  const show = (fromFields) => {
    const rgb = fromHsv(H, S, V);
    sq.style.setProperty('--h', H);
    sqKnob.style.left = S * 100 + '%';
    sqKnob.style.top = (1 - V) * 100 + '%';
    sqKnob.style.background = toHex(...rgb);
    hueKnob.style.left = H / 3.6 + '%';
    hueKnob.style.background = `hsl(${H} 100% 50%)`;
    if (!fromFields)
      fields.forEach((f, i) => {
        f.value = Math.round(rgb[i]);
      });
  };
  function setFrom(c) {
    const [h, a, b] = toHsv(...fromHex(c));
    if (a) H = h;
    S = a;
    V = b;
    show();
  }
  const emit = (final) => {
    const c = toHex(...fromHsv(H, S, V));
    customColor = c;
    paintMore();
    mark(c);
    pickColor(c, final);
  };
  const dragOn = (node, onMove) => {
    node.addEventListener('pointerdown', (e) => {
      e.preventDefault();
      node.setPointerCapture(e.pointerId);
      onMove(e);
      emit(false);
    });
    node.addEventListener('pointermove', (e) => {
      if (node.hasPointerCapture(e.pointerId)) {
        onMove(e);
        emit(false);
      }
    });
    node.addEventListener('pointerup', () => emit(true));
  };
  dragOn(sq, (e) => {
    const r = sq.getBoundingClientRect();
    S = clamp((e.clientX - r.left) / r.width, 0, 1);
    V = 1 - clamp((e.clientY - r.top) / r.height, 0, 1);
    show();
  });
  dragOn(hue, (e) => {
    const r = hue.getBoundingClientRect();
    H = clamp((e.clientX - r.left) / r.width, 0, 0.9999) * 360;
    show();
  });
  fields.forEach((f) => {
    const read = () => {
      const [h, a, b] = toHsv(...fields.map((i) => clamp(+i.value || 0, 0, 255)));
      if (a) H = h;
      S = a;
      V = b;
      show(true);
    };
    f.addEventListener('input', () => {
      read();
      emit(false);
    });
    f.addEventListener('change', () => {
      read();
      show();
      emit(true);
    });
    f.addEventListener('keydown', (e) => e.stopPropagation());
  });
  more.addEventListener('mousedown', (e) => e.preventDefault());
  more.addEventListener('click', () => {
    sound();
    colorOpen = !colorOpen;
    box.classList.toggle('on', colorOpen);
    more.setAttribute('aria-expanded', String(colorOpen));
    if (colorOpen && customColor) {
      setFrom(customColor);
      mark(customColor);
      pickColor(customColor, true);
    }
  });
  wrap.append(row, box);
  setFrom(cur);
  mark(cur);
  return wrap;
}
const SWATCHES = ['#FFFFFF', '#151413', '#75716B', '#B3261E', '#1F4FA8', '#2C6E49'];
const label = (t) => el('span', 'side-label dyn', T(t));
const softBtn = (id, text, cls = 'btn soft') => {
  const b = el('button', cls + ' dyn', T(text));
  b.type = 'button';
  b.id = id;
  b.addEventListener('mousedown', (e) => e.preventDefault());
  return b;
};
// the family's slider (shared/web/parts.js), with this tool's names and units
const slider = ({ id, label: name, min, max, value, unit, onInput, onChange }) => {
  let v = value;
  const s = familySlider({
    id,
    label: name,
    min,
    max,
    step: 1,
    get: () => v,
    input: (n) => {
      v = n;
      onInput(n);
    },
    fmt: (n) => n + unit,
  });
  s.querySelector('input').addEventListener('change', () => onChange && onChange(v));
  return { root: s };
};
const allowed = (k) => (k === 'edit' || k === 'redact' ? rights.modify : rights.annotate);
function setTool(k) {
  if (editing) finishEdit();
  finishBlock();
  hoverBlock = null;
  if (k && !allowed(k)) return;
  if (k && pagesMode) leavePages();
  if (k && findBox.classList.contains('on')) closeFind();
  tool = k;
  pick(null);
  ['edit', 'hl', 'brush', 'text', 'sign', 'redact'].forEach((t) => root.classList.toggle('tool-' + t, t === k));
  if (k === 'edit') pages.forEach(readBlocks);
  toolBtns.forEach((b) => b.setAttribute('aria-pressed', String(b.dataset.k === k)));
  if (k) infoBtn.setAttribute('aria-pressed', 'false');
  if (k === 'hl') highlightSelection();
  ring.classList.remove('on');
  shell.closeAll();
  syncSeg();
  renderPanel();
}
function renderPanel() {
  const info = infoBtn.getAttribute('aria-pressed') === 'true',
    was = side.classList.contains('on');
  if (!opened || ((!tool || tool === 'edit' || tool === 'redact') && !info)) {
    side.classList.remove('on');
    if (was) syncPads();
    return;
  }
  side.textContent = '';
  const body = el('div', 'side-body'),
    titles = { hl: '高亮', brush: '画笔', text: '笔注', sign: '签名' };
  side.append(el('h3', 'dyn', T(info ? '信息' : titles[tool])), body);
  if (info) panelInfo(body);
  else if (tool === 'hl') panelHl(body);
  else if (tool === 'brush') panelBrush(body);
  else if (tool === 'text') panelText(body);
  else if (tool === 'sign') panelSign(body);
  side.classList.add('on');
  if (!was) syncPads();
}
function panelHl(body) {
  const c = el('div'),
    row = el('div', 'sw-row');
  const btns = Object.entries(HL).map(([k, v]) => {
    const b = el('button', 'swatch');
    b.type = 'button';
    b.id = 'hl-' + k;
    b.style.setProperty('--c', v);
    b.setAttribute('aria-label', k);
    b.setAttribute('aria-pressed', String(k === hlColor));
    b.addEventListener('mousedown', (e) => e.preventDefault());
    b.addEventListener('click', () => {
      sound();
      hlColor = k;
      btns.forEach((x) => x.setAttribute('aria-pressed', String(x === b)));
    });
    row.appendChild(b);
    return b;
  });
  c.append(label('色彩'), row);
  body.append(c);
}
function panelBrush(body) {
  const c = el('div');
  c.append(
    label('色彩'),
    swatches(brush.color, (v) => {
      brush.color = v;
    })
  );
  body.append(
    c,
    slider({
      id: 'brushSize',
      label: '笔画粗细',
      min: 1,
      max: 20,
      value: brush.size,
      unit: ' pt',
      onInput: (v) => {
        brush.size = v;
      },
    }).root
  );
}
function panelText(body) {
  const m = picked && picked.m.t === 'text' ? picked.m : editing,
    cur = m || words;
  const apply = (k, v, final) => {
    words[k] = v;
    const t = editing || (picked && picked.m);
    if (!t) return;
    t[k] = v;
    if (editing) syncEditStyle();
    else {
      drawMarks(picked.pg);
      if (final) commit();
    }
  };
  const c = el('div');
  c.append(
    label('色彩'),
    swatches(cur.color, (v, final) => apply('color', v, final))
  );
  const size = slider({
    id: 'textSize',
    label: '字号',
    min: 8,
    max: 48,
    value: cur.size,
    unit: ' pt',
    onInput: (v) => apply('size', v, false),
    onChange: () => {
      if (picked && !editing) commit();
    },
  });
  const bold = el('button', 'row compact');
  bold.type = 'button';
  bold.id = 'textBold';
  bold.setAttribute('role', 'switch');
  bold.setAttribute('aria-checked', String(!!cur.bold));
  bold.innerHTML = '<span class="row-t"><b></b></span><i class="tog"></i>';
  bold.querySelector('b').textContent = T('加粗');
  bold.addEventListener('mousedown', (e) => e.preventDefault());
  bold.addEventListener('click', () => {
    sound();
    const v = bold.getAttribute('aria-checked') !== 'true';
    bold.setAttribute('aria-checked', String(v));
    apply('bold', v, true);
  });
  body.append(c, size.root, bold);
  if (picked && picked.m.t === 'text') {
    const del = softBtn('textDelete', '删除笔注');
    del.style.justifyContent = 'center';
    del.addEventListener('click', () => {
      sound();
      removePicked();
    });
    body.appendChild(del);
  }
}
// signing: write once, and it's there next time. The picture stays on this computer
function saveSig(sig) {
  signature = sig;
  store.set('dotany-pdf-sign', sig ? JSON.stringify(sig) : null);
}
async function importSignature() {
  let bytes;
  try {
    bytes = new Uint8Array(await own.pickPicture(T('图像')));
  } catch {
    return;
  }
  const blob = new Blob([bytes], { type: bytes[0] === 0xff ? 'image/jpeg' : 'image/png' });
  const url = await new Promise((ok) => {
    const r = new FileReader();
    r.onload = () => ok(r.result);
    r.readAsDataURL(blob);
  });
  const i = new Image();
  i.onload = () => {
    saveSig({ w: i.width, h: i.height, img: url });
    if (tool === 'sign') renderPanel();
  };
  i.src = url;
}
function panelSign(body) {
  const pad = el('div', 'sign-pad'),
    c = el('canvas'),
    hint = el('span', 'sign-hint dyn', T('在此绘制手写签名'));
  c.id = 'signPad';
  pad.append(c, hint);
  const W = 232,
    H = 110,
    dpr = devicePixelRatio || 1;
  c.width = W * dpr;
  c.height = H * dpr;
  const g = c.getContext('2d');
  g.scale(dpr, dpr);
  g.lineCap = g.lineJoin = 'round';
  g.lineWidth = 2.4;
  g.strokeStyle = getComputedStyle(root).getPropertyValue('--text');
  let strokes = signature && signature.strokes ? signature.strokes.map((a) => a.slice()) : [],
    now = null;
  const paint = () => {
    g.clearRect(0, 0, W, H);
    hint.hidden = !!strokes.length || !!(signature && signature.img);
    strokes.forEach((a) => g.stroke(new Path2D(smooth(a))));
  };
  if (signature && signature.img) {
    const i = new Image();
    i.onload = () => {
      const k = Math.min(W / i.width, H / i.height);
      g.drawImage(i, (W - i.width * k) / 2, (H - i.height * k) / 2, i.width * k, i.height * k);
    };
    i.src = signature.img;
  }
  const at = (e) => {
    const r = c.getBoundingClientRect();
    return [((e.clientX - r.left) * W) / r.width, ((e.clientY - r.top) * H) / r.height];
  };
  c.addEventListener('pointerdown', (e) => {
    if (e.button) return;
    e.preventDefault();
    if (signature && signature.img) saveSig(null);
    c.setPointerCapture(e.pointerId);
    now = at(e);
    strokes.push(now);
    paint();
  });
  c.addEventListener('pointermove', (e) => {
    if (!now) return;
    now.push(...at(e));
    paint();
  });
  const stop = () => {
    if (!now) return;
    now = null;
    saveSig({ w: W, h: H, strokes: strokes.map((a) => a.map((n) => Math.round(n * 10) / 10)) });
  };
  c.addEventListener('pointerup', stop);
  c.addEventListener('pointercancel', stop);
  paint();
  const row = el('div', 'side-row'),
    clear = softBtn('signClear', '重写'),
    imp = softBtn('signImport', '导入签名图片');
  clear.addEventListener('click', () => {
    sound();
    strokes = [];
    saveSig(null);
    renderPanel();
  });
  imp.addEventListener('click', () => {
    sound();
    importSignature();
  });
  row.append(clear, imp);
  body.append(pad, row);
  if (picked && picked.m.t === 'sign') {
    const del = softBtn('signDelete', '删除签名');
    del.style.justifyContent = 'center';
    del.addEventListener('click', () => {
      sound();
      removePicked();
    });
    body.appendChild(del);
  }
}
function panelInfo(body) {
  const dl = el('dl', 'info-list'),
    p = pages[nowAt];
  const row = (k, v) => {
    if (!v) return;
    const d = el('div');
    d.append(el('dt', 'dyn', T(k)), el('dd', 'dyn', v));
    d.lastChild.dataset.file = '';
    dl.appendChild(d);
  };
  row('文档标题', meta_.title);
  row('作者', meta_.author);
  row('格式', 'PDF' + (meta_.version ? ' ' + meta_.version : ''));
  row('总页数', String(pages.length));
  if (p) {
    const mm = (n) => Math.round((n / 72) * 25.4),
      a4 = Math.abs(p.w - 595) < 3 && Math.abs(p.h - 842) < 3;
    row('页面尺寸', (a4 ? 'A4 · ' : '') + `${mm(p.w)} × ${mm(p.h)} mm`);
  }
  row('文件大小', money(fileSize));
  row('创建工具', meta_.producer);
  row('创建时间', meta_.created);
  body.appendChild(dl);
}
function closePanels() {
  const was = tocPanel.classList.contains('on') || infoBtn.getAttribute('aria-pressed') === 'true';
  tocPanel.classList.remove('on');
  root.classList.remove('toc-on');
  tocBtn.setAttribute('aria-pressed', 'false');
  infoBtn.setAttribute('aria-pressed', 'false');
  renderPanel();
  syncPads();
  return was;
}
function closeAll() {
  let was = closePanels();
  if (closeFind()) was = true;
  if (shell.closeAll()) was = true;
  return was;
}
tocBtn.addEventListener('click', () => {
  sound();
  if (pagesMode) leavePages();
  const on = !tocPanel.classList.contains('on');
  shell.closeAll();
  if (on && tool) setTool('');
  tocPanel.classList.toggle('on', on);
  root.classList.toggle('toc-on', on);
  tocBtn.setAttribute('aria-pressed', String(on));
  syncPads();
  if (on) placeTocPill();
});
infoBtn.addEventListener('click', () => {
  sound();
  if (pagesMode) leavePages();
  const on = infoBtn.getAttribute('aria-pressed') !== 'true';
  closeFind();
  shell.closeAll();
  if (tool) setTool('');
  infoBtn.setAttribute('aria-pressed', String(on));
  renderPanel();
});
pagesBtn.addEventListener('click', () => {
  sound();
  if (pagesMode) leavePages();
  else enterPages();
});
// the family's card, menu and settings open over the page: this page's own panels make room for them
// only the moment one opens: closing panels changes classes too, which this would otherwise hear again
let familyOpen = false;
new MutationObserver(() => {
  const now = !!document.querySelector('.dlg.on, .set.on, .pop.on:not(#toolMenu)');
  if (now && !familyOpen) {
    familyOpen = true;
    if (tocPanel.classList.contains('on') || infoBtn.getAttribute('aria-pressed') === 'true') closePanels();
    closeFind();
    shell.closePop();
  }
  familyOpen = now;
}).observe(document.body, { subtree: true, attributes: true, attributeFilter: ['class'] });

// the contents, on the left, laid out as DotAny Books's
let tocRows = [],
  tocPill = null;
function drawToc() {
  tocPanel.textContent = '';
  const list_ = el('div', 'toc-list');
  tocPill = el('i', 'toc-pl');
  list_.appendChild(tocPill);
  const rows = outline.length
    ? outline
    : pages.map((_, i) => ({ title: T('第 {n} 页', { n: i + 1 }), key: pages[i], depth: 0 }));
  tocRows = rows.map((t) => {
    const b = el('button', 'toc-row');
    b.type = 'button';
    b.style.paddingLeft = 12 + (t.depth || 0) * 14 + 'px';
    b.textContent = t.title;
    if (outline.length) b.dataset.file = '';
    b.addEventListener('mousedown', (e) => e.preventDefault());
    b.addEventListener('click', () => {
      sound();
      const i = pages.findIndex((p) => p === t.key || (t.n && p.src.doc === t.doc && p.src.n === t.n));
      if (i >= 0) goPage(i);
    });
    b.__t = t;
    list_.appendChild(b);
    return b;
  });
  tocPanel.append(el('h3', 'dyn', T('目录')), list_);
  markToc();
}
function markToc() {
  if (!tocRows.length) return;
  const p = pages[nowAt];
  if (!p) return;
  let on = null;
  for (const b of tocRows) {
    const t = b.__t;
    if (t.key === p || (t.n && t.doc === p.src.doc && t.n <= p.src.n)) on = b;
  }
  tocRows.forEach((b) => b.setAttribute('aria-current', String(b === on)));
  placeTocPill();
}
function placeTocPill() {
  const on = tocRows.find((b) => b.getAttribute('aria-current') === 'true');
  if (!tocPill) return;
  tocPill.style.opacity = on ? 1 : 0;
  if (!on) return;
  tocPill.style.transform = `translateY(${on.offsetTop}px)`;
  tocPill.style.height = on.offsetHeight + 'px';
}
async function readOutline(doc) {
  const out = [],
    walk = async (items, depth) => {
      for (const it of items || []) {
        let n = 0;
        try {
          const d = typeof it.dest === 'string' ? await doc.getDestination(it.dest) : it.dest;
          if (d) n = (await doc.getPageIndex(d[0])) + 1;
        } catch {
          n = 0;
        }
        if (n) out.push({ title: it.title, n, doc, depth });
        await walk(it.items, depth + 1);
      }
    };
  await walk(await doc.getOutline(), 0);
  return out;
}

// ---------- find: laid out as DotAny Text's, with the same two highlights ----------
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
let findHits = [],
  findAt = -1,
  findToken = 0;
function openFind() {
  if (pagesMode) leavePages();
  if (tool) setTool('');
  shell.closeAll();
  infoBtn.setAttribute('aria-pressed', 'false');
  renderPanel();
  findBox.classList.add('on');
  findBtn.setAttribute('aria-pressed', 'true');
  const s = selected();
  if (s && s.text.length < 80) {
    findIn.value = s.text.trim();
    runFind();
  }
  setTimeout(() => {
    findIn.focus();
    findIn.select();
  }, 30);
}
function closeFind() {
  if (!findBox.classList.contains('on')) return false;
  findBox.classList.remove('on');
  findBtn.setAttribute('aria-pressed', 'false');
  findHits = [];
  findAt = -1;
  findIn.blur();
  pages.forEach(drawMarks);
  return true;
}
async function runFind() {
  const token = ++findToken,
    q = findIn.value.trim().toLowerCase();
  const was = new Set(findHits.map((h) => h.pg));
  findHits = [];
  findAt = -1;
  if (q) {
    for (const pg of pages) {
      if (!pg.src.doc) continue;
      await buildText(pg);
      if (token !== findToken) return;
      const w = document.createTreeWalker(pg.tl, NodeFilter.SHOW_TEXT);
      let text = '';
      const map = [];
      for (let n = w.nextNode(); n; n = w.nextNode()) {
        if (n.parentElement.classList.contains('gone')) continue;
        for (let i = 0; i < n.nodeValue.length; i++) map.push([n, i]);
        text += n.nodeValue;
      }
      const hay = text.toLowerCase();
      for (let at = hay.indexOf(q); at >= 0; at = hay.indexOf(q, at + q.length)) {
        const r = document.createRange(),
          a = map[at],
          b = map[at + q.length - 1];
        r.setStart(a[0], a[1]);
        r.setEnd(b[0], b[1] + 1);
        findHits.push({ pg, rects: textRects(r, pg) });
        was.add(pg);
      }
      // what's found is kept as places on the page; the words of a page far from the view needn't stay built
      if (!pg.seen && !pg.canvas) dropText(pg);
    }
    // jump to the first match from where you're reading, not from the start of the file
    const i = findHits.findIndex((h) => pages.indexOf(h.pg) >= nowAt);
    findAt = findHits.length ? (i >= 0 ? i : 0) : -1;
  }
  was.forEach(drawMarks);
  showHit();
}
function showHit() {
  const q = findIn.value.trim();
  findCount.textContent = !q ? '' : findHits.length ? `${findAt + 1}/${findHits.length}` : T('无匹配结果');
  findBox.classList.toggle('none', !!q && !findHits.length);
  const h = findHits[findAt];
  if (!h) return;
  const r = h.pg.el.getBoundingClientRect(),
    y = h.rects[0] ? h.rects[0][1] * zFor(h.pg) : 0,
    top = stage.getBoundingClientRect().top,
    screen = r.top + y;
  if (screen < top + 90 || screen > top + stage.clientHeight - 120)
    glideTo(stage.scrollTop + screen - top - stage.clientHeight * 0.36);
}
function stepFind(d) {
  if (!findHits.length) return;
  const old = findHits[findAt];
  findAt = (findAt + d + findHits.length) % findHits.length;
  drawMarks(old.pg);
  drawMarks(findHits[findAt].pg);
  showHit();
}
findIn.addEventListener('input', () => {
  clearTimeout(runFind.t);
  runFind.t = setTimeout(runFind, 140);
});
findIn.addEventListener('keydown', (e) => {
  e.stopPropagation();
  if (e.key === 'Enter') {
    e.preventDefault();
    stepFind(e.shiftKey ? -1 : 1);
  } else if (e.key === 'Escape') closeFind();
});
findBtn.addEventListener('click', () => {
  sound();
  if (!closeFind()) openFind();
});

const syncCount = () => {
  countEl.textContent = T('{n} 页 · {size}', { n: pages.length, size: money(fileSize) });
};

// ---------- a page drawn whole as a picture: for PNG and JPG, for Word's scans, and for a page with a redaction ----------
// Everything on it goes in: the page, its annotations, the form's values, and the marks, with redactions black
async function pagePicture(pg, k, { white = true } = {}) {
  const c = document.createElement('canvas'),
    W = Math.max(1, Math.round(pg.w * k)),
    H = Math.max(1, Math.round(pg.h * k));
  c.width = W;
  c.height = H;
  const g = c.getContext('2d');
  if (white) {
    g.fillStyle = '#FFFFFF';
    g.fillRect(0, 0, W, H);
  }
  if (pg.src.img) g.drawImage(pg.src.img, 0, 0, W, H);
  else if (pg.src.doc) {
    const page = await pg.src.doc.getPage(pg.src.n);
    await page.render({
      canvasContext: g,
      viewport: page.getViewport({ scale: k }),
      annotationMode: pdfjs.AnnotationMode.ENABLE_FORMS,
    }).promise;
  }
  g.save();
  g.scale(k, k);
  const ff = fontText();
  for (const f of pg.fieldsInfo || []) {
    if (f.kind === 'text') {
      const v = f.value();
      if (!v) continue;
      const s = Math.min(f.h * 0.62, 12);
      g.fillStyle = INK;
      g.font = `400 ${s}px ${ff}`;
      g.textBaseline = 'middle';
      g.fillText(v, f.x + 2, f.y + f.h / 2, f.w - 4);
    } else if (f.kind === 'check' && f.value()) {
      g.strokeStyle = INK;
      g.lineWidth = Math.max(1, f.w / 8);
      g.lineCap = g.lineJoin = 'round';
      const sx = f.w / 24,
        sy = f.h / 24;
      g.beginPath();
      g.moveTo(f.x + 4 * sx, f.y + 12 * sy);
      g.lineTo(f.x + 9 * sx, f.y + 17 * sy);
      g.lineTo(f.x + 20 * sx, f.y + 6 * sy);
      g.stroke();
    }
  }
  for (const m of pg.marks) {
    if (m.t === 'hl') {
      g.save();
      g.globalAlpha = 0.28;
      g.globalCompositeOperation = 'multiply';
      g.fillStyle = HL_HEX[m.color];
      m.rects.forEach((r) => g.fillRect(r[0], r[1], r[2], r[3]));
      g.restore();
    } else if (m.t === 'brush') {
      g.strokeStyle = m.color;
      g.lineWidth = m.size;
      g.lineCap = g.lineJoin = 'round';
      g.stroke(new Path2D(smooth(m.pts)));
    } else if (m.t === 'text') {
      g.fillStyle = m.color;
      g.font = `${m.bold ? 700 : 400} ${m.size}px ${ff}`;
      g.textBaseline = 'middle';
      g.fillText(m.words, m.x, m.y + m.size * 0.6);
    } else if (m.t === 'redact') {
      g.fillStyle = '#000000';
      g.fillRect(m.x, m.y, m.w, m.h);
    } else if (m.t === 'sign') {
      if (m.sig.img) {
        const i = new Image();
        i.src = m.sig.img;
        await i.decode().catch(() => {});
        g.drawImage(i, m.x, m.y, m.w, m.h);
      } else {
        g.save();
        g.translate(m.x, m.y);
        g.scale(m.w / m.sig.w, m.h / m.sig.h);
        g.strokeStyle = INK;
        g.lineWidth = 2.4;
        g.lineCap = g.lineJoin = 'round';
        m.sig.strokes.forEach((a) => g.stroke(new Path2D(smooth(a))));
        g.restore();
      }
    } else if (m.t === 'edit') {
      g.fillStyle = '#FFFFFF';
      g.fillRect(m.ox - 1, m.oy - 1, m.ow + 2, m.oh + 3);
      g.fillStyle = m.color;
      g.font = `${m.bold ? 700 : 400} ${m.size}px ${m.serif ? "'Noto Serif SC', serif" : ff}`;
      g.textBaseline = 'alphabetic';
      for (const l of blockLines(m)) g.fillText(l.text, l.x, l.y);
    }
  }
  g.restore();
  const r = ((pg.rot % 360) + 360) % 360;
  if (!r) return c;
  const t = document.createElement('canvas');
  t.width = r % 180 ? H : W;
  t.height = r % 180 ? W : H;
  const tg = t.getContext('2d');
  tg.translate(t.width / 2, t.height / 2);
  tg.rotate((r * Math.PI) / 180);
  tg.drawImage(c, -W / 2, -H / 2);
  return t;
}
const blobOf = (c, type, q) => new Promise((ok) => c.toBlob(ok, type, q));

// A replaced paragraph's lines, as the window lays them out: where each line starts and where its baseline sits, so the
// file has the same lines in the same places
const meter = el('div', 'm-edit m-measure');
function blockLines(m) {
  Object.assign(meter.style, {
    width: m.w + 2 + 'px',
    fontSize: m.size + 'px',
    lineHeight: m.lh + 'px',
    fontFamily: blockFont(m),
    fontWeight: m.bold ? 700 : 400,
    textAlign: m.justify ? 'justify' : 'left',
  });
  meter.textContent = m.text;
  document.body.appendChild(meter);
  const node = meter.firstChild,
    box = meter.getBoundingClientRect(),
    out = [];
  if (node) {
    const r = document.createRange(),
      text = m.text;
    let start = 0,
      top = null,
      left = 0;
    const cs = getComputedStyle(meter);
    textMeter.font = `${cs.fontWeight} ${cs.fontSize} ${cs.fontFamily}`;
    const fm = textMeter.measureText('Hg'),
      asc = fm.fontBoundingBoxAscent,
      desc = fm.fontBoundingBoxDescent;
    const close = (end, isLast) => {
      const s = text.slice(start, end).replace(/\n$/, '');
      if (top != null)
        out.push({
          text: s,
          x: m.x + left - box.left,
          y: m.y + top - box.top + (m.lh - asc - desc) / 2 + asc,
          width: m.w + 2,
          justify: !!m.justify && !isLast && !/\n$/.test(text.slice(start, end)),
        });
    };
    for (let i = 0; i < text.length; i++) {
      if (text[i] === '\n') continue;
      r.setStart(node, i);
      r.setEnd(node, i + 1);
      const rect = r.getClientRects()[0];
      if (!rect) continue;
      const lineTop = Math.round((rect.top - box.top) / m.lh) * m.lh + box.top;
      if (top === null) {
        top = lineTop;
        left = rect.left;
        start = i;
        continue;
      }
      if (lineTop > top + 1) {
        close(i, false);
        top = lineTop;
        left = rect.left;
        start = i;
      }
    }
    close(text.length, true);
  }
  meter.remove();
  return out;
}

// ---------- the save card: as DotAny Image's, with the file's size as a choice of its own ----------
let sizeKey = 'same',
  saveScope = null;
const SIZES = { same: ['原始品质', 1], small: ['均衡压缩', 0.58], least: ['极致压缩', 0.34] };
function drawRows(rows, fmt, again) {
  if (fmt !== 'pdf') return;
  const box = el('div'),
    chips = el('div', 'chips three');
  Object.entries(SIZES).forEach(([k, [t]]) => {
    const b = el('button', 'chip dyn', T(t));
    b.type = 'button';
    b.id = 'size-' + k;
    b.setAttribute('aria-pressed', String(k === sizeKey));
    b.addEventListener('mousedown', (e) => e.preventDefault());
    b.addEventListener('click', () => {
      sound();
      sizeKey = k;
      [...chips.children].forEach((x) => x.setAttribute('aria-pressed', String(x === b)));
      again();
      syncNote();
    });
    chips.appendChild(b);
  });
  box.append(el('span', 'side-label dyn', T('文件大小')), chips);
  rows.appendChild(box);
}
const redactedIn = (list_) => list_.some((p) => p.marks.some((x) => x.t === 'redact'));
function noteFor(fmt) {
  const list_ = saveScope || pages;
  if (fmt === 'docx') return '转换为结构化文档，可在 Word 中自由编辑。';
  if (fmt === 'png' || fmt === 'jpg') return '仅导出当前选中页。';
  if (redactedIn(list_)) return '涂黑区域底层的文字与图像将被彻底抹除，不可恢复。';
  if (sizeKey !== 'same') return '文档内的插图分辨率将适度降低以缩减体积。';
  return '';
}
// the note line follows the size chosen, the same way it follows the format
function syncNote() {
  const n = document.getElementById('saveNote');
  if (!n) return;
  const fmt = (document.querySelector('.dlg .sg[aria-pressed="true"]') || {}).dataset?.k || 'pdf';
  const text = T(noteFor(fmt) || '');
  if (n.textContent !== text)
    shell.morph(
      n,
      () => {
        n.textContent = text;
        n.classList.toggle('none', !text);
      },
      () => [n]
    );
}
// pages taken out of the file: the same card, named for the pages, and the window stays on the file
function openExtract() {
  if (!pickedPages.size) return;
  saveScope = pages.filter((p) => pickedPages.has(p));
  // the card hangs from Save As in the top bar
  shell.openSaveAs(document.getElementById('saveAs')).then(() => {
    const nums = saveScope.map((p) => pages.indexOf(p) + 1),
      base = fileName.replace(/\.[^.]+$/, '');
    const i = document.getElementById('saveAsName');
    if (!i) return;
    i.value =
      base + T(' 第{range}页', { range: nums.length > 1 ? nums[0] + '-' + nums[nums.length - 1] : String(nums[0]) });
    i.dispatchEvent(new Event('input'));
    syncNote();
  });
}
new MutationObserver(() => {
  if (!document.querySelector('.dlg.on')) saveScope = null;
}).observe(document.querySelector('.dlg'), { attributes: true, attributeFilter: ['class'] });

// what's needed before the Rust side can write: the family's fonts for new words, and pictures of redacted pages
async function fontsReady(plan) {
  const words = plan.pages.some((p) => p.marks.some((m) => m.t === 'edit' || m.t === 'text')) || plan.fields.length;
  if (!words) return;
  const have = await own.fontsHeld();
  const want = [
    ['latin', fileSrc('StackSansText-wght.ttf', 'dotfont')],
    ['sans', fileSrc('HarmonyOS_SansSC_Regular.ttf', 'dotfont')],
    ['sans-bold', fileSrc('HarmonyOS_SansSC_Bold.ttf', 'dotfont')],
  ];
  for (const [name, url] of want) {
    if (have.includes(name)) continue;
    try {
      const r = await fetch(url);
      if (r.ok) await own.putFont(name, new Uint8Array(await r.arrayBuffer()));
    } catch {
      /* that font's words fall back to the next */
    }
  }
}
let rasterCache = new Map();
async function rasterFor(pg) {
  const key = JSON.stringify([pg.marks, pg.rot, (pg.fieldsInfo || []).map((f) => f.value())]);
  const was = rasterCache.get(pg);
  if (was && was.key === key) return was.id;
  // 200 dots to the inch: sharp enough to read and print, and nothing of the text under it survives
  const c = await pagePicture({ ...pg, rot: 0 }, 200 / 72);
  const blob = await blobOf(c, 'image/jpeg', 0.9),
    id = 'r' + Math.random().toString(36).slice(2);
  await own.stash(id, new Uint8Array(await blob.arrayBuffer()));
  rasterCache.set(pg, { key, id });
  return id;
}
function markOut(m) {
  if (m.t === 'hl') return { t: 'hl', color: HL_HEX[m.color] || HL_HEX.green, rects: m.rects };
  if (m.t === 'brush') return { t: 'brush', color: m.color, size: m.size, pts: m.pts };
  if (m.t === 'text')
    return { t: 'text', x: m.x, y: m.y, words: m.words, size: m.size, bold: !!m.bold, color: m.color };
  if (m.t === 'sign')
    return {
      t: 'sign',
      x: m.x,
      y: m.y,
      w: m.w,
      h: m.h,
      sw: m.sig.w,
      sh: m.sig.h,
      strokes: m.sig.strokes || [],
      img: m.sig.img || null,
    };
  if (m.t === 'edit')
    return {
      t: 'edit',
      ox: m.ox,
      oy: m.oy,
      ow: m.ow,
      oh: m.oh,
      size: m.size,
      bold: !!m.bold,
      serif: !!m.serif,
      color: m.color,
      lines: blockLines(m),
    };
  return null;
}
async function planOf(list_) {
  if (editing) finishEdit();
  finishBlock();
  const out = [];
  for (const pg of list_) {
    const p = {
      src: Math.max(0, pg.src.sid),
      n: pg.src.n || 0,
      w: pg.w,
      h: pg.h,
      vt: pg.vt,
      rot: ((pg.rot % 360) + 360) % 360,
      blank: !!pg.src.blank,
      marks: [],
    };
    if (pg.marks.some((m) => m.t === 'redact')) p.raster = await rasterFor(pg);
    else p.marks = pg.marks.map(markOut).filter(Boolean);
    out.push(p);
  }
  const fields = Object.entries(values).map(([k, v]) => {
    const at = k.indexOf('|');
    return { src: +k.slice(0, at), name: k.slice(at + 1), value: v };
  });
  const plan = { pages: out, fields, size: sizeKey };
  await fontsReady(plan);
  return plan;
}

// Word: the paragraphs found on each page, changed ones as they read now; a page without words goes in as its picture
async function bookOf() {
  const out = [];
  for (const pg of pages) {
    const p = { w: pg.w, h: pg.h, paras: [] };
    const blocks = pg.src.doc ? await readBlocks(pg) : [];
    const edits = pg.marks.filter((m) => m.t === 'edit');
    const paras = [
      ...blocks
        .filter((k) => !editMark(pg, k.id))
        .map((k) => ({ y: k.y, x: k.x, text: k.text, size: k.size, bold: !!k.bold, serif: !!k.serif, color: k.color })),
      ...edits.map((m) => ({
        y: m.y,
        x: m.x,
        text: m.text,
        size: m.size,
        bold: !!m.bold,
        serif: !!m.serif,
        color: m.color,
      })),
    ];
    paras.sort((a, b) => (Math.abs(a.y - b.y) < 2 ? a.x - b.x : a.y - b.y));
    p.paras = paras
      .filter((q) => q.text.trim())
      .map(({ text, size, bold, serif, color }) => ({ text, size: Math.round(size * 2) / 2, bold, serif, color }));
    if (!p.paras.length) {
      const c = await pagePicture(pg, 1.5),
        blob = await blobOf(c, 'image/jpeg', 0.85),
        id = 'w' + Math.random().toString(36).slice(2);
      await own.stash(id, new Uint8Array(await blob.arrayBuffer()));
      p.picture = id;
      p.pw = c.width;
      p.ph = c.height;
    }
    out.push(p);
  }
  return { pages: out };
}

let pictureCache = null;
async function pictureOf(fmt) {
  const pg = pages[nowAt],
    key = JSON.stringify([nowAt, fmt, pg.marks, pg.rot]);
  if (pictureCache && pictureCache.key === key && pictureCache.pg === pg) return pictureCache.bytes;
  // twice the page's size: a sharp picture for a screen
  const c = await pagePicture(pg, 2),
    blob = await blobOf(c, fmt === 'png' ? 'image/png' : 'image/jpeg', 0.92);
  const bytes = new Uint8Array(await blob.arrayBuffer());
  pictureCache = { key, pg, bytes };
  return bytes;
}
async function estimateFor(fmt) {
  if (!opened) return null;
  try {
    if (fmt === 'png' || fmt === 'jpg') return (await pictureOf(fmt)).length;
    if (fmt === 'docx') {
      const text = pages.reduce((n, p) => n + (p.blocks || []).reduce((m, k) => m + k.text.length * 3, 0), 0);
      return 6000 + text;
    }
    if (!rights.writable) return fileSize;
    return await own.estimatePdf(await planOf(saveScope || pages));
  } catch {
    return null;
  }
}
async function saveTo(path, fmt) {
  const doc = shell.doc,
    scope = saveScope;
  let r;
  if (fmt === 'png' || fmt === 'jpg') r = await own.saveBytes(path, await pictureOf(fmt));
  else if (fmt === 'docx') {
    r = await own.saveDocx(path, await bookOf());
  } else r = await own.savePdf(path, await planOf(scope || pages));
  if (r.status === 'unreadable') return { status: 'failed', kind: 'unreadable' };
  if (r.status !== 'saved') return r;
  if (fmt === 'pdf' && !scope) return r;
  // a picture, a Word document or pages taken out aren't this file: the window stays on it, and the line says where the copy went
  const name = r.name;
  setTimeout(() => shell.setState('已存为', name), 0);
  return { status: 'saved', path: doc.path, name: doc.name, exported: true };
}
function afterSave(r) {
  if (r.exported) return;
  savedSnap = history[hAt];
  fileName = r.name;
  fileSize = r.size || fileSize;
  syncCount();
}

// ---------- opening ----------
const pdfDate = (d) => {
  const m = /^D:(\d{4})(\d{2})(\d{2})/.exec(d || '');
  return m ? `${m[1]}-${m[2]}-${m[3]}` : '';
};
// a file with a password asks for it; a wrong one asks again; giving up leaves the window as it was
async function loadPdf(data, name) {
  let pw = null,
    wrong = false;
  for (;;) {
    const task = pdfjs.getDocument({
      data: data.slice(),
      password: pw || undefined,
      isEvalSupported: false,
      enableXfa: false,
      verbosity: 0,
      cMapUrl: new URL('./cmaps/', import.meta.url).href,
      cMapPacked: true,
      standardFontDataUrl: new URL('./standard_fonts/', import.meta.url).href,
      useSystemFonts: false,
      disableFontFace: false,
    });
    try {
      const doc = await task.promise;
      return { doc, password: pw };
    } catch (e) {
      task.destroy();
      if (e && e.name === 'PasswordException') {
        wrong = pw != null;
        pw = await askPassword(name, wrong);
        if (pw == null) return null;
        continue;
      }
      throw e;
    }
  }
}
async function openPdf(path) {
  let bytes;
  try {
    bytes = new Uint8Array(await own.readPdf(path));
  } catch (e) {
    return { status: String(e) === 'missing' || String(e) === 'refused' ? String(e) : 'damaged' };
  }
  const name = path.replace(/^.*[\\/]/, '');
  let got;
  try {
    got = await loadPdf(bytes, name);
  } catch {
    return { status: 'damaged' };
  }
  if (!got) {
    setTimeout(() => shell.setState(''), 0);
    return { status: 'abandoned' };
  }
  const doc = got.doc,
    out = [];
  try {
    for (let n = 1; n <= doc.numPages; n++) {
      const v = (await doc.getPage(n)).getViewport({ scale: 1 });
      out.push({ n, w: v.width, h: v.height, vt: v.transform });
    }
  } catch {
    return { status: 'damaged' };
  }
  if (!out.length) return { status: 'damaged' };
  const r = await own.adoptPdf(path, got.password);
  pending = { doc, sizes: out, r, password: got.password };
  return r;
}
async function hold(p) {
  closeAll();
  setTool('');
  if (pagesMode) {
    pagesMode = false;
    root.classList.remove('pages-mode');
    pagesBtn.setAttribute('aria-pressed', 'false');
  }
  restartPump();
  forgetPages();
  for (const d of docs.values()) if (d !== p.doc) d.destroy();
  docs = new Map([[0, p.doc]]);
  rasterCache = new Map();
  pictureCache = null;
  own.dropStash();
  values = {};
  picked = null;
  findHits = [];
  pickedPages = new Set();
  password = p.password;
  void password;
  rights = { ...ALL, ...(p.r.rights || {}) };
  pages = p.sizes.map((s) => makePage({ doc: p.doc, n: s.n, sid: 0 }, s.w, s.h, s.vt));
  fileName = p.r.name;
  fileSize = p.r.size;
  opened = true;
  const m = await p.doc.getMetadata().catch(() => ({ info: {} })),
    info = m.info || {};
  meta_ = {
    title: info.Title,
    author: info.Author,
    producer: info.Producer,
    created: pdfDate(info.CreationDate),
    version: info.PDFFormatVersion,
  };
  outline = await readOutline(p.doc).catch(() => []);
  syncCount();
  zoom = fitZ();
  fitted = true;
  setList();
  pages.forEach(drawMarks);
  const back = +(store.get(posKey()) || 0);
  nowAt = clamp(back, 0, pages.length - 1);
  stage.scrollTop = nowAt ? pages[nowAt].el.offsetTop - topGap() : 0;
  stage.scrollLeft = 0;
  history = [snap()];
  hAt = 0;
  savedSnap = history[0];
  syncHist();
  syncPageNo();
  syncPct();
  drawToc();
  syncPicked();
  syncRights();
  shell.changed();
  navPrev.hidden = navNext.hidden = false;
  root.classList.add('has-doc');
  if (!reduce) list.animate([{ opacity: 0 }, { opacity: 1 }], MOVE);
}
function goEmpty() {
  closeAll();
  setTool('');
  if (pagesMode) {
    pagesMode = false;
    root.classList.remove('pages-mode');
    pagesBtn.setAttribute('aria-pressed', 'false');
  }
  restartPump();
  forgetPages();
  for (const d of docs.values()) d.destroy();
  root.classList.remove('has-doc');
  docs = new Map();
  pages = [];
  list.replaceChildren();
  opened = false;
  fileName = '';
  outline = [];
  rights = ALL;
  values = {};
  hideRights();
  history = [];
  hAt = 0;
  savedSnap = null;
  rasterCache = new Map();
  pictureCache = null;
}

// ---------- the password: the family's question box, with Text's underline field and its caret ----------
const pwWrap = el('div', 'ask-wrap pw-wrap');
pwWrap.setAttribute('role', 'alertdialog');
pwWrap.setAttribute('aria-modal', 'true');
pwWrap.id = 'passwordAsk';
const pwBox = el('div', 'ask'),
  pwTitle = el('p', 'ask-title dyn'),
  pwNote = el('p', 'pw-note dyn'),
  pwField = el('span', 'fi pw-in'),
  pwIn = document.createElement('input');
pwIn.type = 'password';
pwIn.id = 'passwordText';
pwIn.autocomplete = 'off';
pwIn.spellcheck = false;
pwField.appendChild(pwIn);
const pwRow = el('div', 'ask-row'),
  pwCancel = el('button', 'btn soft', '取消'),
  pwOk = el('button', 'btn primary', '解锁');
pwCancel.type = pwOk.type = 'button';
pwCancel.id = 'passwordCancel';
pwOk.id = 'passwordOpen';
pwRow.append(el('i', 'ask-gap'), pwCancel, pwOk);
pwBox.append(pwTitle, pwField, pwNote, pwRow);
pwWrap.appendChild(pwBox);
shell.translates(pwWrap);
let pwAnswer = null;
function askPassword(name, wrong) {
  return new Promise((ok) => {
    pwAnswer = ok;
    pwTitle.textContent = T('此 PDF 受密码保护');
    pwIn.placeholder = T('访问密码');
    pwNote.textContent = wrong ? T('密码不正确，请重新输入。') : '';
    pwNote.classList.toggle('bad', wrong);
    // in the page only while it asks, so it is never mistaken for the family's own question box
    pwIn.value = '';
    document.body.appendChild(pwWrap);
    void pwWrap.offsetWidth;
    pwWrap.classList.add('on');
    setTimeout(() => pwIn.focus(), 30);
  });
}
const pwDone = (v) => {
  pwWrap.classList.remove('on');
  setTimeout(() => {
    if (!pwWrap.classList.contains('on')) pwWrap.remove();
  }, 300);
  const f = pwAnswer;
  pwAnswer = null;
  if (f) f(v);
};
pwCancel.addEventListener('click', () => {
  sound();
  pwDone(null);
});
pwOk.addEventListener('click', () => {
  sound();
  pwDone(pwIn.value);
});
pwIn.addEventListener('keydown', (e) => {
  e.stopPropagation();
  if (e.key === 'Enter') {
    e.preventDefault();
    sound();
    pwDone(pwIn.value);
  } else if (e.key === 'Escape') {
    sound();
    pwDone(null);
  }
});
pwWrap.addEventListener('keydown', (e) => e.stopPropagation());

// ---------- a file that limits what can be done: the family's notice row ----------
const limits = shell.notice('该文档设有权限保护，部分编辑与导出功能已锁定。', [['知道了', null]], {
  icon: 'info',
  id: 'limitNotice',
});
const showRights = () => limits.show(),
  hideRights = () => limits.hide();
function syncRights() {
  toolBtns.forEach((b) => {
    b.disabled = !allowed(b.dataset.k);
  });
  pages.forEach((p) => (p.fieldsInfo || []).forEach((f) => f.sync()));
  ['docx', 'png', 'jpg'].forEach((k) => {
    const b = document.getElementById('fmt-' + k);
    if (b) b.hidden = !rights.copy;
  });
  syncPicked();
  const limited = !rights.modify || !rights.copy || !rights.annotate || !rights.fill || !rights.assemble;
  if (limited) showRights();
  else hideRights();
}

// ---------- keys ----------
function keyDown(e) {
  const mod = modKey(e),
    k = e.key,
    typing = /INPUT|TEXTAREA/.test(document.activeElement.tagName) && document.activeElement.type !== 'range';
  // the browser's own find, print and reload are the family's to stop; here they do nothing either
  if (k === 'F5' || (mod && ['p', 'r'].includes(k.toLowerCase()))) return;
  if (!opened) {
    if (mod && k.toLowerCase() === 'f') e.preventDefault();
    return;
  }
  if (mod && k.toLowerCase() === 'f') {
    e.preventDefault();
    openFind();
    return;
  }
  if (k === 'Escape') {
    if (editing) {
      finishEdit();
      return;
    }
    if (selBar.classList.contains('on')) {
      hideSelBar();
      return;
    }
    if (closeAll()) return;
    if (tool) {
      setTool('');
      return;
    }
    if (pagesMode) leavePages();
    return;
  }
  if (typing) return;
  if (mod && k.toLowerCase() === 'z') {
    e.preventDefault();
    if (e.shiftKey) redo();
    else undo();
    return;
  }
  if (mod && k.toLowerCase() === 'y' && yRedoes) {
    e.preventDefault();
    redo();
    return;
  }
  if (mod && k.toLowerCase() === 'a' && pagesMode) {
    e.preventDefault();
    pickedPages = new Set(pages);
    syncPicked();
    return;
  }
  if (mod && (k === '=' || k === '+')) {
    e.preventDefault();
    zoomBy(1.25);
    return;
  }
  if (mod && k === '-') {
    e.preventDefault();
    zoomBy(1 / 1.25);
    return;
  }
  if (mod && k === '0') {
    e.preventDefault();
    fitNow();
    return;
  }
  if (mod && k.toLowerCase() === 'c' && !rights.copy) {
    e.preventDefault();
    return;
  }
  if (k === 'Delete' || k === 'Backspace') {
    if (picked) {
      removePicked();
      return;
    }
    if (pagesMode) {
      deletePicked();
      return;
    }
  }
  if (pagesMode) return;
  // the Mac's ⌘↑ ⌘↓: the first and the last page, as Home and End (fn ← →) and Preview
  const edge = macEdge(e);
  if (edge) {
    e.preventDefault();
    goPage(edge === 'end' ? pages.length - 1 : 0);
  } else if (k === 'PageDown') {
    e.preventDefault();
    goPage(nowAt + 1);
  } else if (k === 'PageUp') {
    e.preventDefault();
    goPage(nowAt - 1);
  } else if (k === 'Home') {
    e.preventDefault();
    goPage(0);
  } else if (k === 'End') {
    e.preventDefault();
    goPage(pages.length - 1);
  }
}

// ---------- settings: one row of its own, on the family's Appearance page ----------
shell.settingRow('appearance', {
  id: 'dimPaper',
  title: '页面随深色模式变暗',
  note: '页面文字与线条随深色模式调暗以缓解眼部疲劳，文档内的图片保持原色。',
  get: () => dimPaper,
  set: (on) => {
    dimPaper = on;
    store.set('dotany-pdf-dim', dimPaper ? 'on' : null);
    repaintPaper();
  },
});
function repaintPaper() {
  root.classList.toggle('dim-paper', nightDim());
  pages.forEach((p) => {
    p.canvasKey = '';
  });
  thumbed.forEach(standIn);
  pump();
}
new MutationObserver(repaintPaper).observe(root, { attributes: true, attributeFilter: ['data-theme'] });
matchMedia('(prefers-color-scheme: dark)').addEventListener('change', repaintPaper);

// ---------- the language: what's drawn here is drawn again ----------
shell.onLang(() => {
  findIn.placeholder = T('查找');
  // the add tile sits among the pages, not in a part that translates itself
  addBlank.querySelector('span').textContent = T('插入空白页');
  addFile.textContent = T('从文件导入页面');
  if (opened) {
    syncCount();
    drawToc();
    syncPicked();
    renderPanel();
  }
  if (pwWrap.classList.contains('on')) {
    pwTitle.textContent = T('此 PDF 受密码保护');
    pwIn.placeholder = T('访问密码');
  }
  requestAnimationFrame(syncSeg);
});

// ---------- begin ----------
placeTools();
repaintPaper();
if (document.fonts && document.fonts.ready) document.fonts.ready.then(() => syncSeg());
// with DOTANY_TEST set, shell.test exposes the open document and its actions, so that an automated test can read and drive it
// getters stay getters: what a test reads is always what is open now
Object.defineProperties(
  shell.test,
  Object.getOwnPropertyDescriptors({
    get pages() {
      return pages;
    },
    get now() {
      return nowAt;
    },
    get zoom() {
      return zoom;
    },
    get mode() {
      return pagesMode;
    },
    get picked() {
      return pickedPages;
    },
    get hits() {
      return findHits;
    },
    get at() {
      return findAt;
    },
    get values() {
      return values;
    },
    get tool() {
      return tool;
    },
    get rights() {
      return rights;
    },
    get opened() {
      return opened;
    },
    goPage,
    setTool,
    enterPages,
    leavePages,
    readBlocks,
    blockLines,
    commit,
    drawMarks,
    openBlock,
    finishBlock,
    markFor,
    editMark,
    addPaths,
    pick: (ns) => {
      pickedPages = new Set(ns.map((n) => pages[n]));
      syncPicked();
    },
    rotatePicked,
    deletePicked,
    openExtract,
    async plan() {
      return planOf(pages);
    },
    // a change, as a person's highlight would make one
    touch() {
      const pg = pages[0];
      pg.marks.push({ t: 'hl', color: 'green', rects: [[pg.w * 0.1, pg.h * 0.1, pg.w * 0.2, 12]] });
      drawMarks(pg);
      commit();
    },
    memory: () => own.memory(),
    get history() {
      return hAt;
    },
  })
);
shell.begin();
