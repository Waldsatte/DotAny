// DotAny Image's page: the window, the picture and the tools. The pixels come from the Rust side and never change here;
// turning, cropping, the sliders and drawing are kept as a recipe, which Rust repeats at full size when saving.
import '@dotany/web/fonts.js';
import { inkfall } from '@dotany/web/inkfall-input.js';
import { LUCIDE } from './icons.js';
import { el, root, reduce, sound, store, tool as family, LUCIDE as FAMILY_ICONS } from '@dotany/web/shared.js';
import { T, money } from './i18n.js';
import { keyLabel, modKey, yRedoes } from '@dotany/web/keys.js';
import { mac, wheelSource, quarterTurns, pinch } from '@dotany/web/gestures.js';
import { createShell } from '@dotany/web/shell.js';
import * as be from './backend.js';
import { initRender, setPixels, setLut, draw, adjustSmall, usesGpu } from './render.js';

// the family's parts draw with Image's icons and say Image's name
family.name = 'DotAny Image';
Object.assign(FAMILY_ICONS, LUCIDE);

const t0 = performance.now();
const boot = window.__DOTANY__ || {};
const icon = (n) => `<svg class="i" viewBox="0 0 24 24" aria-hidden="true">${LUCIDE[n]}</svg>`;
const clamp = (v, a, b) => Math.min(b, Math.max(a, v));
const afterFrames = () => new Promise((ok) => requestAnimationFrame(() => requestAnimationFrame(ok)));

const FORMAT = {
  bmp: { ext: '.bmp', label: 'BMP', raw: true },
  tiff: { ext: '.tiff', label: 'TIFF', raw: true },
  jpg: { ext: '.jpg', label: 'JPG', lossy: true },
  png: { ext: '.png', label: 'PNG' },
  webp: { ext: '.webp', label: 'WebP', lossy: true, lossless: true },
  gif: { ext: '.gif', label: 'GIF', keep: true },
  svg: { ext: '.svg', label: 'SVG', keep: true },
  ico: { ext: '.ico', label: 'ICO', keep: true },
};
// formats Image only reads (TGA, QOI, Netpbm, farbfeld, PCX, ILBM): saved as one it writes, PNG first, as nothing is lost
const READ_ONLY = ['tga', 'qoi', 'pbm', 'pgm', 'ppm', 'pam', 'ff', 'pcx', 'ilbm'];
const formatsFor = (k) =>
  k === 'webp' && s.moving
    ? ['webp', 'gif', 'png']
    : READ_ONLY.includes(k)
      ? ['png', 'jpg', 'webp']
      : {
          gif: ['gif', 'webp', 'png'],
          svg: ['svg', 'png'],
          ico: ['ico', 'png'],
          tiff: ['tiff', 'png', 'jpg', 'webp'],
          bmp: ['bmp', 'png', 'jpg', 'webp'],
        }[k] || ['jpg', 'png', 'webp'];
const CARRIES_DETAILS = ['jpg', 'png', 'webp', 'tiff'];
const NOTES = {
  turned: '仅更新方向元数据，不重新编码像素，画质零损耗。',
  'jpg-alpha': 'JPEG 不支持透明通道，透明区域将填充为白色。',
  'one-frame': '目标格式为静态图像，仅导出当前画面帧。',
  'one-page': '目标格式不支持多页，仅导出当前页。',
  'one-size': '目标格式仅支持单一尺寸，将按当前分辨率导出。',
  'gif-colors': 'GIF 格式上限为 256 色，导出后可能出现色彩断层。',
  'eight-bit': '原图色深超过 8-bit，导出后将收敛为每通道 8-bit。',
  'no-details': '目标格式不支持保存拍摄元数据（EXIF）。',
  'to-srgb': '目标格式不支持内嵌色彩配置文件，色彩空间将转换为标准 sRGB。',
};
// ---------- the window: the family's (shared/web/shell.js), with Image's own words, formats and buttons ----------
const PASTED = '\u0000pasted';
const shell = createShell({
  name: 'DotAny Image',
  icons: LUCIDE,
  notices: () => import('./notices.generated.js'),
  dropTitle: '拖拽图像至此',
  openLabel: '打开本地图像',
  fileKind: '图像',
  licences: [['libwebp', '编解码器', 'BSD-3-Clause']],
  count: true,
  print: () => printFile(),
  money,
  formats: FORMAT,
  formatsFor: () => formatsFor(file.kind),
  formatOf: () => file.kind,
  liveNote: true,
  cardScrolls: true,
  dimWhileSaving: true,
  cantOpen: { toolarge: '图像分辨率过高，当前可用内存不足以载入。' },
  failWords: { toolarge: '图像尺寸超出该格式的上限，未能保存。当前编辑仍完整保留在此窗口中。' },
  open: openFile,
  clear: closeFile,
  dirty: () => dirty(),
  discard() {
    file.slots.forEach((o) => {
      o.savedAt = o.at;
    });
  },
  beforeGuard() {
    commitText();
    if (tool === 'crop') finishCrop(false);
  },
  beforeSaveAs: () => cardOpens(),
  saveRows: (box, fmt, est) => {
    rowsBox = box;
    fmtKey = fmt;
    reEstimate = est;
    drawRows();
  },
  estimate: (fmt) => estimateSize(fmt),
  saveLine: (fmt) => saveLine(fmt),
  save: (path, fmt) => saveTo(path, fmt),
  saved: (r) => afterSave(r),
  keys: (e) => keys(e),
});
const { app, chrome, head } = shell;
const infoBtn = shell.headButton('infoBtn', 'info', '信息');
infoBtn.setAttribute('aria-pressed', 'false');

const seg = el('div', 'tools'),
  segPill = el('i', 'tool-pl');
seg.setAttribute('role', 'toolbar');
seg.appendChild(segPill);
const TOOLS = [
  ['crop', 'crop', '裁剪'],
  ['adjust', 'adjust', '调整'],
  ['brush', 'brush', '标注'],
  ['text', 'type', '文本'],
];
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
const barBtn = (id, name, tip, run) => {
  const b = el('button', 'tb');
  b.type = 'button';
  b.id = id;
  b.innerHTML = icon(name);
  b.setAttribute('aria-label', tip);
  b.dataset.tip = tip;
  b.addEventListener('mousedown', (e) => e.preventDefault());
  b.addEventListener('click', () => {
    sound();
    run();
  });
  return b;
};
const group = (items) => {
  const g = el('div', 'grp');
  g.append(...items);
  return g;
};
const undoBtn = barBtn('undo', 'undo', '撤销 · {Mod+Z}', () => undo()),
  redoBtn = barBtn('redo', 'redo', '重做 · {Mod+Y}', () => redo());
const geoGroup = group([
    barBtn('rotL', 'rotateL', '向左旋转', () => rotate(-1)),
    barBtn('rotR', 'rotateR', '向右旋转', () => rotate(1)),
    barBtn('flipH', 'flipH', '水平翻转', () => flip(true)),
    barBtn('flipV', 'flipV', '垂直翻转', () => flip(false)),
  ]),
  histGroup = group([undoBtn, redoBtn]);
geoGroup.classList.add('edit-only');
histGroup.classList.add('edit-only');

// ---------- the picture ----------
const stage = shell.stage,
  photo = el('div', 'photo'),
  pic = el('div', 'pic');
const base = el('canvas'),
  marks = el('canvas'),
  svgImg = el('img'),
  detail = el('canvas', 'detail'),
  detailMarks = el('canvas', 'detail');
svgImg.alt = '';
svgImg.hidden = true;
base.id = 'base';
marks.id = 'marks';
detail.id = 'detail';
pic.append(base, svgImg, marks, detail, detailMarks);
photo.appendChild(pic);
stage.appendChild(photo);
const cropBox = el('div', 'crop');
['t1', 't2', 't3', 't4'].forEach((t) => cropBox.appendChild(el('i', 'third ' + t)));
['nw', 'n', 'ne', 'e', 'se', 's', 'sw', 'w'].forEach((h) => {
  const d = el('i', 'hd ' + h);
  d.dataset.h = h;
  cropBox.appendChild(d);
});
stage.appendChild(cropBox);
const txtBox = el('div', 'txt-box');
stage.appendChild(txtBox);
const side = el('aside', 'side'),
  pill = el('div', 'pill'),
  pillExtra = el('div', 'pill-extra'),
  rail = el('div', 'rail');
const ring = el('div', 'ring');
const pillBtn = (id, name, label, run) => {
  const b = el('button', 'tb');
  b.type = 'button';
  b.id = id;
  b.innerHTML = icon(name);
  b.setAttribute('aria-label', label);
  b.addEventListener('click', () => {
    sound();
    run();
  });
  return b;
};
const pct = el('button', 'tb pct dyn', '100%');
pct.type = 'button';
pct.id = 'zoomFit';
pct.addEventListener('click', () => {
  sound();
  if (fitted) zoomTo(1);
  else fitView();
});
const editSep = () => el('i', 'sep edit-only');
const sepTools = editSep();
sepTools.classList.add('sep-tools');
const zoomIsle = el('div', 'grp zoom-grp');
zoomIsle.append(
  pillBtn('zoomOut', 'minus', 'Zoom out', () => zoomBy(1 / 1.4)),
  pct,
  pillBtn('zoomIn', 'plus', 'Zoom in', () => zoomBy(1.4))
);
const sepExtra = el('i', 'sep sep-extra');
pill.append(seg, sepTools, geoGroup, editSep(), histGroup, sepExtra, pillExtra, editSep(), zoomIsle);
// the arrows stand just outside the dock, each its own small sheet of glass: always in sight, always level with each other
const navPrev = barBtn('prevPic', 'prev', '上一张', () => step(-1)),
  navNext = barBtn('nextPic', 'next', '下一张', () => step(1));
const navParts = [navPrev, navNext],
  dockRow = el('div', 'dock-row');
navParts.forEach((p) => p.classList.add('pic-nav'));
dockRow.append(navPrev, pill, navNext);
shell.place(side, rail, dockRow);
document.body.append(ring);

// ---------- what is open ----------
// one file holds one or more slots (the pages of a TIFF, the sizes of an ICO); each keeps its own edits and undo.
// The pixels live on the Rust side and never change. A slot here is a recipe: which way round, what's cropped, the sliders, what's drawn.
let file = null,
  slotAt = 0,
  s = null,
  tool = '';
const zeroAdj = () => ({ exposure: 0, contrast: 0, saturation: 0, temperature: 0, highlights: 0, shadows: 0 });
const ADJ = [
  ['exposure', '曝光'],
  ['contrast', '对比度'],
  ['saturation', '饱和度'],
  ['temperature', '色温'],
  ['highlights', '高光'],
  ['shadows', '阴影'],
];
const NO_TURN = [1, 0, 0, 1];
const snap = () => ({ geo: s.geo.slice(), crop: s.crop && { ...s.crop }, adj: { ...s.adj }, items: s.items.slice() });
function newSlot(info) {
  const slot = {
    w: info.w,
    h: info.h,
    alpha: info.alpha,
    moving: info.moving,
    geo: NO_TURN.slice(),
    crop: null,
    adj: zeroAdj(),
    items: [],
    hist: [],
    at: 0,
    savedAt: 0,
    pw: 0,
    ph: 0,
    small: null,
  };
  slot.hist.push({ geo: slot.geo.slice(), crop: null, adj: zeroAdj(), items: [] });
  return slot;
}
const CAPS = { jpg: 'all', png: 'all', webp: 'all', bmp: 'all', tiff: 'all', ico: 'all', gif: 'crop', svg: 'none' };
const curW = (o = s) => (o.geo[0] === 0 ? o.h : o.w),
  curH = (o = s) => (o.geo[0] === 0 ? o.w : o.h);
const isDirty = (slots) => slots.some((o) => o.at !== o.savedAt);
const dirty = () => !!file && isDirty(file.slots);

// ---------- pixels, as much as the window needs ----------
const SMALL_SIDE = 512;
let renderSeq = 0;
const previewSide = () => (usesGpu() ? 4096 : 2048);
// a picture up to this size is shown pixel for pixel; a larger one as a smaller copy, sharpened where it is zoomed into
const FULL_PIXELS = 16e6;
const previewMax = (o) => {
  const long = Math.max(o.w, o.h);
  return usesGpu() && o.w * o.h <= FULL_PIXELS && long <= 8192 ? long : Math.min(long, previewSide());
};
const QUICK_SIDE = 2048;
const FRAME_SIDE = 1600;

async function showSlot(i, firstPaint) {
  const seq = ++renderSeq,
    o = file.slots[i],
    gen = file.gen;
  hideDetail();
  if (file.svg) return;
  if (o.moving) {
    await loadFrame(0, seq);
    return;
  }
  const want = previewMax(o),
    long = Math.max(o.w, o.h);
  // the window fills first; the full preview arrives a moment later
  if (want > QUICK_SIDE) {
    const q = await be.viewPixels(gen, i, 0, Math.min(QUICK_SIDE, long));
    if (!q || seq !== renderSeq) return;
    o.pw = q.w;
    o.ph = q.h;
    setPixels(q.data, q.w, q.h);
    drawAll();
    if (firstPaint) firstPaint();
    // the small copy reaches the screen before the large one takes the graphics card's attention
    await afterFrames();
    await new Promise((ok) => setTimeout(ok, firstPaint ? 120 : 0));
    if (seq !== renderSeq) return;
    const full = await be.viewPixels(gen, i, 0, want);
    if (!full || seq !== renderSeq) return;
    o.pw = full.w;
    o.ph = full.h;
    setPixels(full.data, full.w, full.h);
    drawAll();
    performance.mark('sharp');
    scheduleDetail();
    return;
  }
  const v = await be.viewPixels(gen, i, 0, want);
  if (!v || seq !== renderSeq) return;
  o.pw = v.w;
  o.ph = v.h;
  setPixels(v.data, v.w, v.h);
  drawAll();
  if (firstPaint) firstPaint();
}

// the small copy the film strip is made from
async function smallOf(o, i) {
  if (o.small) return o.small;
  const v = await be.viewPixels(file.gen, i, 0, SMALL_SIDE);
  if (!v) return null;
  const c = el('canvas');
  c.width = v.w;
  c.height = v.h;
  c.getContext('2d').putImageData(
    new ImageData(new Uint8ClampedArray(v.data.buffer, v.data.byteOffset, v.w * v.h * 4), v.w, v.h),
    0,
    0
  );
  o.small = c;
  return c;
}

function loadFile(f, firstPaint) {
  stopPlay();
  file = f;
  slotAt = 0;
  s = f.slots[0];
  root.classList.remove('is-empty');
  closePanelTool();
  const caps = s.moving ? 'crop' : CAPS[f.kind] || (READ_ONLY.includes(f.kind) ? 'all' : undefined);
  root.classList.toggle('bar-none', caps === 'none');
  placeTools();
  root.classList.toggle('caps-crop', caps === 'crop');
  infoBtn.setAttribute('aria-pressed', 'false');
  renderPanel();
  photo.classList.toggle('alpha', s.alpha);
  svgImg.hidden = !f.svg;
  base.hidden = !!f.svg;
  if (f.svg) {
    svgImg.src = 'data:image/svg+xml;charset=utf-8,' + encodeURIComponent(f.svg);
    svgImg.style.width = s.w + 'px';
    svgImg.style.height = s.h + 'px';
  } else svgImg.removeAttribute('src');
  setLut(null);
  if (f.wide)
    be.lutTable(f.gen).then((t) => {
      if (file === f && t) {
        setLut(t.data);
        drawAll();
      }
    });
  placed = '';
  syncSeg();
  frameAt = 0;
  frameCache.clear();
  frameBytes = 0;
  frameCount = s.moving ? 2 : 0;
  frameDelays = [];
  frameDone = !s.moving;
  layout();
  drawPill();
  syncTop();
  fitView(true);
  if (!reduce) photo.animate([{ opacity: 0 }, { opacity: 1 }], { duration: 240, easing: 'cubic-bezier(.2,.7,.2,1)' });
  const painted = showSlot(0, firstPaint);
  if (f.svg && firstPaint) {
    svgImg
      .decode()
      .catch(() => {})
      .then(firstPaint);
  }
  if (s.moving) {
    playing = true;
    pollFrames(f);
    painted.then(() => {
      if (firstPaint) firstPaint();
      if (playing && file === f) scheduleTick();
      drawPill();
    });
  }
  return painted;
}
function setSlot(i) {
  commitText();
  finishCrop(true);
  slotAt = i;
  s = file.slots[i];
  photo.classList.toggle('alpha', s.alpha);
  layout();
  drawPill();
  syncTop();
  if (tool) renderPanel();
  fitView();
  showSlot(i);
}

// ---------- painting ----------
function drawBase(plain) {
  if (file.svg || !s.pw) return;
  draw(s.geo, s.adj, plain);
  if (detail.classList.contains('on')) {
    hideDetail();
  }
  if (!plain) scheduleDetail();
}
const FONT_STACK = '"Stack Sans Text", "HarmonyOS Sans SC", "Microsoft YaHei UI", system-ui, sans-serif';
const textFont = (it) => `${it.bold ? 600 : 400} ${it.size}px ${FONT_STACK}`;
function paintItem(g, it) {
  if (it === editing) return;
  if (it.t === 'brush') {
    const q = it.pts;
    g.strokeStyle = it.color;
    g.lineWidth = it.size;
    g.lineCap = 'round';
    g.lineJoin = 'round';
    g.beginPath();
    g.moveTo(q[0], q[1]);
    if (q.length === 2) g.lineTo(q[0] + 0.01, q[1]);
    for (let i = 2; i + 3 < q.length; i += 2)
      g.quadraticCurveTo(q[i], q[i + 1], (q[i] + q[i + 2]) / 2, (q[i + 1] + q[i + 3]) / 2);
    if (q.length > 2) g.lineTo(q[q.length - 2], q[q.length - 1]);
    g.stroke();
  } else {
    g.fillStyle = it.color;
    g.font = textFont(it);
    g.textBaseline = 'middle';
    g.textRendering = 'geometricPrecision';
    g.fillText(it.text, it.x, it.y + it.size * 0.6);
  }
}
function drawMarks() {
  if (!file || file.svg) return;
  const w = base.width || 1,
    h = base.height || 1,
    k = w / curW();
  if (marks.width !== w || marks.height !== h) {
    marks.width = w;
    marks.height = h;
  }
  // the same way of drawing as the pieces sent to a save, so edges come out the same
  const g = marks.getContext('2d', { willReadFrequently: true });
  g.setTransform(1, 0, 0, 1, 0, 0);
  g.clearRect(0, 0, w, h);
  g.setTransform(k, 0, 0, k, 0, 0);
  s.items.forEach((it) => paintItem(g, it));
  if (detailAt) paintDetailMarks();
}
function drawAll() {
  if (!file) return;
  if (!file.svg) {
    drawBase();
    drawMarks();
  }
  layout();
}
const full = () => ({ x: 0, y: 0, w: curW(), h: curH() });
const shown = () => (tool === 'crop' ? full() : s.crop || full());
function layout() {
  if (!s) return;
  const r = shown();
  photo.style.width = r.w + 'px';
  photo.style.height = r.h + 'px';
  pic.style.transform = `translate(${-r.x}px, ${-r.y}px)`;
  const W = curW(),
    H = curH();
  pic.style.width = W + 'px';
  pic.style.height = H + 'px';
  for (const c of [base, marks]) {
    c.style.width = W + 'px';
    c.style.height = H + 'px';
  }
}
function syncTop() {
  const r = s.crop || full();
  shell.setCount(`${r.w} × ${r.h} · ${money(file.size)}`);
  undoBtn.disabled = s.at <= 0;
  redoBtn.disabled = s.at >= s.hist.length - 1;
  undoBtn.style.opacity = s.at <= 0 ? 0.4 : '';
  redoBtn.style.opacity = s.at >= s.hist.length - 1 ? 0.4 : '';
  const alone = !file.path || file.beside < 2;
  if (navPrev.hidden !== alone) {
    navParts.forEach((p) => {
      p.hidden = alone;
      if (!alone && !reduce)
        p.animate([{ opacity: 0 }, { opacity: 1 }], { duration: 240, easing: 'cubic-bezier(.2,.7,.2,1)' });
    });
  }
}

// zoomed in past what the preview holds: Rust sends the part on screen, sharp, through the same arithmetic as a save
let detailTimer = 0,
  detailSeq = 0,
  detailAt = null;
function hideDetail() {
  detailSeq++;
  clearTimeout(detailTimer);
  detail.classList.remove('on');
  detailMarks.classList.remove('on');
  detailAt = null;
}
// The screen's pixels to a page pixel. On the Mac (Retina: two) the sharp part is fetched for the screen's own pixels,
// so a picture shown at its own size, or near it, is as sharp as in Preview; the file is saved at the picture's own
// pixels either way. On Windows it is one.
const screenPixels = () => (mac ? devicePixelRatio || 1 : 1);
function scheduleDetail() {
  clearTimeout(detailTimer);
  detailTimer = setTimeout(fetchDetail, 140);
}
async function fetchDetail() {
  if (!file || file.svg || s.moving || !s.pw || Math.max(s.pw, s.ph) >= Math.max(s.w, s.h)) {
    hideDetail();
    return;
  }
  const k = base.width / curW(),
    px = screenPixels();
  // only when the screen shows more than the preview holds
  if ((vz * px) / k <= 1.02) {
    hideDetail();
    return;
  }
  const a = toImg({ clientX: 0, clientY: 0 }),
    b = toImg({ clientX: innerWidth, clientY: innerHeight });
  const x = Math.floor(clamp(a.x, 0, curW())),
    y = Math.floor(clamp(a.y, 0, curH())),
    r = Math.ceil(clamp(b.x, 0, curW())),
    B = Math.ceil(clamp(b.y, 0, curH()));
  if (r <= x || B <= y) return;
  const rect = { x, y, w: r - x, h: B - y };
  const scale = Math.min(1, vz * px, Math.sqrt(16e6 / (rect.w * rect.h))),
    ow = Math.max(1, Math.round(rect.w * scale)),
    oh = Math.max(1, Math.round(rect.h * scale));
  const seq = ++detailSeq,
    adj = { ...s.adj };
  const got = await be.regionPixels(file.gen, slotAt, 0, rect, ow, oh, s.geo, adj);
  if (!got || seq !== detailSeq) return;
  detail.width = ow;
  detail.height = oh;
  detail
    .getContext('2d')
    .putImageData(
      new ImageData(new Uint8ClampedArray(got.data.buffer, got.data.byteOffset, ow * oh * 4), ow, oh),
      0,
      0
    );
  for (const c of [detail, detailMarks]) {
    Object.assign(c.style, { left: x + 'px', top: y + 'px', width: rect.w + 'px', height: rect.h + 'px' });
    c.classList.add('on');
  }
  detailAt = { rect, ow, oh };
  paintDetailMarks();
}
function paintDetailMarks() {
  const { rect, ow, oh } = detailAt,
    k = ow / rect.w;
  if (detailMarks.width !== ow || detailMarks.height !== oh) {
    detailMarks.width = ow;
    detailMarks.height = oh;
  }
  const g = detailMarks.getContext('2d', { willReadFrequently: true });
  g.setTransform(1, 0, 0, 1, 0, 0);
  g.clearRect(0, 0, ow, oh);
  g.setTransform(k, 0, 0, k, -rect.x * k, -rect.y * k);
  s.items.forEach((it) => paintItem(g, it));
}

// ---------- history ----------
// the family dot and its unsaved line follow every change, undo and redo
function noteDirty() {
  shell.changed();
  syncTop();
}
function commit() {
  s.hist.length = s.at + 1;
  s.hist.push(snap());
  s.at++;
  if (s.savedAt > s.at - 1) s.savedAt = -1;
  noteDirty();
}
function restore(h) {
  s.geo = h.geo.slice();
  s.crop = h.crop && { ...h.crop };
  s.adj = { ...h.adj };
  s.items = h.items.slice();
  drawAll();
  renderPanel();
  noteDirty();
  if (fitted) fitView();
}
function undo() {
  commitText();
  if (tool === 'crop') finishCrop(true);
  if (s.at > 0) restore(s.hist[--s.at]);
}
function redo() {
  commitText();
  if (tool === 'crop') finishCrop(true);
  if (s.at < s.hist.length - 1) restore(s.hist[++s.at]);
}

// ---------- turning and flipping: the picture turns, everything drawn on it moves with it ----------
const compose = (a, b) => [
  a[0] * b[0] + a[1] * b[2],
  a[0] * b[1] + a[1] * b[3],
  a[2] * b[0] + a[3] * b[2],
  a[2] * b[1] + a[3] * b[3],
];
// a small hint of the way it moved, around the picture's own centre
function settlePhoto(deg, sx, sy) {
  if (reduce) return;
  const r = shown(),
    at = (d, x, y) =>
      `translate(${vx}px, ${vy}px) rotate(${d}deg) scale(${vz * x}, ${vz * y}) translate(${-r.w / 2}px, ${-r.h / 2}px)`;
  photo.animate(
    [
      { transform: at(deg, sx, sy), opacity: 0.75 },
      { transform: at(0, 1, 1), opacity: 1 },
    ],
    { duration: 240, easing: 'cubic-bezier(.2,.7,.2,1)' }
  );
}
function rotate(dir) {
  if (!file || file.svg) return;
  commitText();
  const keepCrop = tool === 'crop';
  finishCrop(true);
  const W = curW(),
    H = curH(),
    tp = dir > 0 ? (x, y) => [H - y, x] : (x, y) => [y, W - x];
  s.geo = compose(dir > 0 ? [0, -1, 1, 0] : [0, 1, -1, 0], s.geo);
  if (s.crop) {
    const [a, b] = tp(s.crop.x, s.crop.y),
      [c, d] = tp(s.crop.x + s.crop.w, s.crop.y + s.crop.h);
    s.crop = { x: Math.min(a, c), y: Math.min(b, d), w: s.crop.h, h: s.crop.w };
  }
  s.items = s.items.map((it) => {
    if (it.t === 'brush') {
      const q = [];
      for (let i = 0; i < it.pts.length; i += 2) q.push(...tp(it.pts[i], it.pts[i + 1]));
      return { ...it, pts: q };
    }
    const [x, y] = tp(it.x, it.y);
    return { ...it, x, y };
  });
  commit();
  drawAll();
  if (tool === 'adjust') renderPanel();
  fitView(true);
  settlePhoto(dir * -10, 0.96, 0.96);
  if (keepCrop) startCrop();
}
function flip(h) {
  if (!file || file.svg) return;
  commitText();
  const keepCrop = tool === 'crop';
  finishCrop(true);
  const W = curW(),
    H = curH(),
    fp = (x, y) => (h ? [W - x, y] : [x, H - y]);
  s.geo = compose(h ? [-1, 0, 0, 1] : [1, 0, 0, -1], s.geo);
  if (s.crop) s.crop = h ? { ...s.crop, x: W - s.crop.x - s.crop.w } : { ...s.crop, y: H - s.crop.y - s.crop.h };
  s.items = s.items.map((it) => {
    if (it.t === 'brush') {
      const q = [];
      for (let i = 0; i < it.pts.length; i += 2) q.push(...fp(it.pts[i], it.pts[i + 1]));
      return { ...it, pts: q };
    }
    // words stay readable: only their place mirrors
    return h ? { ...it, x: W - it.x - measureText(it) } : { ...it, y: H - it.y - it.size * 1.2 };
  });
  commit();
  drawAll();
  if (tool === 'adjust') renderPanel();
  if (h) settlePhoto(0, 0.88, 1);
  else settlePhoto(0, 1, 0.88);
  if (keepCrop) startCrop();
}
const measureCtx = document.createElement('canvas').getContext('2d');
function measureText(it) {
  measureCtx.font = textFont(it);
  measureCtx.textRendering = 'geometricPrecision';
  return measureCtx.measureText(it.text || ' ').width;
}

// ---------- looking around: the picture glides to where it was asked to be ----------
let vx = 0,
  vy = 0,
  vz = 1,
  tx = 0,
  ty = 0,
  tz = 1,
  fitted = true,
  viewRaf = 0,
  lastT = 0;
function area() {
  const top = chrome.offsetHeight,
    wide = innerWidth > 720,
    open = side.classList.contains('on');
  const W = app.clientWidth,
    H = app.clientHeight;
  const sideH = open && !wide ? side.offsetHeight + 16 : 0;
  return {
    l: root.classList.contains('has-rail') ? rail.offsetLeft + rail.offsetWidth + 24 : 16,
    r: W - 16 - (open && wide ? 264 + 16 + 8 : 0),
    t: top + 16,
    b: H - (sideH ? sideH + 86 : 88),
  };
}
function fitView(now) {
  if (!file) return;
  const a = area(),
    r = shown(),
    w = a.r - a.l,
    h = a.b - a.t;
  const cap = file.svg ? 8 : file.kind === 'ico' ? Math.min(8, 256 / curW()) : 1;
  tz = Math.max(0.002, Math.min(w / r.w, h / r.h, cap));
  tx = (a.l + a.r) / 2;
  ty = (a.t + a.b) / 2;
  fitted = true;
  if (now || reduce) {
    vx = tx;
    vy = ty;
    vz = tz;
    applyView();
  } else kickView();
  syncPct();
}
function zoomAround(next, px, py) {
  const z = clamp(next, 0.002, 32),
    k = z / tz;
  tx = px + (tx - px) * k;
  ty = py + (ty - py) * k;
  tz = z;
  fitted = false;
  kickView();
  syncPct();
}
function zoomBy(f) {
  const a = area();
  zoomAround(tz * f, (a.l + a.r) / 2, (a.t + a.b) / 2);
}
function zoomTo(z) {
  const a = area();
  zoomAround(z, (a.l + a.r) / 2, (a.t + a.b) / 2);
}
function syncPct() {
  pct.textContent = Math.round(tz * 100) + '%';
  const a = area(),
    r = shown();
  root.classList.toggle('can-pan', !tool && (r.w * tz > a.r - a.l + 1 || r.h * tz > a.b - a.t + 1));
}
function kickView() {
  if (!viewRaf) {
    lastT = performance.now();
    viewRaf = requestAnimationFrame(viewFrame);
  }
}
function viewFrame(now) {
  viewRaf = 0;
  const k = 1 - Math.exp(-Math.min(64, now - lastT) / 70);
  lastT = now;
  vx += (tx - vx) * k;
  vy += (ty - vy) * k;
  vz += (tz - vz) * k;
  const done = Math.abs(tx - vx) < 0.3 && Math.abs(ty - vy) < 0.3 && Math.abs(tz - vz) < tz * 0.001;
  if (done) {
    vx = tx;
    vy = ty;
    vz = tz;
  }
  applyView();
  if (!done) viewRaf = requestAnimationFrame(viewFrame);
}
function applyView() {
  const r = shown();
  photo.style.transform = `translate(${vx - (r.w * vz) / 2}px, ${vy - (r.h * vz) / 2}px) scale(${vz})`;
  photo.style.setProperty('--z', vz);
  photo.classList.toggle('pixels', vz >= 2);
  placeOverlays();
  if (file && !file.svg) scheduleDetail();
}
const toImg = (e) => {
  const r = shown();
  return { x: (e.clientX - vx) / vz + r.w / 2 + r.x, y: (e.clientY - vy) / vz + r.h / 2 + r.y };
};
const toScreen = (x, y) => {
  const r = shown();
  return { x: vx + (x - r.x - r.w / 2) * vz, y: vy + (y - r.y - r.h / 2) * vz };
};
function placeOverlays() {
  if (tool === 'crop' && temp) {
    const a = toScreen(temp.x, temp.y);
    Object.assign(cropBox.style, {
      left: a.x + 'px',
      top: a.y + 'px',
      width: temp.w * vz + 'px',
      height: temp.h * vz + 'px',
    });
  }
  if (editing && txtEdit.isConnected) {
    const a = toScreen(editing.x, editing.y);
    Object.assign(txtEdit.style, { left: a.x + 'px', top: a.y + 'px', fontSize: editing.size * vz + 'px' });
  }
  const it = picked;
  txtBox.classList.toggle('on', !!it && !editing);
  if (it && !editing) {
    const a = toScreen(it.x, it.y),
      pad = 4;
    Object.assign(txtBox.style, {
      left: a.x - pad + 'px',
      top: a.y - pad + 'px',
      width: measureText(it) * vz + pad * 2 + 'px',
      height: it.size * 1.2 * vz + pad * 2 + 'px',
    });
  }
  ring.style.width = ring.style.height = brush.size * vz + 'px';
}

// ---------- the tools ----------
let temp = null,
  ratio = null,
  brush = { color: '#B3261E', size: 12 },
  words = { color: '#FFFFFF', size: 0, bold: false },
  editing = null,
  picked = null;
const SWATCHES = ['#FFFFFF', '#151413', '#75716B', '#B3261E', '#1F4FA8', '#2C6E49'];
function setTool(t, opts = {}) {
  if (!file) return;
  commitText();
  if (tool === 'crop' && t !== 'crop') finishCrop(!!opts.discard);
  picked = null;
  tool = t;
  root.classList.remove('tool-crop', 'tool-adjust', 'tool-brush', 'tool-text');
  if (t) root.classList.add('tool-' + t);
  toolBtns.forEach((b) => b.setAttribute('aria-pressed', String(b.dataset.k === t)));
  placed = '';
  syncSeg();
  if (t) infoBtn.setAttribute('aria-pressed', 'false');
  if (t === 'crop') startCrop();
  renderPanel();
  layout();
  fitView();
  applyView();
}
const closePanelTool = () => {
  if (tool) setTool('');
  else {
    side.classList.remove('on');
  }
};
let placed = '';
function syncSeg() {
  const on = toolBtns.find((b) => b.dataset.k === tool);
  segPill.style.opacity = on ? 1 : 0;
  const at = on ? [on.offsetLeft, on.offsetTop, on.offsetWidth, on.offsetHeight].join() : '';
  if (on && at !== placed) {
    placed = at;
    segPill.style.transform = `translate(${on.offsetLeft}px, ${on.offsetTop}px)`;
    segPill.style.width = on.offsetWidth + 'px';
    segPill.style.height = on.offsetHeight + 'px';
  }
}
// a slider
function slider({ id, label, min, max, value, center, unit = '', onInput, onChange, reset }) {
  const w = el('label', 'slide'),
    top = el('span', 'slide-top'),
    name = el('b', 'dyn', T(label)),
    val = el('span', 'dyn');
  top.append(name, val);
  const i = document.createElement('input');
  i.type = 'range';
  i.id = id;
  i.min = min;
  i.max = max;
  i.value = value;
  i.setAttribute('aria-label', T(label));
  const paint = () => {
    const v = +i.value,
      p = ((v - min) / (max - min)) * 100,
      c = center ? 50 : 0;
    i.style.setProperty('--a', Math.min(p, c) + '%');
    i.style.setProperty('--b', Math.max(p, c) + '%');
    val.textContent = (center && v > 0 ? '+' : '') + v + unit;
  };
  i.addEventListener('input', () => {
    paint();
    onInput(+i.value);
  });
  i.addEventListener('change', () => onChange && onChange(+i.value));
  w.addEventListener('dblclick', (e) => {
    e.preventDefault();
    if (reset == null) return;
    i.value = reset;
    paint();
    onInput(reset);
    if (onChange) onChange(reset);
  });
  w.append(top, i);
  paint();
  return {
    root: w,
    set(v) {
      i.value = v;
      paint();
    },
  };
}
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
function fromHsv(h, v_s, v) {
  const c = v * v_s,
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
function swatches(cur, pick) {
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
      pick(c, true);
    });
    fixed.push(b);
    row.appendChild(b);
  });
  const more = el('button', 'swatch custom');
  more.type = 'button';
  more.id = 'colorMore';
  more.setAttribute('aria-label', 'Custom color');
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
    pick(c, final);
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
      pick(customColor, true);
    }
  });
  wrap.append(row, box);
  setFrom(cur);
  mark(cur);
  return wrap;
}
const label = (t) => el('span', 'side-label dyn', T(t));
const softBtn = (id, text, cls = 'btn soft') => {
  const b = el('button', cls + ' dyn', T(text));
  b.type = 'button';
  b.id = id;
  b.addEventListener('mousedown', (e) => e.preventDefault());
  return b;
};
let panelFor = '';
function renderPanel() {
  const was = side.classList.contains('on'),
    info = infoBtn.getAttribute('aria-pressed') === 'true',
    now = file && (tool || info) ? (info ? 'info' : tool) : '';
  // redrawing the same panel changes its height at most; a different one fades in too
  if (was && now) {
    const same = now === panelFor;
    panelFor = now;
    morph(side, drawPanel, () => (same ? [] : [...side.children]));
    return;
  }
  panelFor = now;
  drawPanel();
}
function drawPanel() {
  side.textContent = '';
  const info = infoBtn.getAttribute('aria-pressed') === 'true',
    body = el('div', 'side-body');
  if (!file || (!tool && !info)) {
    side.classList.remove('on');
    edgeFix();
    return;
  }
  const titles = { crop: '裁剪', adjust: '调整', brush: '标注', text: '文本' };
  side.append(el('h3', 'dyn', T(info ? '信息' : titles[tool])), body);
  if (info) panelInfo(body);
  else if (tool === 'adjust') panelAdjust(body);
  else if (tool === 'crop') panelCrop(body);
  else if (tool === 'brush') panelBrush(body);
  else panelText(body);
  side.classList.add('on');
  edgeFix();
}
const edgeFix = () => {
  root.style.setProperty('--chrome-now', chrome.offsetHeight + 'px');
};
const PRESETS = [
  ['原图', {}],
  ['明亮', { exposure: 16, contrast: -4, shadows: 22 }],
  ['鲜明', { contrast: 16, saturation: 26 }],
  ['柔和', { contrast: -18, saturation: -8, highlights: -16, shadows: 18 }],
  ['暖调', { exposure: 4, saturation: 8, temperature: 32 }],
  ['冷调', { contrast: 6, saturation: -4, temperature: -30 }],
  ['胶片', { contrast: -10, saturation: -20, temperature: 10, highlights: -14, shadows: 16 }],
  ['黑白', { contrast: 14, saturation: -100 }],
].map(([name, v]) => [name, { ...zeroAdj(), ...v }]);
const sameAdj = (a, b) => Object.keys(a).every((k) => a[k] === b[k]);
// the small pictures are made once for each state of the picture
let thumbKey = '',
  thumbs = [];
function presetThumbs(src) {
  const key = [file.gen, slotAt, s.geo.join(), s.crop ? [s.crop.x, s.crop.y, s.crop.w, s.crop.h].join() : ''].join('|');
  if (thumbKey === key) return thumbs;
  thumbKey = key;
  // the small copy, turned and cropped as the picture is
  const k = (s.geo[0] === 0 ? src.height : src.width) / curW();
  const r = s.crop || full(),
    side_ = Math.min(r.w, r.h) * k,
    small = el('canvas');
  small.width = small.height = 104;
  const turned = el('canvas');
  turned.width = Math.max(1, Math.round(curW() * k));
  turned.height = Math.max(1, Math.round(curH() * k));
  const tg = turned.getContext('2d');
  tg.setTransform(s.geo[0], s.geo[2], s.geo[1], s.geo[3], turned.width / 2, turned.height / 2);
  tg.drawImage(src, -src.width / 2, -src.height / 2);
  small
    .getContext('2d')
    .drawImage(turned, (r.x + r.w / 2) * k - side_ / 2, (r.y + r.h / 2) * k - side_ / 2, side_, side_, 0, 0, 104, 104);
  thumbs = PRESETS.map(([, v]) => adjustSmall(small, v));
  return thumbs;
}
// choosing one eases the picture and the sliders there together, and counts as one step
let tween = null,
  adjSliders = {},
  presetBtns = [];
let presetRing = null,
  presetTrack = null,
  presetLabel = null,
  foldNote = null,
  presetAt = 0;
function moveStrip(x) {
  presetTrack.style.transform = `translateX(${x}px)`;
}
const stripX = (i) => {
  const b = presetBtns[i];
  return presetTrack.parentElement.offsetWidth / 2 - b.offsetLeft - b.offsetWidth / 2;
};
function markPreset(i) {
  presetBtns.forEach((b, n) => b.setAttribute('aria-pressed', String(n === i)));
  if (!presetRing) return;
  presetRing.classList.toggle('on', i >= 0);
  const name = T(i >= 0 ? PRESETS[i][0] : '自定义');
  if (presetLabel.textContent !== name) {
    presetLabel.textContent = name;
    if (!reduce)
      presetLabel.animate([{ opacity: 0 }, { opacity: 1 }], { duration: 240, easing: 'cubic-bezier(.2,.7,.2,1)' });
  }
  if (i < 0) return;
  presetAt = i;
  moveStrip(stripX(i));
}
function syncPresets() {
  markPreset(PRESETS.findIndex((p) => sameAdj(s.adj, p[1])));
  if (foldNote)
    foldNote.classList.toggle(
      'on',
      FOLDED.some((k) => s.adj[k])
    );
}
function finishTween() {
  if (!tween) return;
  const { to, raf } = tween;
  tween = null;
  cancelAnimationFrame(raf);
  s.adj = { ...to };
  ADJ.forEach(([k]) => adjSliders[k] && adjSliders[k].set(s.adj[k]));
  drawBase();
  syncPresets();
  commit();
}
function choosePreset(to) {
  finishTween();
  if (sameAdj(s.adj, to)) return;
  const from = { ...s.adj };
  tween = { to, raf: 0 };
  markPreset(PRESETS.findIndex((p) => p[1] === to));
  if (reduce) {
    finishTween();
    return;
  }
  const start = performance.now();
  const frame = (now) => {
    if (!tween) return;
    const t = Math.min(1, (now - start) / 240);
    if (t >= 1) {
      finishTween();
      return;
    }
    const k = 1 - (1 - t) ** 3;
    Object.keys(to).forEach((a) => {
      s.adj[a] = Math.round(from[a] + (to[a] - from[a]) * k);
      if (adjSliders[a]) adjSliders[a].set(s.adj[a]);
    });
    drawBase();
    tween.raf = requestAnimationFrame(frame);
  };
  tween.raf = requestAnimationFrame(frame);
}
// the first three are what people reach for; the other three wait behind one line
const FOLDED = ['temperature', 'highlights', 'shadows'];
let foldOpen = false;
let stripCleanup = null;
function panelAdjust(body) {
  if (stripCleanup) stripCleanup();
  const strip = el('div', 'strip'),
    track = el('div', 'strip-track');
  presetRing = el('i', 'strip-frame');
  presetTrack = track;
  presetLabel = el('p', 'strip-name dyn');
  presetBtns = PRESETS.map(([name, v], i) => {
    const b = el('button', 'strip-pic');
    b.type = 'button';
    b.id = 'preset-' + i;
    b.setAttribute('aria-label', T(name));
    b.addEventListener('mousedown', (e) => e.preventDefault());
    // a pointer is handled on the strip below; this is the keyboard
    b.addEventListener('click', (e) => {
      if (e.detail) return;
      sound();
      choosePreset(v);
    });
    track.appendChild(b);
    return b;
  });
  const fill = (src) => {
    if (!src) return;
    presetThumbs(src).forEach((c, i) => {
      if (presetBtns[i]) presetBtns[i].replaceChildren(c);
    });
  };
  const fileNow = file,
    slotNow = slotAt,
    btnsNow = presetBtns;
  smallOf(s, slotAt).then((src) => {
    if (file === fileNow && slotAt === slotNow && presetBtns === btnsNow) fill(src);
  });
  strip.append(track, presetRing);
  body.append(strip, presetLabel);
  const pick = (i) => {
    const n = clamp(i, 0, PRESETS.length - 1);
    sound();
    choosePreset(PRESETS[n][1]);
    if (sameAdj(s.adj, PRESETS[n][1])) moveStrip(stripX(n));
  };
  let grip = null,
    wheelAt = 0;
  strip.addEventListener('pointerdown', (e) => {
    strip.setPointerCapture(e.pointerId);
    grip = {
      x: e.clientX,
      from: new DOMMatrix(getComputedStyle(track).transform).m41,
      tile: presetBtns.indexOf(e.target.closest('.strip-pic')),
      moved: false,
    };
    track.style.transition = 'none';
    moveStrip(grip.from);
  });
  strip.addEventListener('pointermove', (e) => {
    if (!grip) return;
    const dx = e.clientX - grip.x;
    if (Math.abs(dx) > 4) grip.moved = true;
    if (grip.moved) moveStrip(clamp(grip.from + dx, stripX(PRESETS.length - 1) - 24, stripX(0) + 24));
  });
  const release = () => {
    if (!grip) return;
    const h = grip;
    grip = null;
    track.style.transition = '';
    let n = h.tile;
    if (h.moved) {
      const at = new DOMMatrix(getComputedStyle(track).transform).m41;
      n = presetBtns.reduce((best, _, i) => (Math.abs(stripX(i) - at) < Math.abs(stripX(best) - at) ? i : best), 0);
    }
    if (n < 0) {
      moveStrip(stripX(presetAt));
      return;
    }
    if (!sameAdj(s.adj, PRESETS[n][1])) pick(n);
    else markPreset(n);
  };
  // let go anywhere: on the strip, over the picture, or outside the window
  ['pointerup', 'pointercancel', 'lostpointercapture'].forEach((n) => strip.addEventListener(n, release));
  const anywhere = (e) => {
    if (grip && (e.type !== 'pointermove' || !e.buttons)) release();
  };
  ['pointerup', 'pointercancel', 'pointermove', 'blur'].forEach((n) => addEventListener(n, anywhere, true));
  stripCleanup = () =>
    ['pointerup', 'pointercancel', 'pointermove', 'blur'].forEach((n) => removeEventListener(n, anywhere, true));
  strip.addEventListener(
    'wheel',
    (e) => {
      e.preventDefault();
      e.stopPropagation();
      const now = performance.now(),
        d = Math.abs(e.deltaX) > Math.abs(e.deltaY) ? e.deltaX : e.deltaY;
      if (!d || now - wheelAt < 140) return;
      wheelAt = now;
      pick(presetAt + Math.sign(d));
    },
    { passive: false }
  );
  strip.addEventListener('keydown', (e) => {
    if (e.key !== 'ArrowLeft' && e.key !== 'ArrowRight') return;
    e.preventDefault();
    e.stopPropagation();
    pick(presetAt + (e.key === 'ArrowLeft' ? -1 : 1));
    if (presetBtns[presetAt]) presetBtns[presetAt].focus();
  });
  adjSliders = {};
  const makeSlider = ([k, name]) => {
    const one = slider({
      id: 'adj-' + k,
      label: name,
      min: -100,
      max: 100,
      value: s.adj[k],
      center: true,
      reset: 0,
      onInput: (v) => {
        finishTween();
        s.adj[k] = v;
        drawBase();
        syncPresets();
      },
      onChange: () => commit(),
    });
    adjSliders[k] = one;
    return one.root;
  };
  ADJ.filter(([k]) => !FOLDED.includes(k)).forEach((a) => body.appendChild(makeSlider(a)));
  const more = el('button', 'fold-btn');
  more.type = 'button';
  more.id = 'adjMore';
  more.setAttribute('aria-expanded', String(foldOpen));
  more.append(el('span', 'dyn', T('精细调整')));
  foldNote = el('span', 'fold-note dyn', T('已修改'));
  more.appendChild(foldNote);
  more.insertAdjacentHTML('beforeend', icon('chevron'));
  const fold = el('div', 'fold' + (foldOpen ? ' on' : '')),
    inner = el('div', 'fold-in');
  ADJ.filter(([k]) => FOLDED.includes(k)).forEach((a) => inner.appendChild(makeSlider(a)));
  fold.appendChild(inner);
  fold.inert = !foldOpen;
  more.addEventListener('mousedown', (e) => e.preventDefault());
  more.addEventListener('click', () => {
    sound();
    foldOpen = !foldOpen;
    fold.classList.toggle('on', foldOpen);
    fold.inert = !foldOpen;
    more.setAttribute('aria-expanded', String(foldOpen));
    if (fitted) setTimeout(fitView, 440);
  });
  const foldWrap = el('div');
  foldWrap.append(more, fold);
  body.appendChild(foldWrap);
  const hold = softBtn('holdOriginal', '按住对比原图');
  hold.style.justifyContent = 'center';
  const show = (plain) => () => {
    finishTween();
    drawBase(plain);
  };
  hold.addEventListener('pointerdown', show(true));
  ['pointerup', 'pointerleave', 'pointercancel'].forEach((n) => hold.addEventListener(n, show(false)));
  body.appendChild(hold);
  // the frame lands where it belongs when the panel is drawn, and only travels after that
  track.style.transition = 'none';
  moveStrip(stripX(presetAt));
  syncPresets();
  requestAnimationFrame(() =>
    requestAnimationFrame(() => {
      track.style.transition = '';
    })
  );
}
const RATIOS = [
  ['自由比例', null],
  ['原始比例', 'orig'],
  ['1:1', 1],
  ['4:3', 4 / 3],
  ['3:2', 3 / 2],
  ['16:9', 16 / 9],
  ['3:4', 3 / 4],
  ['9:16', 9 / 16],
];
function panelCrop(body) {
  const w = el('div');
  w.appendChild(label('纵横比'));
  const chips = el('div', 'chips');
  RATIOS.forEach(([name, r]) => {
    const b = el('button', 'chip dyn', T(name));
    b.type = 'button';
    b.setAttribute('aria-pressed', String(ratio === r));
    b.addEventListener('mousedown', (e) => e.preventDefault());
    b.addEventListener('click', () => {
      sound();
      ratio = r;
      chips.querySelectorAll('.chip').forEach((x) => x.setAttribute('aria-pressed', String(x === b)));
      fitRatio();
    });
    chips.appendChild(b);
  });
  w.appendChild(chips);
  body.appendChild(w);
  const row = el('div', 'side-row'),
    reset = softBtn('cropReset', '还原::裁剪'),
    ok = softBtn('cropApply', '完成裁剪', 'btn primary');
  reset.addEventListener('click', () => {
    sound();
    ratio = null;
    temp = full();
    renderPanel();
    placeOverlays();
  });
  ok.addEventListener('click', () => {
    sound();
    setTool('');
  });
  row.append(reset, ok);
  body.appendChild(row);
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
      label: '笔触粗细',
      min: 2,
      max: Math.max(120, Math.round(Math.max(curW(), curH()) / 16)),
      value: brush.size,
      unit: ' px',
      onInput: (v) => {
        brush.size = v;
        placeOverlays();
      },
    }).root
  );
}
function panelText(body) {
  const it = editing || picked,
    cur = it || words;
  if (!words.size) words.size = cur.size = Math.max(16, Math.round(curH() / 16));
  const apply = (k, v) => {
    words[k] = v;
    const t = editing || picked;
    if (t) {
      t[k] = v;
      if (editing) syncEditStyle();
      else {
        drawMarks();
        commit();
      }
      placeOverlays();
    }
  };
  const c = el('div');
  c.append(
    label('色彩'),
    swatches(cur.color, (v, final) => {
      words.color = v;
      const t = editing || picked;
      if (!t) return;
      t.color = v;
      if (editing) syncEditStyle();
      else {
        drawMarks();
        if (final) commit();
      }
    })
  );
  const sizeS = slider({
    id: 'textSize',
    label: '字号',
    min: 12,
    max: Math.max(120, Math.round(curH() / 3)),
    value: cur.size,
    unit: ' px',
    onInput: (v) => {
      const t = editing || picked;
      words.size = v;
      if (t) {
        t.size = v;
        if (editing) {
          syncEditStyle();
        } else drawMarks();
        placeOverlays();
      }
    },
    onChange: () => {
      if (picked && !editing) commit();
    },
  });
  const bold = el('button', 'row compact');
  bold.type = 'button';
  bold.id = 'textBold';
  bold.setAttribute('role', 'switch');
  bold.setAttribute('aria-checked', String(!!cur.bold));
  bold.innerHTML = `<span class="row-t"><b>${T('加粗')}</b></span><i class="tog"></i>`;
  bold.addEventListener('mousedown', (e) => e.preventDefault());
  bold.addEventListener('click', () => {
    sound();
    const v = bold.getAttribute('aria-checked') !== 'true';
    bold.setAttribute('aria-checked', String(v));
    apply('bold', v);
  });
  body.append(c, sizeS.root, bold);
  if (picked && !editing) {
    const del = softBtn('textDelete', '删除文本');
    del.style.justifyContent = 'center';
    del.addEventListener('click', () => {
      sound();
      removePicked();
    });
    body.appendChild(del);
  }
}
function removePicked() {
  if (!picked) return;
  s.items = s.items.filter((i) => i !== picked);
  picked = null;
  drawMarks();
  commit();
  renderPanel();
  placeOverlays();
}
function panelInfo(body) {
  const dl = el('dl', 'info-list'),
    r = s.crop || full(),
    x = file.info || {};
  const row = (k, v) => {
    if (!v) return;
    const d = el('div');
    d.append(el('dt', 'dyn', T(k)), el('dd', 'dyn', v));
    dl.appendChild(d);
  };
  row('格式', file.label);
  row('像素尺寸', `${r.w} × ${r.h}`);
  row('文件大小', money(file.size));
  row('拍摄时间', x.time);
  row('相机设备', x.camera);
  row('镜头型号', x.lens);
  row('曝光参数', x.settings);
  row('拍摄地点', x.place);
  body.appendChild(dl);
  // shown only when DotAny Text is really there to open it
  if (file.svg && file.text_tool) {
    const b = softBtn('openInText', '在 Text 中查看源码');
    b.style.justifyContent = 'center';
    b.addEventListener('click', () => {
      sound();
      be.openInText();
    });
    body.appendChild(b);
  }
}

// crop
function startCrop() {
  temp = s.crop ? { ...s.crop } : full();
  ratio = null;
  layout();
  applyView();
}
function finishCrop(discard) {
  if (tool !== 'crop' || !temp) return;
  const f = full(),
    t = temp;
  temp = null;
  if (discard) return;
  const x = clamp(Math.round(t.x), 0, f.w - 1),
    y = clamp(Math.round(t.y), 0, f.h - 1);
  const rounded = { x, y, w: clamp(Math.round(t.w), 1, f.w - x), h: clamp(Math.round(t.h), 1, f.h - y) };
  const whole = rounded.w === f.w && rounded.h === f.h;
  const same = s.crop
    ? rounded.x === s.crop.x && rounded.y === s.crop.y && rounded.w === s.crop.w && rounded.h === s.crop.h
    : whole;
  if (same) return;
  s.crop = whole ? null : rounded;
  commit();
}
const ratioValue = () => (ratio === 'orig' ? s.w / s.h : ratio);
function fitRatio() {
  const r = ratioValue();
  if (!r) return;
  const f = full();
  let w = temp.w,
    h = w / r;
  if (h > temp.h) {
    h = temp.h;
    w = h * r;
  }
  if (w > f.w) {
    w = f.w;
    h = w / r;
  }
  if (h > f.h) {
    h = f.h;
    w = h * r;
  }
  temp = { x: clamp(temp.x + (temp.w - w) / 2, 0, f.w - w), y: clamp(temp.y + (temp.h - h) / 2, 0, f.h - h), w, h };
  placeOverlays();
}
function dragCrop(h, start, p) {
  const f = full(),
    o = start,
    dx = p.x - start.px,
    dy = p.y - start.py,
    min = 16,
    r = ratioValue();
  if (h === 'move') {
    temp = { ...temp, x: clamp(o.x + dx, 0, f.w - o.w), y: clamp(o.y + dy, 0, f.h - o.h) };
    return;
  }
  let l = o.x,
    t = o.y,
    R = o.x + o.w,
    B = o.y + o.h;
  if (h.includes('w')) l = clamp(o.x + dx, 0, R - min);
  if (h.includes('e')) R = clamp(o.x + o.w + dx, l + min, f.w);
  if (h.includes('n')) t = clamp(o.y + dy, 0, B - min);
  if (h.includes('s')) B = clamp(o.y + o.h + dy, t + min, f.h);
  if (r) {
    let w = R - l,
      H = B - t;
    if (h === 'n' || h === 's') w = H * r;
    else if (h === 'e' || h === 'w') H = w / r;
    else if (w / H > r) H = w / r;
    else w = H * r;
    if (h.includes('w')) l = R - w;
    else if (!h.includes('e')) l = o.x + o.w / 2 - w / 2;
    if (h.includes('n')) t = B - H;
    else if (!h.includes('s')) t = o.y + o.h / 2 - H / 2;
    if (l < 0 || t < 0 || l + w > f.w || t + H > f.h) return;
    R = l + w;
    B = t + H;
  }
  temp = { x: l, y: t, w: R - l, h: B - t };
}

// words on the picture
const txtEdit = el('div', 'txt-edit'),
  txtInput = document.createElement('input');
txtInput.type = 'text';
txtInput.id = 'textInput';
txtInput.spellcheck = false;
txtInput.autocomplete = 'off';
txtEdit.appendChild(txtInput);
stage.appendChild(txtEdit);
inkfall(txtInput);
txtEdit.remove();
function syncEditStyle() {
  txtEdit.style.color = editing.color;
  txtEdit.style.fontWeight = editing.bold ? 600 : 400;
  txtEdit.style.fontFamily = 'var(--font-text)';
  sizeEdit();
}
function sizeEdit() {
  txtEdit.style.width = Math.ceil(measureText({ ...editing, text: txtInput.value + ' ' }) * vz) + 8 + 'px';
}
function startEdit(it) {
  editing = it;
  picked = null;
  txtInput.value = it.text;
  stage.appendChild(txtEdit);
  syncEditStyle();
  drawMarks();
  placeOverlays();
  renderPanel();
  txtInput.dispatchEvent(new Event('input'));
  requestAnimationFrame(() => {
    txtInput.focus();
    const n = txtInput.value.length;
    txtInput.setSelectionRange(n, n);
  });
}
function commitText() {
  finishTween();
  if (!editing) return;
  const it = editing,
    was = s.items.includes(it),
    text = txtInput.value.trim();
  editing = null;
  txtEdit.remove();
  if (text) {
    const changed = it.text !== text || !was;
    it.text = text;
    if (!was) s.items.push(it);
    drawMarks();
    if (changed) {
      commit();
    }
    picked = it;
  } else {
    if (was) {
      s.items = s.items.filter((i) => i !== it);
      drawMarks();
      commit();
    }
    drawMarks();
  }
  if (tool === 'text') renderPanel();
  placeOverlays();
}
txtInput.addEventListener('input', () => {
  if (editing) sizeEdit();
});
txtInput.addEventListener('keydown', (e) => {
  e.stopPropagation();
  if (e.key === 'Enter' || e.key === 'Escape') {
    e.preventDefault();
    commitText();
  }
});
const hitText = (p) => {
  for (let i = s.items.length - 1; i >= 0; i--) {
    const t = s.items[i];
    if (t.t === 'text' && p.x >= t.x && p.x <= t.x + measureText(t) && p.y >= t.y && p.y <= t.y + t.size * 1.2)
      return t;
  }
  return null;
};

// the pointer on the picture
let drag = null,
  spaceDown = false;
stage.addEventListener('pointerdown', (e) => {
  if (!file || e.target === txtInput) return;
  const p = toImg(e),
    pan = e.button === 1 || spaceDown || (!tool && e.button === 0) || tool === 'adjust';
  stage.setPointerCapture(e.pointerId);
  if (pan) {
    drag = { t: 'pan', x: e.clientX, y: e.clientY };
    root.classList.add('panning');
    return;
  }
  if (tool === 'crop') {
    if (!temp) return;
    const h = e.target.dataset && e.target.dataset.h;
    if (!h && !cropBox.contains(e.target)) return;
    drag = { t: 'crop', h: h || 'move', start: { ...temp, px: p.x, py: p.y } };
    cropBox.classList.add('moving');
  } else if (tool === 'brush') {
    const it = { t: 'brush', color: brush.color, size: brush.size, pts: [p.x, p.y] };
    s.items.push(it);
    drag = { t: 'brush', it };
    drawMarks();
  } else if (tool === 'text') {
    const hit = hitText(p);
    if (editing) {
      commitText();
      drag = null;
      return;
    }
    if (hit) {
      const again = picked === hit;
      picked = hit;
      renderPanel();
      placeOverlays();
      drag = { t: 'move', it: hit, sx: e.clientX, sy: e.clientY, ox: hit.x, oy: hit.y, again, moved: false };
      return;
    }
    if (picked) {
      picked = null;
      renderPanel();
      placeOverlays();
      return;
    }
    startEdit({
      t: 'text',
      x: p.x,
      y: p.y - (words.size || 40) * 0.6,
      text: '',
      color: words.color,
      size: words.size || Math.max(16, Math.round(curH() / 16)),
      bold: words.bold,
    });
  }
});
stage.addEventListener('pointermove', (e) => {
  ring.style.left = e.clientX + 'px';
  ring.style.top = e.clientY + 'px';
  ring.classList.add('on');
  if (!drag) return;
  if (drag.t === 'pan') {
    const dx = e.clientX - drag.x,
      dy = e.clientY - drag.y;
    drag.x = e.clientX;
    drag.y = e.clientY;
    vx += dx;
    vy += dy;
    tx += dx;
    ty += dy;
    fitted = false;
    applyView();
  } else if (drag.t === 'crop') {
    dragCrop(drag.h, drag.start, { ...toImg(e) });
    placeOverlays();
  } else if (drag.t === 'brush') {
    const ev = e.getCoalescedEvents ? e.getCoalescedEvents() : [e];
    (ev.length ? ev : [e]).forEach((c) => {
      const q = toImg(c);
      drag.it.pts.push(q.x, q.y);
    });
    drawMarks();
  } else if (drag.t === 'move') {
    const dx = (e.clientX - drag.sx) / vz,
      dy = (e.clientY - drag.sy) / vz;
    if (Math.abs(dx) + Math.abs(dy) > 2 / vz) drag.moved = true;
    drag.it.x = drag.ox + dx;
    drag.it.y = drag.oy + dy;
    drawMarks();
    placeOverlays();
  }
});
stage.addEventListener('pointerleave', () => ring.classList.remove('on'));
const endDrag = () => {
  if (!drag) return;
  const d = drag;
  drag = null;
  root.classList.remove('panning');
  cropBox.classList.remove('moving');
  if (d.t === 'brush') commit();
  if (d.t === 'move') {
    if (d.moved) {
      commit();
    } else if (d.again) startEdit(d.it);
  }
};
stage.addEventListener('pointerup', endDrag);
stage.addEventListener('pointercancel', endDrag);
stage.addEventListener(
  'wheel',
  (e) => {
    if (!file) return;
    e.preventDefault();
    // the Mac's trackpad: two fingers move a picture larger than the window, as Preview does, and a picture that fits
    // stays where it is (the pinch zooms, below); a mouse's wheel zooms as everywhere
    if (fromTrackpad && !e.ctrlKey && !e.metaKey && fromTrackpad(e) === 'trackpad') {
      panBy(-e.deltaX, -e.deltaY);
      return;
    }
    zoomAround(tz * Math.exp(-e.deltaY * (e.ctrlKey ? 0.01 : 0.0016)), e.clientX, e.clientY);
  },
  { passive: false }
);
// ---------- the Mac's trackpad (gestures.js): pinch, rotate, smart zoom ----------
const fromTrackpad = mac ? wheelSource() : null,
  quarters = quarterTurns();
// the picture follows the fingers at once: no easing between it and them
function zoomNow(next, px, py) {
  zoomAround(next, px, py);
  cancelAnimationFrame(viewRaf);
  viewRaf = 0;
  vx = tx;
  vy = ty;
  vz = tz;
  applyView();
}
// as far as the picture reaches: its edges stop at the window's, and a side that fits stays centred
function panBy(dx, dy) {
  if (!root.classList.contains('can-pan')) return;
  const a = area(),
    r = shown(),
    w = r.w * tz,
    h = r.h * tz,
    keep = (t, d, lo, hi, size) => (size <= hi - lo ? (lo + hi) / 2 : clamp(t + d, hi - size / 2, lo + size / 2));
  tx = keep(tx, dx, a.l, a.r, w);
  ty = keep(ty, dy, a.t, a.b, h);
  fitted = false;
  cancelAnimationFrame(viewRaf);
  viewRaf = 0;
  vx = tx;
  vy = ty;
  vz = tz;
  applyView();
}
pinch(stage, {
  begin: () => quarters.begin(),
  zoom: (k, x, y) => file && zoomNow(tz * k, x, y),
  rotate: (deg) => {
    const q = file && !tool ? quarters.change(deg) : 0;
    if (q) rotate(q);
  },
});
if (mac)
  be.on('smart-zoom', ({ x, y }) => {
    if (!file || tool) return;
    if (fitted) zoomAround(1, x, y);
    else fitView();
  });
stage.addEventListener('dblclick', (e) => {
  if (!file || tool) return;
  if (fitted) zoomAround(1, e.clientX, e.clientY);
  else fitView();
});
new ResizeObserver(() => {
  edgeFix();
  if (tool === 'adjust' && presetRing) syncPresets();
  if (roomyNow() !== root.classList.contains('has-rail')) placeTools();
  if (!file) return;
  if (fitted) fitView();
  else {
    syncPct();
    applyView();
  }
  syncSeg();
}).observe(app);
new ResizeObserver(() => {
  edgeFix();
  if (file && fitted) fitView();
}).observe(chrome);

// ---------- the pill: moving formats play, ICO shows its sizes, TIFF its pages ----------
// frames stay on the Rust side; the page holds a limited handful, and what it hasn't got it asks for
const FRAME_CACHE_BYTES = 128 * 1048576;
let playTimer = 0,
  playing = false,
  frameAt = 0,
  frameCount = 0,
  frameDelays = [],
  frameDone = true,
  frameBytes = 0,
  frameFetches = new Map(),
  lastShownAt = 0;
const frameCache = new Map();
function stopPlay() {
  playing = false;
  clearTimeout(playTimer);
}
// very short delays are how old GIFs say "as fast as you can"; browsers slow those down the same way
const frameMs = (i) => {
  const d = frameDelays[i];
  return d >= 20 ? d : 100;
};
function fetchFrame(i, gen) {
  if (frameCache.has(i)) return Promise.resolve(frameCache.get(i));
  if (frameFetches.has(i)) return frameFetches.get(i);
  const p = be.viewPixels(gen, slotAt, i, FRAME_SIDE).then((v) => {
    frameFetches.delete(i);
    if (!v || !file || file.gen !== gen) return null;
    frameCache.set(i, v);
    frameBytes += v.data.length;
    // the oldest goes first
    for (const [k, old] of frameCache) {
      if (frameBytes <= FRAME_CACHE_BYTES) break;
      if (k !== i) {
        frameCache.delete(k);
        frameBytes -= old.data.length;
      }
    }
    return v;
  });
  frameFetches.set(i, p);
  return p;
}
async function loadFrame(i, seq) {
  const f = file,
    gen = f.gen;
  const v = await fetchFrame(i, gen);
  if (!v || file !== f || (seq && seq !== renderSeq)) return false;
  s.pw = v.w;
  s.ph = v.h;
  frameAt = i;
  setPixels(v.data, v.w, v.h);
  drawBase();
  drawMarks();
  lastShownAt = performance.now();
  return true;
}
async function pollFrames(f) {
  while (file === f) {
    const st = await be.framesState(0);
    if (!st || file !== f) return;
    frameCount = st.count;
    frameDelays = st.delays;
    frameDone = st.done;
    if (st.alpha && !s.alpha) {
      s.alpha = true;
      photo.classList.add('alpha');
    }
    if (st.done) return;
    await new Promise((ok) => setTimeout(ok, 250));
  }
}
function scheduleTick() {
  clearTimeout(playTimer);
  if (!playing) return;
  const wait = frameMs(frameAt) - (performance.now() - lastShownAt);
  // the next one on its way while this one is showing
  if (frameCount > 1 && file) fetchFrame((frameAt + 1) % frameCount, file.gen);
  playTimer = setTimeout(tick, Math.max(0, wait));
}
async function tick() {
  if (!playing || !file || !frameCount) return;
  const f = file,
    next = (frameAt + 1) % frameCount;
  // the end of what's been read so far is not the end of the picture
  if (next === 0 && !frameDone) {
    playTimer = setTimeout(tick, 50);
    return;
  }
  const ok = await loadFrame(next);
  if (file === f && playing) {
    if (ok) scheduleTick();
    else playTimer = setTimeout(tick, 100);
  }
}
function togglePlay() {
  playing = !playing;
  clearTimeout(playTimer);
  if (playing) {
    lastShownAt = 0;
    tick();
  }
  drawPill();
}
// many pages: a counter, not a row of buttons that won't fit
const MANY_PAGES = 9;
function drawPill() {
  if (reduce || !pill.offsetWidth) {
    fillPill();
    return;
  }
  pill.getAnimations().forEach((a) => a.cancel());
  const from = pill.offsetWidth;
  fillPill();
  const to = pill.offsetWidth;
  if (from === to) return;
  pillExtra.animate([{ opacity: 0 }, { opacity: 1 }], MOVE);
  pill.style.overflow = 'hidden';
  const a = pill.animate([{ width: from + 'px' }, { width: to + 'px' }], MOVE);
  a.onfinish = a.oncancel = () => {
    pill.style.overflow = '';
  };
}
function fillPill() {
  pillExtra.textContent = '';
  if (!file) return;
  if (s.moving) {
    pillExtra.appendChild(pillBtn('playToggle', playing ? 'pause' : 'play', playing ? 'Pause' : 'Play', togglePlay));
    return;
  }
  const n = file.slots.length;
  if (n < 2) return;
  if (n > MANY_PAGES) {
    const at = el('span', 'page-at dyn', `${slotAt + 1} / ${n}`);
    pillExtra.append(
      pillBtn('pagePrev', 'prev', 'Previous page', () => setSlot((slotAt - 1 + n) % n)),
      at,
      pillBtn('pageNext', 'next', 'Next page', () => setSlot((slotAt + 1) % n))
    );
    return;
  }
  file.slots.forEach((slot, i) => {
    const b = el('button', 'chip dyn', file.kind === 'ico' ? String(slot.w) : String(i + 1));
    b.type = 'button';
    b.id = 'slot-' + i;
    b.setAttribute('aria-pressed', String(i === slotAt));
    b.addEventListener('click', () => {
      sound();
      setSlot(i);
    });
    pillExtra.appendChild(b);
  });
}

// ---------- the tools stand in the rail; a window too narrow for it keeps them in the dock ----------
const roomyNow = () => innerWidth > 720 && !root.classList.contains('bar-none');
function placeTools() {
  const on = roomyNow();
  root.classList.toggle('has-rail', on);
  if (on) rail.appendChild(seg);
  else pill.prepend(seg);
  placed = '';
  syncSeg();
  if (file && fitted) fitView();
}

// ---------- save as: what comes out and how big ----------
let fmtKey = 'jpg',
  quality = 90,
  qualityTouched = false,
  lossless = false,
  stripExif = false,
  outW = 0,
  outH = 0,
  estToken = 0,
  noteKeys = [],
  rowsBox = null,
  reEstimate = () => {},
  saveVersions = [];
const outSize = () => {
  const r = s.crop || full();
  return { w: r.w, h: r.h };
};
const saveRequest = (format = fmtKey) => ({
  format,
  quality,
  quality_touched: qualityTouched,
  lossless,
  strip: stripExif && CARRIES_DETAILS.includes(format),
  w: outW,
  h: outH,
  slot: slotAt,
  frame: frameAt,
  edits: file.slots.map((o) => ({ geo: o.geo, crop: o.crop, adj: o.adj, marked: o.items.length > 0 })),
});
const morph = shell.morph;
// whatever changes size eases to it, and what's new fades in (the dock's extra buttons too)
const MOVE = { duration: 240, easing: 'cubic-bezier(.2,.7,.2,1)' };
// the card opens: whatever is being typed or cropped is settled, and a new save starts from the picture as shown
function cardOpens() {
  commitText();
  if (tool === 'crop') setTool('');
  const o = outSize();
  outW = o.w;
  outH = o.h;
  qualityTouched = false;
  noteKeys = [];
}
// the quiet line under the formats, in the page's words, from Rust's keys
const noteLine = (keys) => keys.map((k) => T(NOTES[k])).join(' ');
// the size, and the quiet line under the formats, from Rust's plan of the save; an answer nobody waits for is dropped
async function estimateSize(fmt) {
  fmtKey = fmt;
  // a slider being dragged asks again and again: only the question still standing 60 ms later is sent
  const tok = ++estToken;
  await new Promise((ok) => setTimeout(ok, 60));
  if (tok !== estToken) return null;
  const plan = await be.estimateSave(tok, saveRequest());
  if (!plan) return null;
  noteKeys = plan.notes;
  return { size: plan.size, line: noteLine(noteKeys) };
}
// the same notes at once, without the encode, for a format the card is switching to (the size follows)
const saveLine = (fmt) => be.saveNotes(saveRequest(fmt)).then((keys) => (keys ? noteLine(keys) : undefined));
function drawRows() {
  const rows = rowsBox,
    estimate = reEstimate;
  if (!rows) return;
  rows.textContent = '';
  const F = FORMAT[fmtKey];
  if (F.lossy) {
    if (F.lossless) {
      const t = el('button', 'row compact');
      t.type = 'button';
      t.id = 'saveLossless';
      t.setAttribute('role', 'switch');
      t.setAttribute('aria-checked', String(lossless));
      t.innerHTML = `<span class="row-t"><b>${T('无损压缩')}</b><small>${T('完整保留原始画质，文件体积较大。')}</small></span><i class="tog"></i>`;
      t.addEventListener('mousedown', (e) => e.preventDefault());
      t.addEventListener('click', () => {
        sound();
        lossless = !lossless;
        morph(rows, drawRows);
        estimate();
      });
      rows.appendChild(t);
    }
    if (!(F.lossless && lossless))
      rows.appendChild(
        slider({
          id: 'saveQuality',
          label: '画质',
          min: 10,
          max: 100,
          value: quality,
          onInput: (v) => {
            quality = v;
            qualityTouched = true;
            estimate();
          },
        }).root
      );
  }
  if (!F.keep) {
    const size = el('div', 'sv-size'),
      wI = document.createElement('input'),
      hI = document.createElement('input');
    [wI, hI].forEach((i, n) => {
      i.type = 'number';
      i.min = 1;
      i.max = 16384;
      i.className = 'sv-num';
      i.id = n ? 'saveH' : 'saveW';
      i.setAttribute('aria-label', n ? 'Height' : 'Width');
    });
    wI.value = outW;
    hI.value = outH;
    const o = outSize(),
      ar = o.w / o.h;
    wI.addEventListener('input', () => {
      outW = clamp(+wI.value || 1, 1, 16384);
      outH = Math.max(1, Math.round(outW / ar));
      hI.value = outH;
      estimate();
    });
    hI.addEventListener('input', () => {
      outH = clamp(+hI.value || 1, 1, 16384);
      outW = Math.max(1, Math.round(outH * ar));
      wI.value = outW;
      estimate();
    });
    size.append(label('像素尺寸'), wI, el('span', 'sv-x', '×'), hI);
    rows.appendChild(size);
  }
  if (file.details && CARRIES_DETAILS.includes(fmtKey)) {
    const t = el('button', 'row compact');
    t.type = 'button';
    t.id = 'saveStrip';
    t.setAttribute('role', 'switch');
    t.setAttribute('aria-checked', String(stripExif));
    t.innerHTML = `<span class="row-t"><b>${T('抹除拍摄元数据')}</b><small>${T('移除拍摄时间、相机型号与地理位置等隐私信息。')}</small></span><i class="tog"></i>`;
    t.addEventListener('mousedown', (e) => e.preventDefault());
    t.addEventListener('click', () => {
      sound();
      stripExif = !stripExif;
      t.setAttribute('aria-checked', String(stripExif));
      estimate();
    });
    rows.appendChild(t);
  }
}
// what was drawn is rasterised here at full size, in pieces, and laid over the pixels in Rust
const TILE = 4096;
function itemBounds(it) {
  if (it.t === 'brush') {
    let l = Infinity,
      t = Infinity,
      r = -Infinity,
      b = -Infinity;
    for (let i = 0; i < it.pts.length; i += 2) {
      l = Math.min(l, it.pts[i]);
      r = Math.max(r, it.pts[i]);
      t = Math.min(t, it.pts[i + 1]);
      b = Math.max(b, it.pts[i + 1]);
    }
    const m = it.size / 2 + 2;
    return [l - m, t - m, r + m, b + m];
  }
  const m = it.size * 0.3;
  return [it.x - m, it.y - m, it.x + measureText(it) + m, it.y + it.size * 1.2 + m];
}
const pngOf = async (c) => new Uint8Array(await (await c.convertToBlob({ type: 'image/png' })).arrayBuffer());
async function sendMarks(i, o) {
  if (!o.items.length) return;
  const W = curW(o),
    H = curH(o),
    area_ = o.crop || { x: 0, y: 0, w: W, h: H };
  const bounds = o.items.map(itemBounds);
  const L = Math.max(area_.x, Math.floor(Math.min(...bounds.map((b) => b[0])))),
    T_ = Math.max(area_.y, Math.floor(Math.min(...bounds.map((b) => b[1]))));
  const R = Math.min(area_.x + area_.w, Math.ceil(Math.max(...bounds.map((b) => b[2])))),
    B = Math.min(area_.y + area_.h, Math.ceil(Math.max(...bounds.map((b) => b[3]))));
  const canvas = new OffscreenCanvas(1, 1),
    g = canvas.getContext('2d', { willReadFrequently: true });
  for (let y = T_; y < B; y += TILE) {
    for (let x = L; x < R; x += TILE) {
      const w = Math.min(TILE, R - x),
        h = Math.min(TILE, B - y);
      const here = [x, y, x + w, y + h];
      const items = o.items.filter(
        (_, n) => bounds[n][0] < here[2] && bounds[n][2] > here[0] && bounds[n][1] < here[3] && bounds[n][3] > here[1]
      );
      if (!items.length) continue;
      if (canvas.width !== w) canvas.width = w;
      if (canvas.height !== h) canvas.height = h;
      g.setTransform(1, 0, 0, 1, 0, 0);
      g.clearRect(0, 0, w, h);
      g.setTransform(1, 0, 0, 1, -x, -y);
      items.forEach((it) => paintItem(g, it));
      await be.putTile(i, x, y, await pngOf(canvas));
    }
  }
}
// a drawing goes out as pixels at the size asked for
async function sendDrawing() {
  for (let y = 0; y < outH; y += TILE)
    for (let x = 0; x < outW; x += TILE) {
      const w = Math.min(TILE, outW - x),
        h = Math.min(TILE, outH - y),
        c = new OffscreenCanvas(w, h),
        g = c.getContext('2d', { willReadFrequently: true });
      g.drawImage(svgImg, -x, -y, outW, outH);
      await be.putTile(0, x, y, await pngOf(c));
    }
}
// the family's card has asked where (the system's dialog, which asked before replacing anything); this writes it
async function saveTo(path, fmt) {
  fmtKey = fmt;
  const req = saveRequest(),
    f = file,
    at = slotAt;
  saveVersions = f.slots.map((o) => o.at);
  let r;
  try {
    await be.clearTiles();
    if (f.svg) {
      if (fmtKey !== 'svg') await sendDrawing();
    } else if (fmtKey === f.kind && (f.kind === 'tiff' || f.kind === 'ico')) {
      for (let i = 0; i < f.slots.length; i++) await sendMarks(i, f.slots[i]);
    } else await sendMarks(at, f.slots[at]);
    r = await be.saveAs(path, true, req);
  } catch (e) {
    r = { status: 'failed', kind: 'other', detail: String(e) };
  }
  if (r.status === 'saved') return { ...r, exported: !r.whole };
  return { status: 'failed', kind: r.status === 'toolarge' ? 'toolarge' : r.kind || 'other' };
}
// what's on disk now is this state of the picture; later edits still count as unsaved
function afterSave(r) {
  if (!file) return;
  if (r.whole) {
    file.path = r.path;
    file.name = r.name;
    file.dir = r.path.replace(/[\\/][^\\/]*$/, '');
    file.slots.forEach((o, n) => {
      o.savedAt = saveVersions[n];
    });
    file.size = r.size;
    file.beside = Math.max(file.beside, 2);
  }
  syncTop();
}
infoBtn.addEventListener('click', () => {
  sound();
  const on = infoBtn.getAttribute('aria-pressed') !== 'true';
  if (on && tool) setTool('');
  infoBtn.setAttribute('aria-pressed', String(on));
  renderPanel();
  fitView();
});

// ---------- opening: the family's (the empty window, recent files, the drop, the arrows); the reading is Image's ----------
let pasted = null;
// the picture is read; the family names the window at once, and the pixels follow. Only the file the program
// starts with is on screen (or nearly: no more than 4 s) before the window is shown
let started = !boot.start;
async function openFile(path) {
  const r = path === PASTED ? await be.openPasted(pasted.name, pasted.bytes) : await be.openPath(path);
  if (r.status !== 'ok') return r;
  const name = path === PASTED ? pasted.name : r.name;
  const f = {
    gen: r.gen,
    path: r.path,
    dir: r.dir,
    name,
    kind: r.kind,
    label: r.label,
    size: r.size,
    info: r.info,
    details: r.details,
    svg: r.svg,
    wide: r.wide,
    text_tool: r.text_tool,
    beside: r.beside,
    slots: r.slots.map(newSlot),
  };
  const painted = loadFile(f, null);
  if (!started) {
    started = true;
    await Promise.race([painted, new Promise((ok) => setTimeout(ok, 4000))]);
  }
  return { ...r, name };
}
function closeFile() {
  if (tool) setTool('', { discard: true });
  stopPlay();
  file = null;
  s = null;
  renderSeq++;
  hideDetail();
  infoBtn.setAttribute('aria-pressed', 'false');
  renderPanel();
  root.classList.remove('bar-none', 'caps-crop');
  placed = '';
}
const step = (d) => shell.step(d);
addEventListener('paste', async (e) => {
  if (e.target.tagName === 'INPUT' || e.target.tagName === 'TEXTAREA') return;
  const f = [...(e.clipboardData?.files || [])][0];
  if (!f) return;
  e.preventDefault();
  const bytes = new Uint8Array(await f.arrayBuffer());
  // a copied bitmap arrives with a made-up name
  const name = !f.name || /^image\.\w+$/.test(f.name) ? T('剪贴板图像.png') : f.name;
  shell.guard(() => {
    pasted = { name, bytes };
    shell.openPath(PASTED);
  });
});

// ---------- printing goes through the system window; on paper, only the picture ----------
const printSheet = el('div', 'print-sheet');
document.body.appendChild(printSheet);
addEventListener('afterprint', () => {
  printSheet.textContent = '';
});
function printFile() {
  if (!file) return;
  commitText();
  printSheet.textContent = '';
  if (file.svg) {
    const i = el('img');
    i.src = svgImg.src;
    printSheet.appendChild(i);
  } else {
    const r = s.crop || full(),
      k = base.width / curW(),
      c = el('canvas');
    c.width = Math.round(r.w * k);
    c.height = Math.round(r.h * k);
    const g = c.getContext('2d');
    g.drawImage(base, r.x * k, r.y * k, r.w * k, r.h * k, 0, 0, c.width, c.height);
    g.drawImage(marks, r.x * k, r.y * k, r.w * k, r.h * k, 0, 0, c.width, c.height);
    printSheet.appendChild(c);
  }
  be.printWindow();
}

// ---------- words: the family's translate in place; Image's own parts too ----------
shell.translates(seg, pill, ...navParts);
shell.onLang(() => {
  placed = '';
  syncSeg();
  if (file) renderPanel();
});

// ---------- the keyboard: the family's keys first (open, save as, print, Escape), then Image's ----------
function keys(e) {
  const typing = /INPUT|TEXTAREA/.test(document.activeElement?.tagName) && document.activeElement.type !== 'range';
  const mod = modKey(e),
    k = e.key.toLowerCase();
  if (typing || !file) return;
  if (mod && k === 'z') {
    e.preventDefault();
    if (e.shiftKey) redo();
    else undo();
    return;
  }
  if (mod && k === 'y' && yRedoes) {
    e.preventDefault();
    redo();
    return;
  }
  if (e.key === ' ') {
    e.preventDefault();
    spaceDown = true;
    if (s.moving) {
      if (!e.repeat) togglePlay();
    }
    return;
  }
  if (e.key === 'Escape') {
    if (tool === 'crop') {
      sound();
      setTool('', { discard: true });
      return;
    }
    if (tool) {
      sound();
      setTool('');
      return;
    }
    if (infoBtn.getAttribute('aria-pressed') === 'true') {
      infoBtn.click();
    }
    return;
  }
  if (e.key === 'Enter' && tool === 'crop') {
    e.preventDefault();
    sound();
    setTool('');
    return;
  }
  if ((e.key === 'Delete' || e.key === 'Backspace') && picked) {
    e.preventDefault();
    removePicked();
    return;
  }
  if (!tool && e.key === 'ArrowLeft') {
    step(-1);
    return;
  }
  if (!tool && e.key === 'ArrowRight') {
    step(1);
    return;
  }
  if (e.key === '+' || e.key === '=') zoomBy(1.4);
  else if (e.key === '-') zoomBy(1 / 1.4);
  else if (e.key === '0') fitView();
  else if (e.key === '1') zoomTo(1);
}
addEventListener('keyup', (e) => {
  if (e.key === ' ') spaceDown = false;
});

// ---------- begin ----------
initRender(base, !!boot.nogl);
edgeFix();
if (document.fonts && document.fonts.ready)
  document.fonts.ready.then(() => {
    placed = '';
    syncSeg();
  });
placeTools();
// with DOTANY_TEST set, window.__img exposes the editor's state and actions, so that an automated test can read and
// drive it
if (boot.test) {
  window.__img = {
    get s() {
      return s;
    },
    get file() {
      return file;
    },
    get tool() {
      return tool;
    },
    get z() {
      return vz;
    },
    get frameAt() {
      return frameAt;
    },
    get frameCount() {
      return frameCount;
    },
    get playing() {
      return playing;
    },
    get gpu() {
      return usesGpu();
    },
    get detail() {
      return detailAt;
    },
    get cached() {
      return frameBytes;
    },
    get temp() {
      return temp;
    },
    toScreen,
    toImg,
    fitView,
    zoomTo,
    setSlot,
    loadFrame,
    // past the system's save dialog: the open card saves straight to this path
    writeTo(path) {
      shell.test.answerSaveWith(path);
      document.getElementById('saveAsConfirm').click();
    },
    // what the window is showing, the crop applied, as a PNG at the preview's own resolution
    shownPng() {
      const r = s.crop || full(),
        k = base.width / curW(),
        c = el('canvas');
      c.width = Math.round(r.w * k);
      c.height = Math.round(r.h * k);
      const g = c.getContext('2d');
      g.drawImage(base, Math.round(r.x * k), Math.round(r.y * k), c.width, c.height, 0, 0, c.width, c.height);
      g.drawImage(marks, Math.round(r.x * k), Math.round(r.y * k), c.width, c.height, 0, 0, c.width, c.height);
      return c.toDataURL('image/png').split(',')[1];
    },
    detailPng() {
      return detail.toDataURL('image/png').split(',')[1];
    },
  };
}
// the family opens the file the program was started with, then says the window is ready
shell.begin();
