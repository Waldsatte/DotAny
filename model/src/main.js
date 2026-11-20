// DotAny 3D: a model on the family's page. The window around it, from the top bar to settings and the save card, is
// shared/web; the stage, the dock, the ruler, the section cut, the parts and the facts are here.
import { createShell } from '@dotany/web/shell.js';
import { el, icon, root, reduce, sound } from '@dotany/web/shared.js';
import { T, num, size as sizeText } from '@dotany/web/i18n.js';
import { chips, toggleRow } from '@dotany/web/parts.js';
import { baseName } from '@dotany/web/paths.js';
import { LUCIDE } from './icons.js';
import * as own from './backend.js';
// three.js, the stage and the readers are loaded when a model is opened, not with the empty window: they are most of
// the page, and the empty window doesn't draw anything with them. Once the window is ready and quiet they are loaded in
// the background, so opening the first model doesn't wait for them
let viewer = null,
  THREE = null,
  M = null,
  enginePromise = null,
  prepared = null;
const loadEngine = () =>
  (enginePromise ||= Promise.all([import('./viewer.js'), import('./models.js')]).then(
    ([v, m]) => {
      THREE = v.THREE;
      M = m;
      return v;
    },
    (e) => {
      enginePromise = null;
      throw e;
    }
  ));

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
// the save card's formats. FBX, 3MF, PLY, OFF, G-code and VOX open but aren't written; a picture of the stage is one
// more, a copy that isn't the model
const FORMATS = {
  glb: { ext: '.glb', label: 'GLB' },
  obj: { ext: '.obj', label: 'OBJ' },
  stl: { ext: '.stl', label: 'STL' },
  png: { ext: '.png', label: 'PNG', export: true },
};
// what a format only read is saved as first: the format that keeps most of it (colours: GLB; plain geometry: OBJ or STL)
const SAVED_AS = { '3mf': 'stl', ply: 'glb', off: 'obj', gcode: 'glb', vox: 'glb' };
const MOVE = { duration: 240, easing: 'cubic-bezier(.2,.7,.2,1)' },
  GROW = { duration: 420, easing: 'cubic-bezier(.2,.7,.2,1)' };
const clamp = (v, a, b) => Math.min(b, Math.max(a, v));
const grouped = (n) => num(Math.round(n));
const money = (b) => sizeText(b, { zero: true, gb: true });

// ---------- what's open ----------
// model: what readModel gave. src: the bytes of the file the window is on, its format, and which parts were left out
let opened = false,
  model = null,
  src = null,
  fileSize = 0,
  openSeq = 0;
// with DOTANY_TEST set, shell.test.touch counts as a change the way an editing tool's edit would (DotAny 3D only
// views, so nothing else makes one)
let edits = 0,
  savedAt = 0;

const shell = createShell({
  name: 'DotAny 3D',
  icons: LUCIDE,
  words: WORDS,
  notices: () => import('./notices.generated.js'),
  dropTitle: '拖拽三维模型至此',
  fileKind: '三维模型',
  licences: [
    ['three.js', '三维渲染引擎', 'MIT'],
    ['Draco', '网格解压组件', 'Apache-2.0'],
  ],
  formats: FORMATS,
  formatOf: () => (!model ? 'glb' : SAVED_AS[model.kind] || model.kind),
  // a path or a cloud (G-code, a PLY of points) has no faces: STL can't hold it
  formatsFor: () => (model && !M.statsOf(model.root).tris ? ['glb', 'obj', 'png'] : ['glb', 'obj', 'stl', 'png']),
  cantOpen: {
    damaged: '文件已损坏，或并非有效的三维模型文件。',
    compressed: '该模型使用了暂不支持的几何压缩算法，无法载入。',
    oldfbx: '该 FBX 文件的版本过于早期，暂不支持解析。',
  },
  open: openModel,
  show: (r) => hold(r._m, r.name, r.size, r._bytes),
  clear: goEmpty,
  dirty: () => edits !== savedAt,
  discard() {
    savedAt = edits;
  },
  estimate,
  save: saveTo,
  // the window is on the new file now: saving it again in its own format writes the same bytes
  saved(r, fmt) {
    savedAt = edits;
    if (fmt !== 'png' && written) src = { kind: fmt, bytes: written.bytes, key: written.key };
  },
  saveRows,
  saveNote: (fmt) =>
    fmt === 'png'
      ? ''
      : sameFile(fmt)
        ? '完整保留原始三维数据。'
        : fmt === 'stl'
          ? '仅导出三维几何网格，不包含表面色彩与骨骼动画。'
          : fmt === 'obj'
            ? '导出静态模型，不包含动画序列与外部贴图。'
            : '',
  keys,
});
const { stage } = shell;

// ---------- the stage ----------
const over = el('div', 'm-over'),
  overSvg = document.createElementNS('http://www.w3.org/2000/svg', 'svg');
overSvg.setAttribute('class', 'm-svg');
over.appendChild(overSvg);
stage.appendChild(over);
new MutationObserver(() => {
  if (viewer) viewer.syncTheme();
}).observe(root, { attributes: true, attributeFilter: ['data-theme'] });
matchMedia('(prefers-color-scheme: dark)').addEventListener('change', () => {
  if (viewer) viewer.syncTheme();
});
// the stage itself, made the first time a model is opened
async function stageReady() {
  const { Viewer } = await loadEngine();
  if (viewer) return;
  viewer = new Viewer(stage, reduce, prepared);
  prepared = null;
  stage.insertBefore(viewer.renderer.domElement, over);
  viewer.onUserMove = () => {
    viewPick.set(null);
  };
  viewer.onFrame = () => drawOverlay();
  viewer.onTime = () => syncTime();
  listen(viewer.renderer.domElement);
  plane = new THREE.Plane();
  AX = { x: new THREE.Vector3(1, 0, 0), y: new THREE.Vector3(0, 1, 0), z: new THREE.Vector3(0, 0, 1) };
  hoverBox = new THREE.Box3Helper(new THREE.Box3(), 0xff6a00);
  selBox = new THREE.Box3Helper(new THREE.Box3(), 0x808080);
  hoverBox.layers.set(1);
  selBox.layers.set(1);
  hoverBox.material.depthTest = selBox.material.depthTest = false;
  hoverBox.material.transparent = selBox.material.transparent = true;
  viewer.helpers.add(hoverBox, selBox);
  hoverBox.visible = selBox.visible = false;
  syncPads();
}

// ---------- the panels: DotAny Image's, opened from the top bar ----------
const partsSide = shell.sidePanel('parts', 'parts', '模型结构'),
  infoSide = shell.sidePanel('info', 'info', '信息');
const partsBtn = document.getElementById('partsBtn'),
  infoBtn = document.getElementById('infoBtn');

// ---------- the dock ----------
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
// a choice in the dock: Archive's chips with the sliding dark block
function pickGroup(opts, idp, pick, extra = '') {
  const box = el('div', 'seg m-pick ' + extra),
    pl = el('i', 'pl');
  box.appendChild(pl);
  const btns = opts.map(([k, tx]) => {
    const b = el('button', 'sg', tx);
    b.type = 'button';
    b.id = idp + '-' + k;
    b.dataset.k = k;
    b.addEventListener('mousedown', (e) => e.preventDefault());
    b.addEventListener('click', () => {
      sound();
      pick(k);
    });
    box.appendChild(b);
    return b;
  });
  let cur = null,
    seen = false;
  const place = () => {
    const on = btns.find((b) => String(b.dataset.k) === String(cur));
    btns.forEach((b) => b.setAttribute('aria-pressed', String(b === on)));
    if (!on) {
      pl.style.opacity = 0;
      return;
    }
    if (!box.offsetParent) return;
    if (!seen || pl.style.opacity === '0') {
      pl.style.transition = 'opacity var(--t-1) var(--e-out)';
      seen = true;
    }
    pl.style.opacity = '';
    pl.style.transform = `translate(${on.offsetLeft}px, ${on.offsetTop}px)`;
    pl.style.width = on.offsetWidth + 'px';
    pl.style.height = on.offsetHeight + 'px';
    if (pl.style.transition) {
      void pl.offsetWidth;
      pl.style.transition = '';
    }
  };
  return {
    box,
    place,
    set(v) {
      cur = v;
      place();
    },
    get value() {
      return cur;
    },
  };
}
const viewPick = pickGroup(
  [
    ['iso', '透视图'],
    ['front', '正视图'],
    ['side', '侧视图'],
    ['top', '顶视图'],
  ],
  'view',
  (k) => {
    viewer.setView(k);
    viewPick.set(k);
  }
);
const modePick = pickGroup(
  [
    ['original', '材质渲染'],
    ['clay', '素模（白模）'],
    ['wire', '线框模式'],
  ],
  'mode',
  (k) => {
    viewer.setMode(k);
    modePick.set(k);
  }
);
const measureBtn = barBtn('measureBtn', 'ruler', '测量标尺 · M', () => setMeasure(!measuring)),
  sectionBtn = barBtn('sectionBtn', 'slice', '剖切面 · C', () => setSectioning(!sectioning));
[measureBtn, sectionBtn].forEach((b) => b.setAttribute('aria-pressed', 'false'));
const pill = el('div', 'pill m-dock'),
  pillRow = el('div', 'pill-row');
const modeSep = el('i', 'sep'),
  toolSep = el('i', 'sep');
pillRow.append(viewPick.box, modeSep, modePick.box, toolSep, measureBtn, sectionBtn);
// rows that rise when there's something more to do, the way DotAny Video's does when it starts trimming
const secRow = el('div', 'pill-row m-row'),
  animRow = el('div', 'pill-row m-row');
secRow.id = 'secRow';
animRow.id = 'animRow';
// the display-mode choice rises to this row when the dock's one row doesn't fit beside an open panel, in a longer
// language or a narrow window; it comes back down once the row fits again
const modeRow = el('div', 'pill-row m-row m-mode');
modeRow.id = 'modeRow';
modeRow.hidden = true;
const axisPick = pickGroup(
  [
    ['x', 'X'],
    ['y', 'Y'],
    ['z', 'Z'],
  ],
  'axis',
  (k) => {
    axisPick.set(k);
    applySection();
  }
);
const secFlip = barBtn('secFlip', 'flip', '翻转剖面方向', () => {
  secSide = -secSide;
  applySection();
});
const secSlider = document.createElement('input');
secSlider.type = 'range';
secSlider.id = 'secRange';
secSlider.min = 0;
secSlider.max = 100;
secSlider.value = 50;
secSlider.className = 'm-range';
secSlider.setAttribute('aria-label', '剖切面 · C');
secSlider.addEventListener('input', () => applySection());
const secVal = el('span', 'm-num dyn', '');
secVal.id = 'secVal';
secRow.append(axisPick.box, secSlider, secVal, secFlip);
const playBtn = barBtn('playBtn', 'play', '播放动画 · 空格', () => togglePlay()),
  timeEl = el('span', 'm-num m-time dyn', '0:00 / 0:00'),
  track = el('div', 'm-track'),
  trackFill = el('i', 'm-fill'),
  trackHead = el('i', 'm-head'),
  clipBtn = el('button', 'tb m-clip dyn', '');
timeEl.id = 'animTime';
track.id = 'animTrack';
track.append(trackFill, trackHead);
clipBtn.type = 'button';
clipBtn.id = 'clipBtn';
clipBtn.addEventListener('mousedown', (e) => e.preventDefault());
animRow.append(playBtn, timeEl, track, clipBtn);
pill.append(secRow, animRow, modeRow, pillRow);
secRow.hidden = animRow.hidden = true;
shell.place(pill);
shell.translates(pill);
const panelW = () => (partsSide.classList.contains('on') || infoSide.classList.contains('on') ? 288 : 0);
const risen = () => (secRow.hidden ? 0 : 48) + (animRow.hidden ? 0 : 48) + (modeRow.hidden ? 0 : 48);
const syncPads = () => {
  if (viewer) viewer.setPad(panelW(), risen());
};
function dockGrow(change, timing = MOVE) {
  if (reduce) {
    change();
    return;
  }
  pill.getAnimations().forEach((q) => q.cancel());
  const w0 = pill.offsetWidth,
    h0 = pill.offsetHeight;
  change();
  const w1 = pill.offsetWidth,
    h1 = pill.offsetHeight;
  pill.style.overflow = 'hidden';
  const f = pill.animate(
    [
      { width: w0 + 'px', height: h0 + 'px' },
      { width: w1 + 'px', height: h1 + 'px' },
    ],
    timing
  );
  f.onfinish = f.oncancel = () => {
    pill.style.overflow = '';
  };
}
// the one row's width, wherever the display-mode choice is now: its parts, their gaps, the dock's padding and edge
const outer = (e) => {
  const cs = getComputedStyle(e);
  return e.offsetWidth + (parseFloat(cs.marginLeft) || 0) + (parseFloat(cs.marginRight) || 0);
};
const oneRow = () =>
  [viewPick.box, toolSep, modePick.box, toolSep, measureBtn, sectionBtn].reduce((w, e) => w + outer(e), 0) + 2 * 5 + 14;
function fitMode() {
  if (!opened || !pill.offsetParent) return;
  const up = oneRow() > innerWidth - panelW() - 24;
  if (up === !modeRow.hidden) return;
  dockGrow(() => {
    if (up) {
      modeRow.append(modePick.box);
      modeSep.hidden = true;
    } else {
      modeSep.hidden = false;
      modeSep.after(modePick.box);
    }
    modeRow.hidden = !up;
  });
  if (!reduce) modePick.box.animate([{ opacity: 0 }, { opacity: 1 }], { ...MOVE, delay: 60, fill: 'backwards' });
  modePick.place();
  syncPads();
}
let fitting = 0;
const refitMode = () => {
  if (!fitting)
    fitting = requestAnimationFrame(() => {
      fitting = 0;
      fitMode();
    });
};
new ResizeObserver(refitMode).observe(document.documentElement);
[partsSide, infoSide].forEach((p) =>
  new MutationObserver(refitMode).observe(p, { attributes: true, attributeFilter: ['class'] })
);
if (document.fonts) document.fonts.addEventListener('loadingdone', refitMode);
const riseRow = (row, on) => {
  if (row.hidden !== on) return;
  dockGrow(() => {
    row.hidden = !on;
  });
  if (on && !reduce) row.animate([{ opacity: 0 }, { opacity: 1 }], { ...MOVE, delay: 60, fill: 'backwards' });
  syncPads();
};

// ---------- how long things are: a file's numbers mean what its unit says ----------
const toMm = (v) => v * M.UNITS[model.unit];
function len(v) {
  const mm = toMm(v),
    a = Math.abs(mm),
    f = (x, d) => num(+x.toFixed(d));
  if (model.unit === 'in') return f(mm / 25.4, a / 25.4 < 10 ? 3 : 2) + ' in';
  if (a >= 1000) return f(mm / 1000, 2) + ' m';
  if (a >= 100 && model.unit !== 'mm') return f(mm / 10, 1) + ' cm';
  return f(mm, a < 10 ? 2 : 1) + ' mm';
}
function vol(v) {
  const k = (M.UNITS[model.unit] ** 3 * v) / 1000;
  return k >= 1000000
    ? num(+(k / 1000000).toFixed(2)) + ' m³'
    : (+k.toFixed(k < 10 ? 2 : 1)).toLocaleString('en-US') + ' cm³';
}
// the size in the file's own numbers: the box, in the model's own frame, so standing it up doesn't swap the sides
const worldSize = () => {
  const s = new THREE.Vector3();
  viewer.box.getSize(s);
  return s;
};

// ---------- the overlay: lines are drawn in the page, so they stay thin at any size ----------
const svgEl = (n, a) => {
  const e = document.createElementNS('http://www.w3.org/2000/svg', n);
  for (const k in a) e.setAttribute(k, a[k]);
  return e;
};
const labels = new Map();
function label(key, x, y, text, hot) {
  let e = labels.get(key);
  if (!e) {
    e = el('span', 'm-label');
    e.dataset.k = key;
    labels.set(key, e);
    over.appendChild(e);
    if (!reduce) e.animate([{ opacity: 0 }, { opacity: 1 }], MOVE);
  }
  if (e.textContent !== text) e.textContent = text;
  e.classList.toggle('hot', !!hot);
  e.style.transform = `translate(calc(${x}px - 50%), calc(${y}px - 50%))`;
  e._seen = true;
}
function drawOverlay() {
  overSvg.textContent = '';
  labels.forEach((e) => {
    e._seen = false;
  });
  if (opened && measuring) {
    const b = viewer.box,
      P = (x, y, z) => viewer.project(new THREE.Vector3(x, y, z)),
      { min: a, max: c } = b;
    // the three edges nearest the viewer carry the size
    const cam = viewer.camera.position,
      near = (lo, hi, v) => (Math.abs(v - lo) < Math.abs(v - hi) ? lo : hi),
      nx = near(a.x, c.x, cam.x),
      nz = near(a.z, c.z, cam.z),
      s = worldSize();
    const edges = [
      [[a.x, a.y, nz], [c.x, a.y, nz], 'sx', s.x],
      [[nx, a.y, a.z], [nx, a.y, c.z], 'sz', s.z],
      [[a.x + c.x - nx, a.y, nz], [a.x + c.x - nx, c.y, nz], 'sy', s.y],
    ];
    for (const [p, q, k, v] of edges) {
      const A = P(...p),
        B = P(...q);
      overSvg.appendChild(svgEl('line', { x1: A[0], y1: A[1], x2: B[0], y2: B[1], class: 'm-edge' }));
      label(k, (A[0] + B[0]) / 2, (A[1] + B[1]) / 2, len(v));
    }
    points.forEach((m, i) => {
      const A = viewer.project(m.a);
      if (m.b) {
        const B = viewer.project(m.b);
        overSvg.appendChild(svgEl('line', { x1: A[0], y1: A[1], x2: B[0], y2: B[1], class: 'm-dist' }));
        overSvg.appendChild(svgEl('circle', { cx: B[0], cy: B[1], r: 4, class: 'm-pt' }));
        label(
          'd' + i,
          (A[0] + B[0]) / 2,
          (A[1] + B[1]) / 2,
          len(m.a.distanceTo(m.b) / scaleNow()),
          i === points.length - 1
        );
      }
      overSvg.appendChild(svgEl('circle', { cx: A[0], cy: A[1], r: 4, class: 'm-pt' }));
    });
    if (hoverPt) {
      const H = viewer.project(hoverPt),
        open_ = points.length && !points[points.length - 1].b ? points[points.length - 1] : null;
      if (open_) {
        const A = viewer.project(open_.a);
        overSvg.appendChild(svgEl('line', { x1: A[0], y1: A[1], x2: H[0], y2: H[1], class: 'm-dist live' }));
        label('live', (A[0] + H[0]) / 2, (A[1] + H[1]) / 2, len(open_.a.distanceTo(hoverPt) / scaleNow()), true);
      }
      overSvg.appendChild(svgEl('circle', { cx: H[0], cy: H[1], r: 4, class: 'm-pt live' }));
    }
  }
  labels.forEach((e, k) => {
    if (!e._seen) {
      e.remove();
      labels.delete(k);
    }
  });
}
// the model's own root may be scaled inside the file; distances are read in the file's numbers
const scaleNow = () => 1;

// ---------- measuring ----------
let measuring = false,
  points = [],
  hoverPt = null;
function setMeasure(on) {
  measuring = on;
  points = [];
  hoverPt = null;
  measureBtn.setAttribute('aria-pressed', String(on));
  root.classList.toggle('measuring', on);
  if (viewer) viewer.dirty = true;
}
// close to a corner, it takes the corner: a measurement should land on the thing, not near it
function pointAt(x, y) {
  const h = viewer.hit(x, y);
  if (!h) return null;
  if (h.face) {
    const g = h.object.geometry,
      pos = g.attributes.position,
      r = viewer.host.getBoundingClientRect(),
      v = new THREE.Vector3();
    let best = null,
      gap = 9;
    for (const i of [h.face.a, h.face.b, h.face.c]) {
      v.fromBufferAttribute(pos, i);
      if (h.object.isSkinnedMesh) continue;
      v.applyMatrix4(h.object.matrixWorld);
      const q = viewer.project(v),
        d = Math.hypot(q[0] + r.left - x, q[1] + r.top - y);
      if (d < gap) {
        gap = d;
        best = v.clone();
      }
    }
    if (best) return best;
  }
  return h.point.clone();
}

// ---------- pointer: a click is a press that didn't travel ----------
let press = null,
  hoverRaf = 0;
function listen(canvas) {
  canvas.addEventListener('pointerdown', (e) => {
    press = { x: e.clientX, y: e.clientY, button: e.button };
    shell.closePop();
  });
  canvas.addEventListener('pointerup', (e) => {
    if (!press || press.button || Math.hypot(e.clientX - press.x, e.clientY - press.y) > 4) {
      press = null;
      return;
    }
    press = null;
    if (measuring) {
      const p = pointAt(e.clientX, e.clientY);
      if (!p) return;
      sound();
      const last = points[points.length - 1];
      if (last && !last.b) {
        last.b = p;
      } else {
        points = [{ a: p }];
      }
      viewer.dirty = true;
      return;
    }
    if (partsSide.classList.contains('on')) {
      const h = viewer.hit(e.clientX, e.clientY);
      selectPart(h ? partOf(h.object) : null);
    }
  });
  canvas.addEventListener('pointermove', (e) => {
    if (!measuring || press) return;
    cancelAnimationFrame(hoverRaf);
    hoverRaf = requestAnimationFrame(() => {
      hoverPt = pointAt(e.clientX, e.clientY);
      viewer.dirty = true;
    });
  });
  canvas.addEventListener('pointerleave', () => {
    if (hoverPt) {
      hoverPt = null;
      viewer.dirty = true;
    }
  });
  canvas.addEventListener('dblclick', (e) => {
    if (measuring) return;
    const h = viewer.hit(e.clientX, e.clientY);
    if (h) {
      const b = new THREE.Box3().setFromObject(partOf(h.object));
      viewer.setView(viewer.view || 'iso', false, b);
      viewPick.set(null);
    } else {
      sound();
      viewer.setView(viewPick.value || 'iso');
      viewPick.set(viewer.view);
    }
  });
}

// ---------- cutting through ----------
let sectioning = false,
  secSide = 1,
  plane = null,
  AX = null;
function applySection() {
  if (!sectioning) return;
  const k = axisPick.value,
    b = viewer.box,
    lo = b.min[k],
    hi = b.max[k],
    t = +secSlider.value / 100,
    at = lo + (hi - lo) * t,
    v = AX[k];
  plane.normal.copy(v).multiplyScalar(-secSide);
  plane.constant = secSide * at;
  viewer.setSection(plane);
  secSlider.style.setProperty('--p', secSlider.value + '%');
  secVal.textContent = len(at - lo);
}
function setSectioning(on) {
  sectioning = on;
  sectionBtn.setAttribute('aria-pressed', String(on));
  if (on) {
    axisPick.set(axisPick.value || 'y');
    secRow.hidden = false;
    axisPick.place();
    secRow.hidden = true;
    riseRow(secRow, true);
    applySection();
    requestAnimationFrame(() => axisPick.place());
  } else {
    riseRow(secRow, false);
    viewer.setSection(null);
  }
}

// ---------- motion ----------
function fmt(t) {
  const s = Math.max(0, Math.floor(t));
  return `${Math.floor(s / 60)}:${String(s % 60).padStart(2, '0')}`;
}
function syncTime() {
  const d = viewer.clip ? viewer.clip.duration : 0,
    t = d ? viewer.time() % d : 0,
    p = d ? (t / d) * 100 : 0;
  timeEl.textContent = `${fmt(t)} / ${fmt(d)}`;
  trackFill.style.width = p + '%';
  trackHead.style.left = p + '%';
}
function syncPlayBtn() {
  playBtn.innerHTML = icon(viewer.playing ? 'pause' : 'play');
  const tip = T(viewer.playing ? '暂停动画 · 空格' : '播放动画 · 空格');
  playBtn.dataset.tip = tip;
  playBtn.setAttribute('aria-label', tip);
}
function togglePlay() {
  if (!viewer.mixer) return;
  viewer.playing = !viewer.playing;
  viewer.clock.getDelta();
  syncPlayBtn();
}
let seeking = false;
const seekTo = (e) => {
  const r = track.getBoundingClientRect(),
    d = viewer.clip.duration;
  viewer.seek(clamp((e.clientX - r.left) / r.width, 0, 1) * d * 0.9999);
  syncTime();
};
track.addEventListener('pointerdown', (e) => {
  if (!viewer.clip) return;
  seeking = true;
  track.setPointerCapture(e.pointerId);
  if (viewer.playing) togglePlay();
  seekTo(e);
});
track.addEventListener('pointermove', (e) => {
  if (seeking) seekTo(e);
});
track.addEventListener('pointerup', () => {
  seeking = false;
});
// the animation's name, in a menu that rises from the dock
clipBtn.addEventListener('click', () => {
  sound();
  shell.openPop(
    clipBtn,
    viewer.clips.map((c, i) => ({ value: i, label: c.name || String(i + 1), raw: true, on: c === viewer.clip })),
    (i) => {
      viewer.pickClip(i);
      clipBtn.textContent = viewer.clip.name || String(i + 1);
      syncTime();
    },
    { up: true }
  );
});

// ---------- parts: the panel. Hiding only changes what you see ----------
let selected = null,
  hoverBox = null,
  selBox = null;
const named = (o) => o.name || (o.isMesh ? o.geometry.name || T('网格（Mesh）') : T('群组（Group）'));
const hasMesh = (o) => {
  let y = false;
  o.traverse((q) => {
    if (q.isMesh) y = true;
  });
  return y;
};
function partOf(mesh) {
  let q = mesh;
  while (q.parent && q.parent !== model.root && q.parent.parent !== model.root) q = q.parent;
  return q === model.root ? mesh : q;
}
function treeRows() {
  const out = [];
  const walk = (o, depth) => {
    for (const c of o.children) {
      if (!hasMesh(c)) continue;
      const only = c.children.length === 1 && !c.isMesh && c.children[0].isMesh && !c.name;
      if (only) {
        out.push({ o: c.children[0], depth });
        continue;
      }
      out.push({ o: c, depth });
      if (!c.isMesh && depth < 3) walk(c, depth + 1);
    }
  };
  walk(model.root, 0);
  return out;
}
function frameHelper(h, o) {
  if (!h) return;
  if (!o) {
    h.visible = false;
    viewer.dirty = true;
    return;
  }
  h.box.setFromObject(o);
  h.visible = true;
  h.updateMatrixWorld(true);
  viewer.dirty = true;
}
function selectPart(o) {
  selected = o;
  frameHelper(selBox, o);
  if (selBox) selBox.material.color.copy(viewer.wire.color);
  if (partsSide.classList.contains('on')) syncParts();
  const row = o && partsSide.querySelector(`[data-uuid="${o.uuid}"]`);
  if (row) row.scrollIntoView({ block: 'nearest', behavior: reduce ? 'auto' : 'smooth' });
}
function toggleHide(o) {
  viewer.crossfade(() => {
    o.visible = !o.visible;
    refitShadow();
  });
  syncParts();
}
const isAncestor = (a, b) => {
  for (let q = b.parent; q; q = q.parent) if (q === a) return true;
  return false;
};
function isolate(o) {
  const rows = treeRows();
  viewer.crossfade(() => {
    const alone = rows.every((r) => r.o === o || isAncestor(r.o, o) || isAncestor(o, r.o) || !r.o.visible) && o.visible;
    rows.forEach((r) => {
      r.o.visible = alone || r.o === o || isAncestor(r.o, o) || isAncestor(o, r.o);
    });
    refitShadow();
  });
  syncParts();
}
function showAll() {
  viewer.crossfade(() => {
    model.root.traverse((o) => {
      o.visible = true;
    });
    refitShadow();
  });
  syncParts();
}
function refitShadow() {
  viewer.place();
  if (sectioning) applySection();
}
const cssAccent = () => getComputedStyle(root).getPropertyValue('--accent').trim() || '#FF6A00';
let partsAllBtn = null,
  partsFold = null,
  partsPill = null;
function buildParts(body) {
  const list = el('div', 'part-list');
  list.id = 'partList';
  partsPill = el('i', 'list-pl');
  list.appendChild(partsPill);
  const all = el('button', 'btn soft side-wide', T('显示全部部件'));
  all.type = 'button';
  all.id = 'partsAll';
  all.addEventListener('mousedown', (e) => e.preventDefault());
  all.addEventListener('click', () => {
    sound();
    showAll();
  });
  partsAllBtn = all;
  const f = el('div', 'm-fold'),
    fi = el('div', 'm-in');
  fi.appendChild(all);
  f.appendChild(fi);
  f.hidden = true;
  partsFold = f;
  for (const r of treeRows()) {
    const row = el('button', 'part-row');
    row.type = 'button';
    row.dataset.uuid = r.o.uuid;
    row.style.paddingLeft = 8 + r.depth * 14 + 'px';
    row.append(el('span', 'part-n', named(r.o)));
    const act = (ic, tip, run, cls) => {
      const b = el('i', 'part-a ' + (cls || ''));
      b.innerHTML = icon(ic);
      b.dataset.tip = T(tip);
      b.addEventListener('click', (e) => {
        e.stopPropagation();
        sound();
        run();
      });
      return b;
    };
    row.append(
      act('focus', '独显当前部件', () => isolate(r.o)),
      act('eye', '隐藏', () => toggleHide(r.o), 'part-eye')
    );
    row.addEventListener('mousedown', (e) => e.preventDefault());
    row.addEventListener('click', () => {
      sound();
      selectPart(selected === r.o ? null : r.o);
    });
    row.addEventListener('dblclick', () => {
      viewer.setView(viewer.view || 'iso', false, new THREE.Box3().setFromObject(r.o));
      viewPick.set(null);
    });
    row.addEventListener('pointerenter', () => {
      hoverBox.material.color.set(cssAccent());
      frameHelper(hoverBox, r.o);
    });
    row.addEventListener('pointerleave', () => frameHelper(hoverBox, null));
    row._o = r.o;
    list.appendChild(row);
  }
  body.append(list, f);
}
// the selected row of a list: the dark block slides between the rows
const placeListPill = (pl, row) => {
  if (!row) {
    pl.style.opacity = 0;
    return;
  }
  if (!row.offsetParent) return;
  const fresh = pl.style.opacity !== '1';
  if (fresh) pl.style.transition = 'opacity var(--t-1) var(--e-out)';
  pl.style.opacity = '1';
  pl.style.transform = `translateY(${row.offsetTop}px)`;
  pl.style.height = row.offsetHeight + 'px';
  if (fresh) {
    void pl.offsetWidth;
    pl.style.transition = '';
  }
};
function syncParts() {
  let hidden = false;
  partsSide.querySelectorAll('.part-row').forEach((r) => {
    const v = r._o.visible,
      eye = r.querySelector('.part-eye');
    r.classList.toggle('off', !v);
    if (!v) hidden = true;
    r.setAttribute('aria-pressed', String(r._o === selected));
    const want = v ? 'eye' : 'hidden';
    if (eye.dataset.ic !== want) {
      eye.innerHTML = icon(want);
      eye.dataset.ic = want;
      eye.dataset.tip = T(v ? '隐藏' : '显示');
    }
  });
  if (partsPill && partsPill.isConnected)
    placeListPill(partsPill, partsSide.querySelector('.part-row[aria-pressed="true"]'));
  if (partsFold && partsFold.hidden === hidden) {
    partsFold.getAnimations().forEach((a) => a.cancel());
    if (reduce) partsFold.hidden = !hidden;
    else if (hidden) {
      partsFold.hidden = false;
      const h = partsFold.offsetHeight;
      partsFold.style.overflow = 'hidden';
      const a = partsFold.animate(
        [
          { height: 0, opacity: 0 },
          { height: h + 'px', opacity: 1 },
        ],
        MOVE
      );
      a.onfinish = a.oncancel = () => {
        partsFold.style.overflow = '';
      };
    } else {
      const h = partsFold.offsetHeight;
      partsFold.style.overflow = 'hidden';
      const a = partsFold.animate(
        [
          { height: h + 'px', opacity: 1 },
          { height: 0, opacity: 0 },
        ],
        MOVE
      );
      a.onfinish = () => {
        partsFold.hidden = true;
        partsFold.style.overflow = '';
      };
      a.oncancel = () => {
        partsFold.style.overflow = '';
      };
    }
  }
}

// ---------- the facts ----------
function facts() {
  const st = M.statsOf(model.root),
    s = worldSize();
  // closed and volume are a solid's: a path or a cloud has neither
  if (!model.solid && !model.solidless && st.tris <= 4000000) model.solid = M.solidOf(model.root);
  // in the file's own axes: a Z-up file stands up on the stage by turning, which swaps what's height and depth
  return { st, size: model.zUp ? [s.x, s.z, s.y] : [s.x, s.y, s.z], solid: model.solid };
}
function panelInfo(body) {
  const { st, solid } = facts(),
    dl = el('dl', 'info-list'),
    row = (k, v) => {
      if (v == null || v === '') return;
      const d = el('div');
      d.append(el('dt', 'dyn', T(k)), el('dd', 'dyn', v));
      dl.appendChild(d);
    },
    s = worldSize();
  row('格式', model.format);
  row('边界尺寸', `${len(s.x)} × ${len(s.z)} × ${len(s.y)}`);
  row('三角面数', grouped(st.tris));
  row('顶点数', grouped(st.verts));
  if (st.meshes > 1) row('模型结构', String(st.meshes));
  if (st.materials > 1 || model.kind === 'glb') row('材质球数', String(st.materials));
  if (st.textures) row('纹理贴图', String(st.textures));
  if (viewer.clips.length) row('动画序列', String(viewer.clips.length));
  row('文件大小', money(fileSize));
  // closed and volume are what a printer asks about, so they're said for STL, OBJ and 3MF, not for scenes
  if (solid && model.kind !== 'glb') {
    const ok = solid.open === 0 && solid.nonManifold === 0,
      bad = grouped(solid.open + solid.nonManifold);
    const broken = T('{n} 条破边', { n: bad });
    row('流形封闭（可打印性）', ok ? T('封闭流形') : broken);
    if (ok) row('几何体积', vol(solid.volume));
  }
  if (model.missing && model.missing.length) row('缺失的外部贴图', model.missing.join(T('、')));
  body.appendChild(dl);
  if (!model.fixedUnit) {
    const unitG = pickGroup(
        [
          ['mm', 'mm'],
          ['cm', 'cm'],
          ['m', 'm'],
          ['in', T('英寸')],
        ],
        'unit',
        (k) => {
          model.unit = k;
          made = null;
          viewer.dirty = true;
          if (sectioning) applySection();
          fillInfo();
        }
      ),
      box = el('div');
    box.append(el('span', 'side-label dyn', T('尺度单位')), unitG.box);
    body.appendChild(box);
    unitG.set(model.unit);
    requestAnimationFrame(() => unitG.place());
  }
  const upG = pickGroup(
      [
        ['y', 'Y'],
        ['z', 'Z'],
      ],
      'up',
      (k) => {
        const z = k === 'z';
        if (z === model.zUp) return;
        model.zUp = z;
        made = null;
        viewer.setUp(z);
        if (sectioning) applySection();
        upG.set(k);
        viewPick.set(viewer.view);
        fillInfo();
      }
    ),
    box2 = el('div');
  box2.append(el('span', 'side-label dyn', T('向上轴向')), upG.box);
  body.appendChild(box2);
  upG.set(model.zUp ? 'z' : 'y');
  requestAnimationFrame(() => upG.place());
}
function fill(side, kind) {
  side.textContent = '';
  const body = el('div', 'side-body');
  side.append(el('h3', 'dyn', T(kind === 'parts' ? '模型结构' : '信息')), body);
  if (kind === 'parts') {
    buildParts(body);
    syncParts();
  } else panelInfo(body);
}
function fillInfo() {
  if (!infoSide.classList.contains('on')) return;
  const y = infoSide.scrollTop;
  fill(infoSide, 'info');
  infoSide.scrollTop = y;
}
// the shell opens and closes the panels; the stage makes room, and a panel is drawn as it opens
for (const [side, kind] of [
  [partsSide, 'parts'],
  [infoSide, 'info'],
]) {
  let was = false;
  new MutationObserver(() => {
    const on = side.classList.contains('on');
    if (on === was) return;
    was = on;
    if (on && opened) fill(side, kind);
    if (!on && kind === 'parts') {
      selectPart(null);
      frameHelper(hoverBox, null);
    }
    shell.closePop();
    syncPads();
  }).observe(side, { attributes: true, attributeFilter: ['class'] });
}

// ---------- opening ----------
// read, then shown: the shell asks the program for the file, and the page reads it. Only a model that read whole
// becomes the open file
async function openModel(path) {
  const seq = ++openSeq,
    name = baseName(path);
  let buf;
  try {
    buf = await own.readModel(path);
  } catch (e) {
    return { status: ['missing', 'refused'].includes(String(e)) ? String(e) : 'damaged' };
  }
  const bytes = new Uint8Array(buf);
  let m;
  try {
    await stageReady();
    m = await M.readModel(bytes, name, own.haveBeside);
  } catch (e) {
    if (!M) {
      console.error(e);
      return { status: 'damaged' };
    }
    if (e instanceof M.Compressed) return { status: 'compressed' };
    if (e instanceof M.OldFbx) return { status: 'oldfbx' };
    if (e instanceof M.Missing) {
      const needs = T('缺少关联文件「{name}」，请将其放在模型同级目录下。', { name: e.names[0] });
      shell.setState('');
      sound();
      shell.say(needs);
      return { status: 'abandoned' };
    }
    return { status: 'damaged' };
  }
  if (seq !== openSeq) return { status: 'abandoned' };
  const r = await own.adoptModel(path);
  return { ...r, kind: m.kind, _m: m, _bytes: bytes };
}
function hold(m, name, size, bytes) {
  selectPart(null);
  setMeasure(false);
  shell.closePop();
  if (sectioning) {
    secRow.hidden = true;
    sectionBtn.setAttribute('aria-pressed', 'false');
    sectioning = false;
  }
  model = m;
  fileSize = size;
  src = { kind: m.kind === 'glb' && !/\.glb$/i.test(name) ? '' : m.kind, bytes, key: '' };
  made = null;
  opened = true;
  edits = savedAt = 0;
  viewer.setSection(null);
  viewer.setModel(m.root, m.clips, m.zUp);
  viewer.mode = 'original';
  viewer.ground.visible = true;
  partsBtn.hidden = M.statsOf(m.root).meshes < 2;
  const hasAnim = viewer.clips.length > 0;
  animRow.hidden = !hasAnim;
  if (hasAnim) {
    clipBtn.textContent = viewer.clip.name || '1';
    clipBtn.hidden = viewer.clips.length < 2;
    syncPlayBtn();
    syncTime();
  }
  syncPads();
  viewPick.set('iso');
  modePick.set('original');
  refitMode();
  if (!reduce) viewer.renderer.domElement.animate([{ opacity: 0 }, { opacity: 1 }], GROW);
  if (m.missing && m.missing.length) shell.say(T('缺少 {name}，暂以灰色显示。', { name: m.missing[0] }));
}
function goEmpty() {
  setMeasure(false);
  shell.closePop();
  opened = false;
  if (viewer) viewer.clear();
  model = null;
  src = null;
  made = null;
  animRow.hidden = secRow.hidden = true;
  sectioning = false;
  sectionBtn.setAttribute('aria-pressed', 'false');
  syncPads();
}

function keys(e) {
  const mod = e.ctrlKey || e.metaKey || e.altKey,
    k = e.key,
    a = document.activeElement,
    typing = /INPUT|TEXTAREA/.test(a.tagName) && a.type !== 'range';
  if (!opened || typing || mod) return;
  if (k === 'Escape') {
    if (measuring) {
      sound();
      setMeasure(false);
      return;
    }
    if (sectioning) {
      sound();
      setSectioning(false);
    }
    return;
  }
  if (document.querySelector('.dlg.on, .set.on, .ask-wrap.on')) return;
  if (k === 'm' || k === 'M') {
    sound();
    setMeasure(!measuring);
    return;
  }
  if (k === 'c' || k === 'C') {
    sound();
    setSectioning(!sectioning);
    return;
  }
  if (k === ' ' && viewer.mixer) {
    e.preventDefault();
    sound();
    togglePlay();
  }
}

// ---------- saving: the family's card; what goes to disk is made here ----------
let sharpKey = '1080',
  transparentOn = true,
  skipHidden = true,
  made = null,
  written = null;
const anyHidden = () => {
  let h = false;
  model.root.traverse((o) => {
    if (!o.visible) h = true;
  });
  return h;
};
// which parts a save leaves out, as a key: the same key and the same format make the same bytes
const keyNow = () =>
  skipHidden && anyHidden()
    ? (() => {
        const ids = [];
        model.root.traverse((o) => {
          if (!o.visible) ids.push(o.uuid);
        });
        return ids.join();
      })()
    : '';
const sameFile = (fmt) => !!src && fmt === src.kind && keyNow() === src.key;
const madeKey = (fmt) => [fmt, keyNow(), model.unit, model.zUp].join('|');
async function bytesFor(fmt) {
  if (sameFile(fmt)) return src.bytes;
  if (fmt === 'png') {
    const { blob } = await viewer.snapshot(+sharpKey, transparentOn);
    return new Uint8Array(await blob.arrayBuffer());
  }
  const key = madeKey(fmt);
  if (made && made.key === key) return made.bytes;
  const shown = [];
  if (!skipHidden)
    model.root.traverse((o) => {
      if (!o.visible) {
        shown.push(o);
        o.visible = true;
      }
    });
  try {
    const bytes =
      fmt === 'glb'
        ? await M.writeGlb(model.root, model, viewer.clips)
        : fmt === 'obj'
          ? await M.writeObj(model.root, model)
          : await M.writeStl(model.root, model);
    made = { key, bytes, fmt };
    return bytes;
  } finally {
    shown.forEach((o) => {
      o.visible = false;
    });
    viewer.place();
    viewer.dirty = true;
  }
}
// the new file's size: exact for the model formats, from the bytes themselves; a fair guess for a picture
async function estimate(fmt) {
  if (!model) return null;
  if (fmt === 'png') {
    const w = (+sharpKey * stage.clientWidth) / Math.max(1, stage.clientHeight);
    return Math.round(w * +sharpKey * (transparentOn ? 0.18 : 0.3));
  }
  if (fmt === 'stl') {
    let tris = 0;
    model.root.traverse((o) => {
      if (o.isMesh && (!skipHidden || viewer.shown(o)))
        tris += (o.geometry.index ? o.geometry.index.count : o.geometry.attributes.position.count) / 3;
    });
    return 84 + 50 * Math.round(tris);
  }
  return (await bytesFor(fmt)).length;
}
function saveRows(box, fmt, again) {
  if (fmt === 'png') {
    const wrap = el('div'),
      c = chips({
        id: 'sharpChips',
        options: [
          ['1080', '1080'],
          ['2160', '2160'],
          ['4320', '4320'],
        ],
        get: () => sharpKey,
        pick: (v) => {
          sharpKey = v;
          again();
        },
      });
    c.querySelectorAll('.chip').forEach((b) => {
      b.id = 'sharp-' + b.dataset.v;
    });
    wrap.append(el('span', 'side-label dyn', T('采样清晰度')), c);
    box.append(
      wrap,
      toggleRow({
        id: 'pngClear',
        label: '透明背景',
        get: () => transparentOn,
        toggle: () => {
          transparentOn = !transparentOn;
          again();
        },
      })
    );
  } else if (model && anyHidden())
    box.appendChild(
      toggleRow({
        id: 'skipHidden',
        label: '忽略已隐藏的部件',
        get: () => skipHidden,
        toggle: () => {
          skipHidden = !skipHidden;
          again();
        },
      })
    );
}
async function saveTo(path, fmt) {
  const bytes = await bytesFor(fmt);
  written = { bytes, key: keyNow() };
  // a picture isn't the model: the Rust side leaves it off the recent list, and the family keeps the window on the model
  return own.saveModel(path, bytes, fmt === 'png');
}

// ---------- language ----------
shell.onLang(() => {
  refitMode();
  if (opened) {
    [viewPick, modePick, axisPick].forEach((g) => g.place());
    syncPlayBtn();
    if (partsSide.classList.contains('on')) fill(partsSide, 'parts');
    fillInfo();
    if (sectioning) applySection();
    viewer.dirty = true;
  }
});
if (document.fonts && document.fonts.ready)
  document.fonts.ready.then(() => {
    [viewPick, modePick, axisPick].forEach((g) => g.place());
  });

// with DOTANY_TEST set, shell.test exposes the viewer and its actions, so that an automated test can read and drive it
// (defineProperties, not assign: the getters stay live)
Object.defineProperties(
  shell.test,
  Object.getOwnPropertyDescriptors({
    touch() {
      edits++;
      shell.changed();
    },
    get viewer() {
      return viewer;
    },
    get THREE() {
      return THREE;
    },
    stageReady,
    get model() {
      return model;
    },
    get points() {
      return points;
    },
    get made() {
      return made;
    },
    facts: () => {
      const f = facts();
      return {
        tris: f.st.tris,
        verts: f.st.verts,
        meshes: f.st.meshes,
        size: f.size,
        unit: model.unit,
        zUp: model.zUp,
        open: f.solid && f.solid.open,
        nonManifold: f.solid && f.solid.nonManifold,
        volume: f.solid && f.solid.volume,
        format: model.format,
        kind: model.kind,
        clips: viewer.clips.map((c) => [c.name, c.duration]),
        missing: model.missing || [],
      };
    },
    treeRows: () => treeRows().map((r) => ({ name: named(r.o), uuid: r.o.uuid, visible: r.o.visible })),
    setMeasure,
    setSectioning,
    togglePlay,
    selectPart,
    showAll,
    project: (x, y, z) => viewer.project(new THREE.Vector3(x, y, z)),
  })
);
shell.begin();
// once the window is up and quiet, three.js and the readers load in the background and the stage's drawing context and
// light are made ready (viewer.js prepare); a file the program was started with has already asked for them
const warm = () =>
  loadEngine()
    .then((v) => {
      if (!viewer && !prepared) prepared = v.prepare();
    })
    .catch(() => {});
// WebKit (the Mac's WKWebView) has no requestIdleCallback: there the warming waits a moment instead
const idle = window.requestIdleCallback || ((fn) => setTimeout(fn, 200));
new MutationObserver((_, mo) => {
  if (root.dataset.ready !== '1') return;
  mo.disconnect();
  idle(() => warm(), { timeout: 2000 });
}).observe(root, { attributes: true, attributeFilter: ['data-ready'] });
