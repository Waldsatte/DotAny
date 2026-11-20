// DotAny Audio's page. The window around the work is the family's (shared/web/shell.js):
// the top bar, the file menu, settings, the save card, the close prompt, the notice row, the empty and drop views.
// What's Audio's own is here: the wave, editing it, the dock, the adjust and info panels, and the save card's options.
import { createShell } from '@dotany/web/shell.js';
import { el, root, reduce, sound, EASE } from '@dotany/web/shared.js';
import { inkfall } from '@dotany/web/inkfall-input.js';
import { T, WORDS } from './i18n.js';
import { num } from '@dotany/web/i18n.js';
import { LUCIDE } from './icons.js';
import { player } from './player.js';
import * as be from './backend.js';
import { modKey, os, yRedoes } from '@dotany/web/keys.js';
import { mac, wheelSource, pinch } from '@dotany/web/gestures.js';

const icon = (n) => `<svg class="i" viewBox="0 0 24 24" aria-hidden="true">${LUCIDE[n]}</svg>`;
const clamp = (v, a, b) => Math.min(b, Math.max(a, v));
const clock = (t) => {
  t = Math.max(0, t);
  const h = Math.floor(t / 3600),
    m = Math.floor(t / 60) % 60,
    s = Math.floor(t % 60),
    ss = String(s).padStart(2, '0');
  return h ? `${h}:${String(m).padStart(2, '0')}:${ss}` : `${m}:${ss}`;
};

// ---------- what's open, and what's been done to it ----------
const FRESH = () => ({ fadeIn: 0, fadeOut: 0, db: 0, level: false, space: 'off', wet: 50 });
// `spans` are the parts of the source still in the song, in frames. The samples themselves never change.
let doc = null,
  meta = {},
  meta0 = {},
  coverNew = false,
  s = FRESH(),
  spans = [],
  levelK = 1;
let past = [],
  ahead = [],
  savedKey = '',
  metaDirty = false,
  sel = null,
  vA = 0,
  vZ = 1;
const lengthOf = (list) => list.reduce((n, [a, z]) => n + z - a, 0);
const total = () => (doc ? lengthOf(spans) / doc.rate : 1);
player.total = total;
const gainNow = () => Math.pow(10, s.db / 20) * (s.level ? levelK : 1);
const recipe = () => ({
  spans,
  db: s.db,
  level: s.level,
  fadeIn: s.fadeIn,
  fadeOut: s.fadeOut,
  space: s.space,
  wet: s.wet,
});
const keyOf = (r) => JSON.stringify({ ...r, wet: r.space === 'off' ? 0 : r.wet });
const recipeKey = () => keyOf(recipe());

// ---------- editing spans: new lists, so undo is just keeping the old ones ----------
function cutSpans(list, a, z) {
  const out = [];
  let at = 0;
  for (const [x, y] of list) {
    const len = y - x,
      from = at,
      to = at + len;
    if (a > from) out.push([x, x + Math.min(len, a - from)]);
    if (z < to) out.push([x + Math.max(0, z - from), y]);
    at = to;
  }
  return out.filter(([x, y]) => y > x);
}
function keepSpans(list, a, z) {
  const out = [];
  let at = 0;
  for (const [x, y] of list) {
    const len = y - x,
      from = at,
      to = at + len;
    const p = Math.max(a, from),
      q = Math.min(z, to);
    if (q > p) out.push([x + p - from, x + q - from]);
    at = to;
  }
  return out;
}
const envelope = (time, all, fadeIn, fadeOut) => {
  let e = 1;
  if (fadeIn > 0 && time < fadeIn) e *= time / fadeIn;
  if (fadeOut > 0 && time > all - fadeOut) e *= Math.max(0, (all - time) / fadeOut);
  return e;
};

// ---------- the window: the family's, with Audio's own words, formats and buttons ----------
const FORMAT = {
  mp3: { ext: '.mp3', label: 'MP3', lossy: true },
  flac: { ext: '.flac', label: 'FLAC' },
  wav: { ext: '.wav', label: 'WAV' },
  aiff: { ext: '.aiff', label: 'AIFF' },
  ogg: { ext: '.ogg', label: 'Vorbis', lossy: true },
};
const CANT_OPEN = {
  toolarge: '音频文件体积过大，磁盘临时空间不足以展开。',
  // the Mac always decodes M4A: it never answers this (the system decoder is Windows' only, decode.rs), and if its own
  // reading ever fails it says only that the file couldn't be read
  nodecoder: os({
    windows: '当前 Windows 系统缺少 M4A 解码组件，请在系统「可选功能」中安装「媒体功能包」。',
    mac: '无法读取该文件。',
  }),
};
let chosenQuality = null,
  cardFmt = null,
  rowsFmt = null,
  estSeq = 0,
  lastRequest = null;
const shell = createShell({
  name: 'DotAny Audio',
  icons: LUCIDE,
  words: WORDS,
  notices: () => import('./notices.generated.js'),
  dropTitle: '拖拽音频至此',
  fileKind: '音频',
  soundNote: '触发按钮操作时播放提示音，音频播放期间自动静默。',
  licences: [
    ['Symphonia', '音频解码', 'MPL-2.0'],
    ['LAME', '音频编码', 'LGPL-2.0-or-later'],
    ['libFLAC', '音频编码', 'BSD-3-Clause'],
    ['libvorbis (aoTuV)', '音频编码', 'BSD-3-Clause'],
    ['libopenmpt', '音频解码', 'BSD-3-Clause'],
  ],
  // an M4A, a tracker module, an AU or a VOC is only read: the card starts on MP3 and never offers to write its format
  formats: FORMAT,
  formatOf: () => (doc && FORMAT[doc.kind] ? doc.kind : 'mp3'),
  liveNote: true,
  cantOpen: CANT_OPEN,
  failWords: { toolarge: '音频长度或体积超出该格式的上限，未能保存。当前编辑仍完整保留在此窗口中。' },
  open: be.openPath,
  show: (r) => openDoc(r),
  clear: () => closeDoc(),
  dirty: () => !!doc && (recipeKey() !== savedKey || metaDirty),
  discard() {
    savedKey = recipeKey();
    metaDirty = false;
    coverNew = false;
  },
  beforeSaveAs() {
    chosenQuality = null;
    rowsFmt = null;
  },
  saveRows: (box, fmt, est) => drawRows(box, fmt, est),
  estimate: (fmt) => estimate(fmt),
  saveLine: (fmt) => saveLine(fmt),
  save(path, fmt) {
    lastRequest = saveRequest(fmt);
    return be.saveAs(path, lastRequest);
  },
  saved: (r, fmt) => afterSave(r, fmt),
  keys: (e) => keys(e),
});
const { app, stage } = shell;
const canvas = el('canvas');
canvas.id = 'wave';
canvas.setAttribute('role', 'img');
canvas.setAttribute('aria-label', T('波形'));
stage.appendChild(canvas);
const adjPanel = shell.sidePanel('adjPanel', 'adjust', '调音'),
  infoPanel = shell.sidePanel('infoPanel', 'info', '信息');
const adjBtn = document.getElementById('adjPanelBtn'),
  infoBtn = document.getElementById('infoPanelBtn');
adjBtn.id = 'adjBtn';
infoBtn.id = 'infoBtn';

// the dock: the same bar of buttons as Image's
const barBtn = (id, n, tip, run) => {
  const b = el('button', 'tb');
  b.type = 'button';
  b.id = id;
  b.innerHTML = icon(n);
  b.setAttribute('aria-label', tip);
  b.dataset.tip = tip;
  b.addEventListener('mousedown', (e) => e.preventDefault());
  b.addEventListener('click', run);
  return b;
};
const undoBtn = barBtn('undo', 'undo', '撤销 · {Mod+Z}', () => {
    sound();
    undo();
  }),
  redoBtn = barBtn('redo', 'redo', '重做 · {Mod+Y}', () => {
    sound();
    redo();
  });
const prevBtn = barBtn('prev', 'prev', '上一曲', () => {
    sound();
    shell.step(-1);
  }),
  playBtn = barBtn('play', 'play', '播放', () => {
    if (!player.playing) sound();
    togglePlay();
  }),
  nextBtn = barBtn('next', 'next', '下一曲', () => {
    sound();
    shell.step(1);
  });
const clockEl = el('span', 'clock'),
  fitBtn = el('button', 'tb fit', '全局波形');
fitBtn.type = 'button';
fitBtn.id = 'fit';
fitBtn.addEventListener('mousedown', (e) => e.preventDefault());
fitBtn.addEventListener('click', () => {
  sound();
  setView(0, total());
});
const pill = el('div', 'pill');
pill.append(undoBtn, redoBtn, el('i', 'sep'), prevBtn, playBtn, nextBtn, el('i', 'sep'), clockEl, fitBtn);
const selbar = el('div', 'selbar'),
  tip = el('div', 'tip');
selbar.id = 'selbar';
shell.place(pill, selbar, tip);

// ---------- the wave: bars that grow into whatever the sound now is ----------
const g = canvas.getContext('2d');
let W = 0,
  H = 0,
  dpr = 1,
  bars = 0,
  shown = new Float32Array(0),
  goal = new Float32Array(0),
  raw = new Float32Array(0),
  rawKey = '',
  barSeq = 0,
  colors = {};
const STEP = 4,
  GUT = 24;
const probe = el('i');
probe.style.display = 'none';
document.body.appendChild(probe);
function readColors() {
  const get = (c) => {
    probe.style.color = c;
    return getComputedStyle(probe).color;
  };
  colors = {
    played: get('var(--text)'),
    rest: get('color-mix(in srgb, var(--text) 34%, var(--bg))'),
    base: get('var(--glass-edge)'),
    line: get('var(--accent)'),
    sel: get('color-mix(in srgb, var(--text) 6%, transparent)'),
    edge: get('color-mix(in srgb, var(--text) 30%, transparent)'),
  };
  paint();
}
new MutationObserver(readColors).observe(root, { attributes: true, attributeFilter: ['data-theme'] });
matchMedia('(prefers-color-scheme: dark)').addEventListener('change', readColors);
const tox = (t) => GUT + ((t - vA) / (vZ - vA)) * (W - GUT * 2);
const tot = (x) => clamp(vA + ((x - GUT) / (W - GUT * 2)) * (vZ - vA), 0, total());
const midY = () => (H - 60) / 2;
const tall = () => Math.min((H - 60) * 0.72, 460);
function fit() {
  const r = stage.getBoundingClientRect();
  W = r.width;
  H = r.height;
  dpr = devicePixelRatio || 1;
  canvas.width = Math.round(W * dpr);
  canvas.height = Math.round(H * dpr);
  const n = Math.max(8, Math.floor((W - GUT * 2) / STEP));
  if (n !== bars) {
    bars = n;
    shown = new Float32Array(n);
    goal = new Float32Array(n);
  }
  measure(true);
}
// the height of every bar is the sound as it will actually come out. The peaks come from Rust; gain and fades are arithmetic here
function shape(snap) {
  if (raw.length !== bars) return;
  const span = (vZ - vA) / bars,
    k = gainNow(),
    all = total();
  for (let i = 0; i < bars; i++) {
    const t0b = vA + i * span;
    goal[i] = Math.min(1, Math.pow(Math.min(1, raw[i] * k * envelope(t0b + span / 2, all, s.fadeIn, s.fadeOut)), 0.78));
  }
  if (snap) shown.set(goal);
  paint();
}
function measure(snap) {
  if (!doc || !bars) return;
  const key = JSON.stringify(spans) + '|' + vA + '|' + vZ + '|' + bars;
  if (key === rawKey) {
    shape(snap);
    return;
  }
  rawKey = key;
  const seq = ++barSeq,
    span = vZ - vA,
    from = vA,
    n = bars;
  be.waveBars(spans, from, span, n).then((buf) => {
    if (seq !== barSeq) return;
    raw = new Float32Array(buf);
    shape(snap);
  });
}
let raf = 0;
const paint = () => {
  if (!raf) raf = requestAnimationFrame(frame);
};
function frame() {
  raf = 0;
  let settled = true;
  for (let i = 0; i < bars; i++) {
    const d = goal[i] - shown[i];
    if (Math.abs(d) > 0.002) {
      shown[i] += reduce ? d : d * 0.2;
      settled = false;
    } else shown[i] = goal[i];
  }
  draw();
  syncTime();
  if (!settled || player.playing) paint();
}
function draw() {
  g.setTransform(dpr, 0, 0, dpr, 0, 0);
  g.clearRect(0, 0, W, H);
  if (!doc) return;
  const cy = midY(),
    max = tall(),
    t = player.time(),
    px = tox(t);
  // while playing, the view keeps the line in sight
  if (player.playing && vZ - vA < total() - 0.01 && (t > vZ || t < vA)) {
    const span = vZ - vA;
    setView(clamp(t - span * 0.05, 0, total() - span), span);
    return;
  }
  if (sel) {
    const a = tox(sel.a),
      b = tox(sel.b);
    g.fillStyle = colors.sel;
    g.fillRect(a, cy - max / 2 - 4, b - a, max + 8);
    g.fillStyle = colors.edge;
    g.fillRect(a - 0.5, cy - max / 2 - 4, 1, max + 8);
    g.fillRect(b - 0.5, cy - max / 2 - 4, 1, max + 8);
  }
  g.fillStyle = colors.base;
  g.fillRect(0, Math.round(cy), W, 1);
  for (let i = 0; i < bars; i++) {
    const x = GUT + i * STEP,
      h = Math.max(2, shown[i] * max);
    g.fillStyle = x + 1 <= px ? colors.played : colors.rest;
    g.fillRect(x, cy - h / 2, 2, h);
  }
  if (px >= GUT - 1 && px <= W - GUT + 1) {
    g.fillStyle = colors.line;
    g.fillRect(px - 1, cy - max / 2 - 8, 2, max + 16);
  }
}
function setView(a, span) {
  span = clamp(span, Math.min(1, total()), total());
  vA = clamp(a, 0, total() - span);
  vZ = vA + span;
  syncZoom();
  measure(true);
  placeSel();
}
function zoomBy(f, around) {
  const span = vZ - vA,
    next = clamp(span / f, Math.min(1, total()), total()),
    t = around ?? (vA + vZ) / 2;
  setView(t - ((t - vA) / span) * next, next);
}
function syncZoom() {
  const hide = vZ - vA >= total() - 0.001;
  if (fitBtn.hidden === hide) return;
  const from = pill.offsetWidth;
  fitBtn.hidden = hide;
  if (reduce) return;
  pill.animate([{ width: from + 'px' }, { width: pill.offsetWidth + 'px' }], { duration: 240, easing: EASE });
  if (!hide) fitBtn.animate([{ opacity: 0 }, { opacity: 1 }], { duration: 240, easing: EASE });
}
function syncTime() {
  if (doc) clockEl.innerHTML = `${clock(player.time())} <i>/ ${clock(total())}</i>`;
}
function syncPlay() {
  const on = player.playing,
    label = T(on ? '暂停' : '播放');
  playBtn.innerHTML = icon(on ? 'pause' : 'play');
  playBtn.setAttribute('aria-label', label);
  playBtn.dataset.tip = label;
  paint();
}
function togglePlay() {
  if (!doc) return;
  if (player.playing) player.pause();
  else {
    pushAudio();
    player.play();
  }
  syncPlay();
}
player.onend = () => {
  syncPlay();
  paint();
};
player.onnodevice = () => {
  syncPlay();
  shell.say(T('未检测到可用的音频输出设备。'));
};

// ---------- editing: everything can be taken back ----------
const snapshot = () => ({ spans, s: { ...s } });
function remember() {
  past.push(snapshot());
  if (past.length > 60) past.shift();
  ahead = [];
  syncUndo();
}
// the dock's undo and redo, and the family's little dot
function syncUndo() {
  undoBtn.disabled = !past.length;
  redoBtn.disabled = !ahead.length;
  shell.changed();
}
function pushAudio() {
  if (doc) be.playerSet(recipe(), levelK);
}
function restore(st) {
  const changed = st.spans !== spans;
  const t = player.time(),
    was = player.playing;
  s = { ...st.s };
  spans = st.spans;
  sel = null;
  if (changed) {
    vA = 0;
    vZ = total();
    syncZoom();
    followLevel();
  }
  pushAudio();
  if (changed) {
    player.offset = Math.min(t, total());
    if (was) player.play(player.offset);
  }
  placeSel();
  syncControls();
  measure(false);
  syncUndo();
  syncPlay();
}
function undo() {
  if (!past.length) return;
  ahead.push(snapshot());
  restore(past.pop());
}
function redo() {
  if (!ahead.length) return;
  past.push(snapshot());
  restore(ahead.pop());
}
// how far from an easy listening level the song sits depends on what's left of it
function followLevel() {
  const list = spans;
  be.levelOf(list).then((k) => {
    if (list !== spans) return;
    levelK = k;
    pushAudio();
    measure(false);
  });
}
function replace(next, at) {
  remember();
  const was = player.playing;
  spans = next;
  sel = null;
  vA = 0;
  vZ = total();
  s.fadeIn = Math.min(s.fadeIn, total() / 2);
  s.fadeOut = Math.min(s.fadeOut, total() / 2);
  pushAudio();
  followLevel();
  player.offset = clamp(at, 0, total());
  if (was) player.play(player.offset);
  placeSel();
  syncZoom();
  syncControls();
  measure(false);
  syncPlay();
  syncUndo();
}
const toFrame = (t) => Math.round(t * doc.rate);
function deleteSel() {
  if (!sel) return;
  const { a, b } = sel;
  if (b - a >= total() - 0.05) return;
  const t = player.time();
  replace(cutSpans(spans, toFrame(a), toFrame(b)), t < a ? t : t > b ? t - (b - a) : a);
}
function keepSel() {
  if (!sel) return;
  const { a, b } = sel,
    t = player.time();
  replace(keepSpans(spans, toFrame(a), toFrame(b)), t >= a && t <= b ? t - a : 0);
}

const selTime = el('span', 'clock');
selTime.id = 'selTime';
function selBtn(id, n, label, fn) {
  const b = el('button', 'tb');
  b.type = 'button';
  b.id = id;
  b.innerHTML = icon(n) + `<span>${T(label)}</span>`;
  b.addEventListener('mousedown', (e) => e.preventDefault());
  b.addEventListener('click', () => {
    sound();
    fn();
  });
  return b;
}
function drawSelbar() {
  selbar.textContent = '';
  selbar.append(
    selTime,
    selBtn('sbPlay', 'play', '试听选区', () => {
      pushAudio();
      player.play(sel.a, sel.b);
      syncPlay();
    }),
    selBtn('sbKeep', 'crop', '仅保留选区', keepSel),
    selBtn('sbDel', 'trash', '删除选区', deleteSel)
  );
}
function placeSel() {
  if (!sel || sel.b - sel.a < 0.05) {
    selbar.classList.remove('on');
    return;
  }
  const len = sel.b - sel.a;
  selTime.textContent =
    len < 60
      ? T('{s} 秒', { s: num(len, { minimumFractionDigits: 1, maximumFractionDigits: 1, useGrouping: false }) })
      : clock(len);
  const r = stage.getBoundingClientRect(),
    box = app.getBoundingClientRect(),
    w = selbar.offsetWidth || 320;
  const mid = (Math.max(tox(sel.a), 0) + Math.min(tox(sel.b), W)) / 2;
  selbar.style.left = clamp(mid - w / 2, 12, box.width - w - 12) + 'px';
  selbar.style.top = Math.max(60, r.top + midY() - tall() / 2 - 62) + 'px';
  selbar.classList.add('on');
}

// ---------- hands on the wave ----------
let dragging = null,
  grab = 0,
  downX = 0,
  moved = false;
const edgeAt = (x) => {
  if (!sel) return null;
  if (Math.abs(x - tox(sel.a)) < 6) return 'a';
  if (Math.abs(x - tox(sel.b)) < 6) return 'b';
  return null;
};
const local = (e) => {
  const r = stage.getBoundingClientRect();
  return [e.clientX - r.left, e.clientY - r.top];
};
function showTip(text, x, y) {
  const r = stage.getBoundingClientRect(),
    w = tip.offsetWidth;
  tip.textContent = text;
  tip.style.left = clamp(r.left + x - w / 2, 4, innerWidth - w - 4) + 'px';
  tip.style.top = Math.max(56, r.top + y - 30) + 'px';
  tip.classList.add('on');
}
stage.addEventListener('pointerdown', (e) => {
  if (!doc || e.button) return;
  const [x] = local(e);
  stage.setPointerCapture(e.pointerId);
  downX = x;
  moved = false;
  const edge = edgeAt(x);
  if (edge) dragging = edge === 'a' ? 'selA' : 'selB';
  else {
    dragging = 'new';
    grab = tot(x);
  }
});
stage.addEventListener('pointermove', (e) => {
  if (!doc) return;
  const [x, y] = local(e);
  if (!dragging) {
    canvas.style.cursor = edgeAt(x) ? 'ew-resize' : 'text';
    if (y > midY() - tall() / 2 && y < midY() + tall() / 2) showTip(clock(tot(x)), x, midY() - tall() / 2);
    else tip.classList.remove('on');
    return;
  }
  if (Math.abs(x - downX) > 3) moved = true;
  if (dragging === 'selA' || dragging === 'selB') {
    const t = tot(x),
      other = dragging === 'selA' ? sel.b : sel.a;
    sel = { a: Math.min(t, other), b: Math.max(t, other) };
    dragging = t < other ? 'selA' : 'selB';
    placeSel();
    paint();
  } else if (moved) {
    const t = tot(x);
    sel = { a: Math.min(grab, t), b: Math.max(grab, t) };
    tip.classList.remove('on');
    placeSel();
    paint();
  }
});
stage.addEventListener('pointerup', (e) => {
  if (!dragging) return;
  const [x] = local(e),
    was = dragging;
  dragging = null;
  if (was === 'new' && !moved) {
    sel = null;
    placeSel();
    player.seek(tot(x));
    syncPlay();
  }
  paint();
});
stage.addEventListener('pointerleave', () => {
  if (!dragging) tip.classList.remove('on');
});
stage.addEventListener(
  'wheel',
  (e) => {
    if (!doc) return;
    e.preventDefault();
    const [x] = local(e);
    if (e.ctrlKey || e.metaKey) zoomBy(e.deltaY < 0 ? 1.25 : 0.8, tot(x));
    else if (fromTrackpad && fromTrackpad(e) === 'trackpad') {
      // the Mac's trackpad: the wave moves under the fingers, a pixel for a pixel, along the stronger direction
      const d = Math.abs(e.deltaX) >= Math.abs(e.deltaY) ? e.deltaX : e.deltaY,
        span = vZ - vA;
      if (span < total()) setView(vA + (d / (W - GUT * 2)) * span, span);
    } else if (vZ - vA < total()) {
      const span = vZ - vA;
      setView(vA + ((e.deltaY || e.deltaX) / 600) * span, span);
    }
  },
  { passive: false }
);
// the Mac's pinch zooms the time about the fingers (gestures.js)
const fromTrackpad = mac ? wheelSource() : null;
pinch(stage, {
  zoom: (k, x) => {
    if (doc) zoomBy(k, tot(x - stage.getBoundingClientRect().left));
  },
});

// the family's keys come first (open, save as, Escape puts away what's open); then Audio's
function keys(e) {
  const mod = modKey(e),
    k = e.key,
    typing = /INPUT|TEXTAREA/.test(document.activeElement.tagName) && document.activeElement.type !== 'range';
  if (k === 'Escape') {
    if (sel) {
      sel = null;
      placeSel();
      paint();
    }
    return;
  }
  if (typing || !doc) return;
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
  if (k === ' ') {
    e.preventDefault();
    togglePlay();
    return;
  }
  if (document.activeElement.type === 'range') return;
  if (k === 'ArrowRight' || k === 'ArrowLeft') {
    e.preventDefault();
    player.seek(clamp(player.time() + (k === 'ArrowRight' ? 5 : -5), 0, total()));
    paint();
    syncTime();
  } else if (k === 'Delete' || k === 'Backspace') {
    e.preventDefault();
    deleteSel();
  } else if (k === 'Home') {
    e.preventDefault();
    player.seek(0);
    paint();
    syncTime();
  }
}

// ---------- the adjust panel ----------
// whatever changes size eases to it, and what's new fades in; nothing jumps
const morph = shell.morph;
let syncers = [];
function slider({ id, label, min, max, step: st, get, input, fmt, center }) {
  const w = el('label', 'slide'),
    top = el('span', 'slide-top'),
    val = el('span');
  top.append(el('b', '', T(label)), val);
  const i = document.createElement('input');
  i.type = 'range';
  i.id = id;
  i.min = min;
  i.max = max;
  i.step = st;
  i.setAttribute('aria-label', T(label));
  const paintIt = () => {
    i.value = get();
    const p = ((i.value - min) / (max - min)) * 100,
      c = center ? 50 : 0;
    i.style.setProperty('--a', Math.min(p, c) + '%');
    i.style.setProperty('--b', Math.max(p, c) + '%');
    val.textContent = fmt(+i.value);
  };
  let started = false;
  i.addEventListener('input', () => {
    if (!started) {
      remember();
      started = true;
    }
    input(+i.value);
    paintIt();
    syncUndo();
  });
  i.addEventListener('change', () => {
    started = false;
  });
  syncers.push(paintIt);
  paintIt();
  w.append(top, i);
  return w;
}
function drawAdjust() {
  adjPanel.textContent = '';
  const body = el('div', 'side-body');
  const levelRow = el('button', 'row compact');
  levelRow.type = 'button';
  levelRow.id = 'levelTog';
  levelRow.setAttribute('role', 'switch');
  levelRow.innerHTML = `<span class="row-t"><b>${T('响度均衡')}</b></span><i class="tog"></i>`;
  levelRow.addEventListener('mousedown', (e) => e.preventDefault());
  levelRow.addEventListener('click', () => {
    sound();
    remember();
    s.level = !s.level;
    pushAudio();
    syncControls();
    measure(false);
    syncUndo();
  });
  syncers.push(() => levelRow.setAttribute('aria-checked', String(s.level)));
  const spaceBox = el('div'),
    chips = el('div', 'chips');
  chips.id = 'spaceChips';
  const wetRow = () => spaceBox.querySelector('.wet');
  const chipBtns = [
    ['off', '关闭::混响'],
    ['room', '室内'],
    ['hall', '音乐厅'],
  ].map(([v, t]) => {
    const b = el('button', v === 'off' ? 'chip dyn' : 'chip', T(t));
    /* 关闭::混响 is Off: the family already says 关闭 as Close */ b.type = 'button';
    b.dataset.v = v;
    b.addEventListener('mousedown', (e) => e.preventDefault());
    b.addEventListener('click', () => {
      sound();
      remember();
      const was = s.space;
      s.space = v;
      pushAudio();
      morph(spaceBox, syncControls, () => (was === 'off' && v !== 'off' ? [wetRow()] : []));
      syncUndo();
    });
    chips.appendChild(b);
    return b;
  });
  syncers.push(() => chipBtns.forEach((b) => b.setAttribute('aria-pressed', String(b.dataset.v === s.space))));
  const wet = slider({
    id: 'wetSl',
    label: '混响强度',
    min: 0,
    max: 100,
    step: 1,
    get: () => s.wet,
    input: (v) => {
      s.wet = v;
      pushAudio();
    },
    fmt: (v) => v + '%',
  });
  wet.classList.add('wet');
  syncers.push(() => {
    wet.hidden = s.space === 'off';
  });
  spaceBox.append(el('span', 'side-label', T('空间混响')), chips, wet);
  const resetRow = el('div', 'side-row'),
    reset = el('button', 'btn soft', T('重置全部参数'));
  reset.type = 'button';
  reset.id = 'resetAll';
  reset.addEventListener('mousedown', (e) => e.preventDefault());
  reset.addEventListener('click', () => {
    sound();
    remember();
    s = FRESH();
    pushAudio();
    syncControls();
    measure(false);
    syncUndo();
  });
  resetRow.appendChild(reset);
  body.append(
    slider({
      id: 'volSl',
      label: '音量增益',
      min: -12,
      max: 12,
      step: 0.5,
      center: true,
      get: () => s.db,
      input: (v) => {
        s.db = v;
        pushAudio();
        measure(false);
      },
      fmt: (v) => (v > 0 ? '+' : '') + v.toFixed(1) + ' dB',
    }),
    levelRow,
    slider({
      id: 'inSl',
      label: '淡入',
      min: 0,
      max: 10,
      step: 0.1,
      get: () => Math.min(s.fadeIn, 10),
      input: (v) => {
        s.fadeIn = Math.min(v, total() / 2);
        pushAudio();
        measure(false);
      },
      fmt: (v) => v.toFixed(1) + ' s',
    }),
    slider({
      id: 'outSl',
      label: '淡出',
      min: 0,
      max: 10,
      step: 0.1,
      get: () => Math.min(s.fadeOut, 10),
      input: (v) => {
        s.fadeOut = Math.min(v, total() / 2);
        pushAudio();
        measure(false);
      },
      fmt: (v) => v.toFixed(1) + ' s',
    }),
    spaceBox,
    resetRow
  );
  adjPanel.append(el('h3', '', T('调音')), body);
}
function syncControls() {
  syncers.forEach((f) => f());
  paint();
}

// ---------- the info panel ----------
async function pickCover() {
  const r = await be.pickCover(T('图像'));
  takeCover(r);
}
function takeCover(r) {
  if (r.status === 'refused') {
    shell.say(T('专辑封面仅支持 JPEG 与 PNG 格式。'));
    return;
  }
  if (r.status !== 'ok') return;
  meta.cover = r.cover;
  coverNew = true;
  metaDirty = true;
  syncUndo();
  drawInfo();
}
const channelWord = (n) => (n === 1 ? T('单声道') : n === 2 ? T('立体声') : T('{n} 声道', { n }));
function factsLine() {
  const name = shell.fileName,
    at = name.lastIndexOf('.'),
    kind = at > 0 ? name.slice(at + 1).toUpperCase() : '',
    hz = doc.fileRate / 1000;
  return [kind, clock(total()), (hz % 1 ? hz.toFixed(1) : hz) + ' kHz', channelWord(doc.fileChannels)]
    .filter(Boolean)
    .join(' · ');
}
function drawInfo() {
  infoPanel.textContent = '';
  if (!doc) return;
  const cover = el('button', 'cover-big' + (meta.cover ? '' : ' none'));
  cover.type = 'button';
  cover.id = 'coverBtn';
  cover.addEventListener('mousedown', (e) => e.preventDefault());
  cover.addEventListener('click', () => {
    sound();
    pickCover();
  });
  if (meta.cover) {
    const i = el('img');
    i.src = meta.cover;
    i.alt = '';
    cover.append(i, el('span', 'cover-tip', T('更换封面')));
  } else {
    cover.innerHTML = icon('image');
    cover.appendChild(el('span', '', T('添加封面')));
  }
  const words = el('div', 'song-words');
  for (const [k, label] of [
    ['title', '曲目名称'],
    ['artist', '艺术家'],
    ['album', '专辑'],
  ]) {
    const field = el('label', 'field word-' + k),
      box = el('span', 'field-in'),
      i = document.createElement('input');
    i.id = 'f-' + k;
    i.value = meta[k] || '';
    i.spellcheck = false;
    i.autocomplete = 'off';
    i.placeholder = T(label);
    i.setAttribute('aria-label', T(label));
    i.addEventListener('input', () => {
      meta[k] = i.value;
      metaDirty = tagsChanged();
      syncUndo();
    });
    box.appendChild(i);
    field.appendChild(box);
    words.appendChild(field);
    inkfall(i);
  }
  const facts = el('p', 'facts', factsLine());
  facts.id = 'facts';
  infoPanel.append(el('h3', '', T('信息')), cover, words, facts);
}
const tagsChanged = () => coverNew || ['title', 'artist', 'album'].some((k) => (meta[k] || '') !== (meta0[k] || ''));
const tagEdit = () => {
  const one = (k) => ((meta[k] || '') !== (meta0[k] || '') ? meta[k] || '' : null);
  return { title: one('title'), artist: one('artist'), album: one('album'), cover: coverNew };
};

// ---------- the save card's options: the quality of a lossy format, and the estimate with its note ----------
// nobody has to pick a quality for a file that needn't be compressed again. A quality chosen is the drawn format's: the
// card asks about a format it is switching to before drawing its rows
const sameSound = (fmt) => fmt === doc.kind && recipeKey() === savedKey;
const qualityNow = (fmt) =>
  FORMAT[fmt].lossy ? (fmt === rowsFmt && chosenQuality) || (sameSound(fmt) ? null : 'hi') : null;
const saveRequest = (fmt) => ({ kind: fmt, quality: qualityNow(fmt), recipe: recipe(), tags: tagEdit() });
function drawRows(box, fmt, est) {
  // another format starts on its own quality
  if (rowsFmt !== fmt) {
    if (rowsFmt) chosenQuality = null;
    rowsFmt = fmt;
  }
  cardFmt = fmt;
  if (!FORMAT[fmt].lossy) return;
  const wrap = el('div'),
    chips = el('div', 'chips');
  chips.id = 'qualityChips';
  const q = qualityNow(fmt);
  [
    ['std', '标准'],
    ['hi', '高'],
    ['top', '极高'],
  ].forEach(([v, t]) => {
    const b = el('button', 'chip', T(t));
    b.type = 'button';
    b.dataset.v = v;
    b.setAttribute('aria-pressed', String(v === q));
    b.addEventListener('mousedown', (e) => e.preventDefault());
    b.addEventListener('click', () => {
      sound();
      chosenQuality = v;
      chips.querySelectorAll('.chip').forEach((c) => c.setAttribute('aria-pressed', String(c.dataset.v === v)));
      est();
    });
    chips.appendChild(b);
  });
  wrap.append(el('span', 'side-label', T('音质')), chips);
  box.appendChild(wrap);
}
// the size and the quiet line under the formats both come from Rust, and change with the options.
// Every format that is only read says M4A's sentence, with its own name where M4A's is (MOD, XM, S3M, IT, AU, VOC):
// every language's sentence has the name in it as it is, so no new words are needed
const READ_ONLY_NOTE = 'M4A 格式当前仅支持读取，修改后将另存为新文件，原文件保持不变。';
const noteWords = (n) =>
  n === READ_ONLY_NOTE && doc && !FORMAT[doc.kind] ? T(n).replace('M4A', doc.kind.toUpperCase()) : T(n);
function estimate(fmt) {
  if (!doc) return null;
  return be
    .estimateSave(++estSeq, saveRequest(fmt))
    .then((plan) => (plan ? { size: plan.size, line: plan.notes.map(noteWords).join(' ') } : null));
}
// the same notes at once, without the encode, for a format the card is switching to (the size follows)
function saveLine(fmt) {
  if (!doc) return undefined;
  return be.saveNotes(saveRequest(fmt)).then((notes) => (notes ? notes.map(noteWords).join(' ') : undefined));
}
function afterSave(r, fmt) {
  doc.path = r.path;
  doc.kind = fmt;
  doc.fileRate = r.rate;
  doc.fileChannels = r.channels;
  doc.beside = Math.max(doc.beside, 1);
  // what's saved is what the card was asked for, even if the song changed while it was written
  savedKey = keyOf(lastRequest.recipe);
  meta0 = { ...meta };
  coverNew = false;
  metaDirty = false;
  drawInfo();
}

// ---------- opening and closing ----------
function openDoc(r) {
  player.reset();
  doc = {
    path: r.path,
    dir: r.dir,
    kind: r.kind,
    rate: r.rate,
    channels: r.channels,
    frames: r.frames,
    beside: r.beside,
    fileRate: r.rate,
    fileChannels: r.channels,
  };
  meta = { ...r.tags };
  meta0 = { ...r.tags };
  coverNew = false;
  s = FRESH();
  spans = [[0, r.frames]];
  past = [];
  ahead = [];
  metaDirty = false;
  sel = null;
  rawKey = '';
  raw = new Float32Array(0);
  // a song with no name of its own shows its file's, and it stays that way unless someone changes it
  if (!meta.title) {
    meta.title = r.name.replace(/\.[^.]+$/, '');
    meta0.title = meta.title;
  }
  savedKey = recipeKey();
  levelK = r.level;
  vA = 0;
  vZ = total();
  pushAudio();
  redraw();
  fit();
  shown.fill(0);
  measure(false);
  syncZoom();
  syncUndo();
  syncPlay();
  placeSel();
  prevBtn.disabled = nextBtn.disabled = r.beside < 2;
}
function closeDoc() {
  player.reset();
  doc = null;
  meta = {};
  meta0 = {};
  sel = null;
  spans = [];
  past = [];
  ahead = [];
  undoBtn.disabled = redoBtn.disabled = true;
  placeSel();
  syncPlay();
  infoPanel.textContent = '';
}
function redraw() {
  syncers = [];
  drawAdjust();
  drawInfo();
  drawSelbar();
  syncControls();
}
new ResizeObserver(() => {
  if (doc) {
    fit();
    placeSel();
  }
}).observe(app);

// ---------- words: the family's translate in place; Audio's own parts too ----------
shell.translates(pill);
shell.onLang(() => {
  canvas.setAttribute('aria-label', T('波形'));
  redraw();
  syncPlay();
  if (doc) placeSel();
});

// ---------- begin ----------
// with DOTANY_TEST set, shell.test exposes the shell's hooks and Audio's state and actions, so that an automated test
// can read and drive it
Object.defineProperties(shell.test, {
  be: { value: be },
  player: { value: player },
  takeCover: { value: takeCover },
  gainNow: { value: gainNow },
  total: { value: total },
  doc: { get: () => doc },
  s: { get: () => s },
  spans: { get: () => spans },
  sel: {
    get: () => sel,
    set: (v) => {
      sel = v;
      placeSel();
      paint();
    },
  },
  past: { get: () => past.length },
  meta: { get: () => meta },
  levelK: { get: () => levelK },
  goal: { get: () => goal },
  raw: { get: () => raw },
  vA: { get: () => vA },
  vZ: { get: () => vZ },
});
readColors();
syncUndo();
shell.begin();
