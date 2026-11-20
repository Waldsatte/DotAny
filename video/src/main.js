// DotAny Video's page. The window around the film is the family's (shared/web/shell.js):
// the top bar, the file menu, settings, the save card, the close prompt, the notice row, the empty and drop views.
// What's Video's own is here: the film, the dock with its timeline and trim, subtitles, the info panel, and its save.
import { createShell } from '@dotany/web/shell.js';
import { app as shared, el, root, reduce, sound, store, EASE } from '@dotany/web/shared.js';
import { inkfall } from '@dotany/web/inkfall-input.js';
import { platform } from '@dotany/web/keys.js';
import { baseName } from '@dotany/web/paths.js';
import { os } from '@dotany/web/keys.js';
import { T, WORDS } from './i18n.js';
import { LUCIDE } from './icons.js';
import { RealFilm } from './player.js';
import { SystemFilm } from './system-film.js';
import { readSubtitles, subtitleText } from './subtitles.js';
import * as be from './backend.js';
import { mac, wheelSource } from '@dotany/web/gestures.js';
import { used } from '@dotany/web/keys.js';

const boot = window.__DOTANY__ || {};
const icon = (n) => `<svg class="i" viewBox="0 0 24 24" aria-hidden="true">${LUCIDE[n]}</svg>`;
const clamp = (v, a, b) => Math.min(b, Math.max(a, v));
const clock = (t) => {
  t = Math.max(0, t || 0);
  const h = Math.floor(t / 3600),
    m = Math.floor(t / 60) % 60,
    s = Math.floor(t % 60),
    ss = String(s).padStart(2, '0');
  return h ? `${h}:${String(m).padStart(2, '0')}:${ss}` : `${m}:${ss}`;
};
const money = (b) =>
  b >= 1073741824
    ? (b / 1073741824).toFixed(1) + ' GB'
    : b >= 1048576
      ? (b / 1048576).toFixed(1) + ' MB'
      : Math.max(1, Math.round(b / 1024)) + ' KB';
let fullOn = false;
let film = null;
// a button's sound never lands on top of the film (the family's sound() asks app.quiet)
shared.quiet = () => !!film && film.playing;

// whatever changes size eases to it, and what's new fades in; nothing jumps
const MOVE = { duration: 240, easing: EASE };

// ---------- what's open ----------
let file = null,
  fileName = '',
  fileSize = 0,
  cover = '',
  title = '',
  trimA = 0,
  trimB = 0,
  keyA = 0,
  keyB = null,
  savedSig = '',
  metaDirty = false,
  titleEdited = false,
  coverEdited = false;
let subs = [],
  subsName = '',
  subsOn = true,
  subsSize = 'm',
  level = +(store.get('dotany-video-volume') ?? 1),
  muted = false,
  rate = 1;
const keys = () => (file ? file.facts.keys : []);
const total = () => (film ? film.duration || 0 : 0);
const trimmed = () => !!film && (trimA > 0.01 || trimB < total() - 0.01);
const sig = () => `${trimA.toFixed(1)}|${trimB.toFixed(1)}|${metaDirty}`;
const dirty = () => !!film && sig() !== savedSig;
const say = (text) => shell.say(T(text));

// ---------- the window: the family's, with Video's own words, buttons and save ----------
// a film keeps its own format, because nothing is re-encoded: its one format is the one it came in, extension as written
const FORMAT = { film: { ext: '.mp4', label: '视频' } };
let sureSize = null,
  reEstimate = null;
const shell = createShell({
  name: 'DotAny Video',
  icons: LUCIDE,
  words: WORDS,
  notices: () => import('./notices.generated.js'),
  dropTitle: '拖拽视频至此',
  fileKind: '视频文件',
  soundNote: '触发按钮操作时播放提示音，视频播放期间自动静默。',
  // a quiet readout before Settings: the picture's size and the file's (shell.setCount)
  count: true,
  money,
  formats: FORMAT,
  formatsFor: () => ['film'],
  formatOf: () => 'film',
  // a long film takes a while: the card stays, Save dimmed, until it's written
  dimWhileSaving: true,
  cantOpen: {
    cantplay: '无法播放此视频，系统可能缺少相应的视频解码器。',
    // the Mac can't play WMV at all; opening one answers { status: 'nowmv' }
    nowmv: os({ windows: '无法播放此视频，系统可能缺少相应的视频解码器。', mac: 'Mac 不支持播放 WMV 格式。' }),
  },
  open: (path) => prepare(path),
  show: (r) => hold(r),
  clear: () => closeFilm(),
  dirty: () => dirty(),
  discard() {
    savedSig = sig();
  },
  // the new file is worked out before it's written, so its size is exact
  beforeSaveAs() {
    sureSize = null;
    be.estimateSave(saveRequest())
      .then((n) => {
        if (n != null && shell.cardOpen) {
          sureSize = n;
          if (reEstimate) reEstimate();
        }
      })
      .catch(() => {});
  },
  saveRows: (box, fmt, est) => {
    reEstimate = est;
  },
  estimate: () => (film ? (sureSize ?? (fileSize * (trimB - trimA)) / (total() || 1)) : null),
  save: (path) => write(path),
  keys: (e) => keyDown(e),
  drop: (paths) => dropped(paths),
  fileItems: () => [{ value: 'subs', label: '载入字幕…', run: () => openSubs() }],
});
const { chrome } = shell;
const stage = shell.stage,
  subline = el('div', 'subline m');
subline.hidden = true;
shell.place(subline);
const cutBtn = shell.headButton('cutBtn', 'cut', '截取片段');
const subsPanel = shell.sidePanel('subsPanel', 'subs', '字幕'),
  infoPanel = shell.sidePanel('infoPanel', 'info', '信息');
const subsBtn = document.getElementById('subsPanelBtn'),
  infoBtn = document.getElementById('infoPanelBtn');
subsBtn.id = 'subsBtn';
infoBtn.id = 'infoBtn';
cutBtn.setAttribute('aria-pressed', 'false');

// the dock: the buttons along the bottom, with the film's timeline in the middle
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
const navPrev = barBtn('prevFilm', 'prev', '上一个', () => {
    sound();
    shell.step(-1);
  }),
  navNext = barBtn('nextFilm', 'next', '下一个', () => {
    sound();
    shell.step(1);
  });
navPrev.classList.add('pic-nav');
navNext.classList.add('pic-nav');
const playBtn = barBtn('play', 'play', '播放', () => togglePlay());
const clockEl = el('span', 'clock');
const bar = el('div', 'bar');
bar.id = 'bar';
bar.setAttribute('role', 'slider');
bar.setAttribute('aria-label', T('播放'));
bar.tabIndex = 0;
const barKeep = el('i', 'bar-keep'),
  barFill = el('i', 'bar-fill'),
  barNow = el('i', 'bar-now'),
  barTip = el('span', 'bar-tip');
bar.append(barKeep, barFill, barNow, barTip);
const trim = el('div', 'trim'),
  trimCv = el('canvas', 'trim-film'),
  dimL = el('i', 'trim-dim'),
  dimR = el('i', 'trim-dim'),
  trimFrame = el('i', 'trim-frame'),
  trimNow = el('i', 'trim-now'),
  tagA = el('span', 'trim-tag a'),
  tagB = el('span', 'trim-tag b');
trim.id = 'trim';
trimCv.id = 'trimFilm';
const gripA = el('button', 'trim-grip'),
  gripB = el('button', 'trim-grip');
gripA.type = gripB.type = 'button';
gripA.id = 'gripA';
gripB.id = 'gripB';
gripA.setAttribute('aria-label', T('起始点'));
gripB.setAttribute('aria-label', T('结束点'));
const trimTrack = el('i', 'trim-track'),
  trimFill = el('i', 'trim-fill');
trim.append(trimTrack, trimFill, trimCv, dimL, dimR, trimFrame, trimNow, gripA, gripB, tagA, tagB);
const cutInfo = el('span', 'cut-info cut-only'),
  cutKept = el('span'),
  cutNote = el('span', 'cut-note', ' · 已对齐关键帧（无损极速导出）');
cutInfo.append(cutKept, cutNote);
const softBtn = (id, text, cls) => {
  const b = el('button', 'btn cut-only ' + cls, text);
  b.type = 'button';
  b.id = id;
  b.addEventListener('mousedown', (e) => e.preventDefault());
  return b;
};
const cutReset = softBtn('trimReset', '还原', 'soft'),
  cutDone = softBtn('trimDone', '完成', 'primary');
const volBtn = barBtn('volBtn', 'volume', '静音', () => {
  sound();
  setMuted(!muted);
});
const volBox = el('label', 'vol'),
  volIn = document.createElement('input');
volIn.type = 'range';
volIn.id = 'vol';
volIn.min = 0;
volIn.max = 1;
volIn.step = 0.01;
volIn.setAttribute('aria-label', T('音量'));
volBox.appendChild(volIn);
const rateBtn = el('button', 'tb rate', '倍速');
rateBtn.type = 'button';
rateBtn.id = 'rate';
rateBtn.setAttribute('aria-label', T('播放速度'));
rateBtn.addEventListener('mousedown', (e) => e.preventDefault());
const fullBtn = barBtn('fullBtn', 'full', '全屏', () => {
  sound();
  toggleFull();
});
const pill = el('div', 'pill'),
  pillMain = el('div', 'pill-main');
pillMain.append(playBtn, clockEl, bar, el('i', 'sep'), volBtn, volBox, rateBtn, fullBtn, cutInfo, cutReset, cutDone);
pill.append(trim, pillMain);
const dockRow = el('div', 'dock-row');
dockRow.append(navPrev, pill, navNext);
shell.place(dockRow);

// ---------- playing ----------
let raf = 0;
function loop() {
  raf = 0;
  syncTime();
  if (film && film.playing) raf = requestAnimationFrame(loop);
}
const kick = () => {
  if (!raf) raf = requestAnimationFrame(loop);
};
function togglePlay() {
  if (!film) return;
  if (film.playing) {
    film.pause();
  } else {
    if (film.time >= trimB - 0.05 || film.time < trimA) film.time = trimA;
    sound();
    const p = film.play();
    if (p && p.catch) p.catch(() => {});
  }
  syncPlay();
  kick();
  wake();
}
// a film that is opened plays at once, with its sound, and the button says pause straight away. The program's own tests
// can ask for a still film (DOTANY_VIDEO_TEST_STILL), so a picture stays put.
const still = !!(boot.test && boot.still);
function autoPlay() {
  if (!film || film.playing || still) return;
  const p = film.play();
  syncPlay();
  kick();
  wake();
  if (p && p.catch)
    p.catch(() => {
      syncPlay();
      wake();
    });
}
function syncPlay() {
  const on = !!film && film.playing,
    label = T(on ? '暂停' : '播放');
  playBtn.innerHTML = icon(on ? 'pause' : 'play');
  playBtn.setAttribute('aria-label', label);
  playBtn.dataset.tip = label;
}
function syncTime() {
  if (!film) return;
  let t = film.time;
  // playback stops where the trim ends, so what you hear is what you'll keep
  if (film.playing && t >= trimB) {
    film.pause();
    film.time = trimB;
    t = trimB;
    syncPlay();
    wake();
  }
  clockEl.innerHTML = `${clock(t)} <i>/ ${clock(total())}</i>`;
  const share = total() ? clamp(t / total(), 0, 1) * 100 : 0;
  barFill.style.width = share + '%';
  barNow.style.left = share + '%';
  trimNow.style.left = share + '%';
  bar.setAttribute('aria-valuenow', Math.round(share));
  syncSub(t);
  checkSound();
}
function syncSub(t) {
  const on = subsOn && subs.find((c) => t >= c.a && t <= c.b);
  subline.hidden = !on;
  if (on && subline.textContent !== on.words) subline.textContent = on.words;
}
function seek(t) {
  if (!film) return;
  film.time = clamp(t, 0, total());
  // the Mac: a paused seek that shows no picture is seeked again (player.js, settle)
  if (platform === 'mac') film.settle();
  syncTime();
}

// the top and the dock step aside while the film plays
let restTimer = 0,
  overDock = false;
const anyOpen = () =>
  cutting
  || rateSheet.classList.contains('on')
  || !!document.querySelector('.side.on, .set.on, .pop.on, .dlg.on, .ask-wrap.on, .notice.on');
function wake() {
  root.classList.remove('resting');
  clearTimeout(restTimer);
  if (film && film.playing && !reduce)
    restTimer = setTimeout(() => {
      if (film && film.playing && !anyOpen() && !overDock && !barDrag) root.classList.add('resting');
      else wake();
    }, 2200);
}
addEventListener('pointermove', wake);
be.on('snap-hover', (over) => {
  if (over) wake();
});
dockRow.addEventListener('pointerenter', () => {
  overDock = true;
});
dockRow.addEventListener('pointerleave', () => {
  overDock = false;
});
chrome.addEventListener('pointerenter', () => {
  overDock = true;
});
chrome.addEventListener('pointerleave', () => {
  overDock = false;
});
stage.addEventListener('click', () => {
  if (film && (cutting || !anyOpen())) togglePlay();
  else {
    shell.closeAll();
    closeRate();
  }
});
stage.addEventListener('dblclick', () => {
  if (film) toggleFull();
});
stage.addEventListener('contextmenu', (e) => e.preventDefault());

// ---------- the timeline ----------
let barDrag = null;
const barAt = (e) => {
  const r = bar.getBoundingClientRect();
  return clamp((e.clientX - r.left) / r.width, 0, 1) * total();
};
bar.addEventListener('pointerdown', (e) => {
  if (!film || e.button) return;
  bar.setPointerCapture(e.pointerId);
  barDrag = 'seek';
  seek(barAt(e));
  wake();
});
bar.addEventListener('pointermove', (e) => {
  if (!film) return;
  const t = barAt(e),
    r = bar.getBoundingClientRect();
  barTip.textContent = clock(t);
  barTip.style.left = clamp(e.clientX - r.left, 16, r.width - 16) + 'px';
  if (barDrag === 'seek') seek(t);
});
const endBar = () => {
  barDrag = null;
};
bar.addEventListener('pointerup', endBar);
bar.addEventListener('pointercancel', endBar);
bar.addEventListener('keydown', (e) => {
  if (e.key === 'ArrowLeft' || e.key === 'ArrowRight') {
    e.preventDefault();
    e.stopPropagation();
    seek(film.time + (e.key === 'ArrowRight' ? 5 : -5));
  }
});
const trimAt = (e) => {
  const r = trim.getBoundingClientRect();
  return clamp((e.clientX - r.left) / r.width, 0, 1) * total();
};
trim.addEventListener('pointerdown', (e) => {
  if (!film || e.button) return;
  trim.setPointerCapture(e.pointerId);
  barDrag = 'strip';
  seek(trimAt(e));
});
trim.addEventListener('pointermove', (e) => {
  if (barDrag === 'strip') seek(trimAt(e));
});
trim.addEventListener('pointerup', endBar);
trim.addEventListener('pointercancel', endBar);
// The Mac's trackpad (gestures.js): two fingers sideways on the timeline move along it, the playhead following them a
// pixel for a pixel. The timeline has no zoom of its own, so a pinch there does nothing.
if (mac) {
  const fromTrackpad = wheelSource();
  for (const strip of [bar, trim])
    strip.addEventListener(
      'wheel',
      (e) => {
        if (!film || fromTrackpad(e) !== 'trackpad' || Math.abs(e.deltaX) <= Math.abs(e.deltaY)) return;
        e.preventDefault();
        seek(clamp(film.time + (e.deltaX / strip.getBoundingClientRect().width) * total(), 0, total()));
        wake();
      },
      { passive: false }
    );
}
for (const [g, key] of [
  [gripA, 'a'],
  [gripB, 'b'],
]) {
  g.addEventListener('pointerdown', (e) => {
    if (e.button) return;
    e.stopPropagation();
    g.setPointerCapture(e.pointerId);
    barDrag = key;
  });
  g.addEventListener('pointermove', (e) => {
    if (barDrag !== key || !g.hasPointerCapture(e.pointerId)) return;
    setTrim(key, trimAt(e));
  });
  g.addEventListener('pointerup', (e) => {
    e.stopPropagation();
    barDrag = null;
    syncTop();
  });
  g.addEventListener('keydown', (e) => {
    if (e.key !== 'ArrowLeft' && e.key !== 'ArrowRight') return;
    e.preventDefault();
    e.stopPropagation();
    stepTrim(key, e.key === 'ArrowRight' ? 1 : -1);
  });
}
// the grips land only where a lossless cut can: on a keyframe, or the very ends of the film
function nearestKey(t, lo, hi) {
  const k = keys();
  let best = lo;
  for (let i = lo; i <= hi && i < k.length; i++) if (Math.abs(k[i] - t) < Math.abs(k[best] - t)) best = i;
  return best;
}
function placeA(i) {
  keyA = i;
  trimA = i === 0 ? 0 : keys()[i];
  seek(trimA);
  syncTrim();
  syncTop();
}
function placeB(i) {
  keyB = i;
  trimB = i == null ? total() : keys()[i];
  seek(trimB);
  syncTrim();
  syncTop();
}
function setTrim(key, t) {
  const k = keys(),
    lastB = keyB == null ? k.length : keyB;
  if (key === 'a') {
    if (lastB > 0) placeA(nearestKey(t, 0, lastB - 1));
    return;
  }
  if (keyA + 1 >= k.length) {
    placeB(null);
    return;
  }
  const i = nearestKey(t, keyA + 1, k.length - 1);
  placeB(Math.abs(total() - t) < Math.abs(k[i] - t) ? null : i);
}
function stepTrim(key, d) {
  const k = keys();
  if (key === 'a') {
    const lastB = keyB == null ? k.length : keyB;
    placeA(clamp(keyA + d, 0, lastB - 1));
    return;
  }
  const at = (keyB == null ? k.length : keyB) + d;
  if (at >= k.length) placeB(null);
  else if (at > keyA) placeB(at);
}
function syncTrim() {
  const all = total() || 1,
    a = (trimA / all) * 100,
    b = (trimB / all) * 100;
  barKeep.hidden = !trimmed();
  barKeep.style.left = a + '%';
  barKeep.style.width = b - a + '%';
  dimL.style.left = '0';
  dimL.style.width = a + '%';
  dimR.style.left = b + '%';
  dimR.style.right = '0';
  trimFrame.style.left = a + '%';
  trimFrame.style.width = b - a + '%';
  gripA.style.left = tagA.style.left = a + '%';
  gripB.style.left = tagB.style.left = b + '%';
  tagA.textContent = clock(trimA);
  tagB.textContent = clock(trimB);
  // at the ends of the strip the times turn inward, so they never touch the edge of the glass
  tagA.classList.toggle('in', a < 7);
  tagB.classList.toggle('in', b > 93);
  cutKept.textContent = T('截取时长') + ' ' + clock(trimB - trimA);
}
// the strip shows the film itself, so you can see where you're cutting
let stripToken = 0,
  stripFor = null,
  stripW = 0;
function paintStrip() {
  const width = trim.getBoundingClientRect().width || pill.clientWidth - 12 - 24,
    dpr = devicePixelRatio || 1,
    r = { width };
  if (!film || width <= 0 || (stripFor === film && Math.abs(stripW - width) < 2)) return;
  stripFor = film;
  stripW = width;
  const token = ++stripToken,
    c = trimCv,
    H = 56,
    w = Math.max(24, (H * film.width) / film.height),
    n = Math.ceil(width / w);
  c.width = Math.round(width * dpr);
  c.height = Math.round(H * dpr);
  const g = c.getContext('2d');
  g.scale(dpr, dpr);
  const slot = r.width / n,
    times = Array.from({ length: n }, (_, i) => ((i + 0.5) / n) * total());
  film
    .thumbs(
      times,
      (i, src) => {
        g.drawImage(src, i * slot, 0, Math.ceil(slot), H);
      },
      () => token === stripToken
    )
    .catch(() => {});
}
new ResizeObserver(() => {
  if (cutting) paintStrip();
}).observe(trim);

// pressing Trim: the thin line lifts and swells into the strip, and the dock rises around it
let cutting = false,
  leaveCut = null;
const GROW = { duration: 420, easing: EASE };
// where the progress line sits relative to the strip: a band of the strip, and how far the strip must move to put it there
function slit(line, box) {
  const left = Math.max(0, line.left - box.left),
    right = Math.max(0, box.right - line.right),
    mid = box.height / 2 - 1;
  const share = total() ? clamp(film.time / total(), 0, 1) : 0;
  trimFill.style.left = left + 'px';
  trimFill.style.width = share * (box.width - left - right) + 'px';
  return {
    transform: `translateY(${line.top + line.height / 2 - (box.top + box.height / 2)}px)`,
    clipPath: `inset(${mid}px ${right}px ${mid}px ${left}px)`,
  };
}
const OPEN = { transform: 'none', clipPath: 'inset(-30px -20px -10px -20px)' };
function enterCut() {
  if (!film || cutting) return;
  if (!file.facts.can_trim) {
    say('该视频的编码结构不支持关键帧无损裁剪。');
    return;
  }
  if (leaveCut) leaveCut();
  root.classList.remove('rising');
  shell.closeAll();
  closeRate();
  if (film.playing) {
    film.pause();
    syncPlay();
  }
  const line = bar.getBoundingClientRect(),
    h0 = pill.offsetHeight;
  cutting = true;
  root.classList.add('cutting', 'rising');
  cutBtn.setAttribute('aria-pressed', 'true');
  syncTrim();
  paintStrip();
  wake();
  if (reduce) {
    root.classList.remove('rising');
    return;
  }
  const box = trim.getBoundingClientRect();
  pill.animate([{ height: h0 + 'px' }, { height: pill.offsetHeight + 'px' }], GROW);
  trim.animate([slit(line, box), OPEN], GROW).onfinish = () => root.classList.remove('rising');
  trimCv.animate([{ opacity: 0 }, { opacity: 1 }], { ...MOVE, delay: 40, fill: 'backwards' });
  [dimL, dimR, trimFrame, gripA, gripB, tagA, tagB, trimNow].forEach((e) =>
    e.animate([{ opacity: 0 }, { opacity: 1 }], { ...MOVE, delay: 260, fill: 'backwards' })
  );
  [...pillMain.children].forEach((e) => {
    if (e.offsetParent) e.animate([{ opacity: 0 }, { opacity: 1 }], { ...MOVE, delay: 100, fill: 'backwards' });
  });
}
function exitCut(now) {
  if (!cutting) return;
  cutting = false;
  cutBtn.setAttribute('aria-pressed', 'false');
  const parts = [dimL, dimR, trimFrame, gripA, gripB, tagA, tagB, trimNow];
  const finish = () => {
    leaveCut = null;
    [trim, pill, trimCv, ...parts].forEach((e) => e.getAnimations().forEach((x) => x.cancel()));
    root.classList.remove('cutting', 'sinking', 'rising');
    trim.style.left = trim.style.bottom = trim.style.width = '';
    syncTrim();
    syncTop();
    wake();
  };
  if (reduce || now) {
    finish();
    return;
  }
  const box = trim.getBoundingClientRect(),
    glass = pill.getBoundingClientRect(),
    h0 = pill.offsetHeight;
  // the strip lifts out of the layout where it stands, so the dock can close under it
  trim.style.left = box.left - glass.left - pill.clientLeft + 'px';
  trim.style.bottom = glass.bottom - box.bottom - pill.clientTop + 'px';
  trim.style.width = box.width + 'px';
  root.classList.remove('cutting', 'rising');
  root.classList.add('sinking');
  [trimTrack, trimFill].forEach((e) => e.getAnimations().forEach((x) => x.cancel()));
  syncTrim();
  const line = bar.getBoundingClientRect();
  parts.forEach((e) =>
    e.animate([{ opacity: 1 }, { opacity: 0 }], { duration: 140, easing: GROW.easing, fill: 'both' })
  );
  trimCv.animate([{ opacity: 1 }, { opacity: 0 }], { duration: 240, delay: 120, easing: GROW.easing, fill: 'both' });
  pill.animate([{ height: h0 + 'px' }, { height: pill.offsetHeight + 'px' }], GROW);
  const a = trim.animate([OPEN, slit(line, box)], { ...GROW, fill: 'both' });
  [...pillMain.children, barNow, barKeep].forEach((e) => {
    if (e !== bar && e.offsetParent)
      e.animate([{ opacity: 0 }, { opacity: 1 }], { ...MOVE, delay: 140, fill: 'backwards' });
  });
  leaveCut = finish;
  a.onfinish = () => {
    if (leaveCut) leaveCut();
  };
}
cutReset.addEventListener('click', () => {
  sound();
  keyA = 0;
  keyB = null;
  trimA = 0;
  trimB = total();
  syncTrim();
  syncTop();
});
cutDone.addEventListener('click', () => {
  sound();
  exitCut();
});
cutBtn.addEventListener('click', () => {
  sound();
  if (cutting) exitCut();
  else enterCut();
});

// ---------- volume and speed ----------
function paintVol() {
  const v = muted ? 0 : level;
  volIn.value = v;
  volIn.style.setProperty('--b', v * 100 + '%');
  volBtn.innerHTML = icon(v ? 'volume' : 'mute');
  const label = T(v ? '静音' : '取消静音');
  volBtn.setAttribute('aria-label', label);
  volBtn.dataset.tip = label;
  if (film) film.volume = v;
}
function setMuted(on) {
  muted = on;
  if (!on && !level) level = 0.5;
  paintVol();
}
volIn.addEventListener('input', () => {
  level = +volIn.value;
  muted = !level;
  store.set('dotany-video-volume', String(level));
  paintVol();
});
const rateWord = () => (rate === 1 ? T('倍速') : T('{rate} 倍', { rate }));
function setRate(v) {
  rate = v;
  rateBtn.textContent = rateWord();
  if (film) film.rate = v;
  placeRate();
}
const rateSheet = el('div', 'seg rate-sheet'),
  ratePl = el('i', 'pl');
rateSheet.id = 'rateSheet';
rateSheet.appendChild(ratePl);
document.body.appendChild(rateSheet);
const rateBtns = [0.5, 1, 1.25, 1.5, 2].map((v) => {
  const b = el('button', 'sg', String(v));
  b.type = 'button';
  b.dataset.v = v;
  b.addEventListener('mousedown', (e) => e.preventDefault());
  b.addEventListener('click', () => {
    sound();
    setRate(v);
  });
  rateSheet.appendChild(b);
  return b;
});
function placeRate() {
  const on = rateBtns.find((b) => +b.dataset.v === rate);
  rateBtns.forEach((b) => b.setAttribute('aria-pressed', String(b === on)));
  ratePl.style.transform = `translateX(${on.offsetLeft}px)`;
  ratePl.style.width = on.offsetWidth + 'px';
}
function closeRate() {
  if (!rateSheet.classList.contains('on')) return false;
  rateSheet.classList.remove('on');
  rateBtn.removeAttribute('aria-pressed');
  return true;
}
rateBtn.addEventListener('click', () => {
  sound();
  if (closeRate()) return;
  const r = rateBtn.getBoundingClientRect(),
    row = dockRow.getBoundingClientRect(),
    w = rateSheet.offsetWidth;
  rateSheet.style.left = clamp(r.left + r.width / 2 - w / 2, 8, innerWidth - w - 8) + 'px';
  rateSheet.style.bottom = innerHeight - row.top + 8 + 'px';
  placeRate();
  rateBtn.setAttribute('aria-pressed', 'true');
  rateSheet.classList.add('on');
  wake();
});
document.addEventListener('pointerdown', (e) => {
  if (!rateSheet.contains(e.target) && !rateBtn.contains(e.target)) closeRate();
});
function toggleFull() {
  be.windowControl('full');
}
be.on('frame-state', ({ full }) => {
  if (full === fullOn) return;
  fullOn = full;
  // into full screen with the top showing (on the Mac the top only steps aside there), and it rests again as it does
  if (full && platform === 'mac') wake();
  const label = T(full ? '退出全屏' : '全屏');
  fullBtn.innerHTML = icon(full ? 'unfull' : 'full');
  fullBtn.setAttribute('aria-label', label);
  fullBtn.dataset.tip = label;
});

// ---------- subtitles and the info panel ----------
async function openSubs() {
  const path = await be.pickSubtitle(T('字幕文件'));
  if (path) loadSubs(path);
}
async function loadSubs(path) {
  let list = [];
  try {
    list = readSubtitles(subtitleText(await be.readSubtitle(path)));
  } catch {
    /* said below */
  }
  if (!list.length) {
    say('未在该字幕文件中解析出有效内容。');
    return;
  }
  subs = list;
  subsName = baseName(path);
  subsOn = true;
  drawSubs();
  syncTime();
}
function drawSubs() {
  subsPanel.textContent = '';
  const body = el('div', 'side-body'),
    row = el('div', 'side-row'),
    open = el('button', 'btn soft', T('载入字幕文件'));
  open.type = 'button';
  open.id = 'subOpen';
  open.addEventListener('mousedown', (e) => e.preventDefault());
  open.addEventListener('click', () => {
    sound();
    openSubs();
  });
  row.appendChild(open);
  if (subsName) body.appendChild(el('p', 'side-note', subsName));
  body.appendChild(row);
  if (subs.length) {
    const t = el('button', 'row compact');
    t.type = 'button';
    t.id = 'subToggle';
    t.setAttribute('role', 'switch');
    t.setAttribute('aria-checked', String(subsOn));
    t.innerHTML = `<span class="row-t"><b>${T('显示字幕')}</b></span><i class="tog"></i>`;
    t.addEventListener('mousedown', (e) => e.preventDefault());
    t.addEventListener('click', () => {
      sound();
      subsOn = !subsOn;
      t.setAttribute('aria-checked', String(subsOn));
      syncTime();
    });
    const sizeBox = el('div'),
      chips = el('div', 'chips');
    chips.id = 'subSize';
    const btns = [
      ['s', '小'],
      ['m', '适中'],
      ['l', '大'],
    ].map(([v, w]) => {
      const b = el('button', 'chip', T(w));
      b.type = 'button';
      b.dataset.v = v;
      b.setAttribute('aria-pressed', String(v === subsSize));
      b.addEventListener('mousedown', (e) => e.preventDefault());
      b.addEventListener('click', () => {
        sound();
        subsSize = v;
        subline.className = 'subline ' + v;
        btns.forEach((x) => x.setAttribute('aria-pressed', String(x === b)));
      });
      chips.appendChild(b);
      return b;
    });
    sizeBox.append(el('span', 'side-label', T('字幕大小')), chips);
    body.append(t, sizeBox);
  }
  subsPanel.append(el('h3', '', T('字幕')), body);
}
const blobUrl = (bytes) =>
  URL.createObjectURL(new Blob([bytes], { type: bytes[0] === 0x89 ? 'image/png' : 'image/jpeg' }));
function showCover(url) {
  if (cover) URL.revokeObjectURL(cover);
  cover = url;
}
function coverChanged(url) {
  showCover(url);
  coverEdited = true;
  metaDirty = true;
  syncTop();
  drawInfo();
}
async function chooseCover() {
  const path = await be.pickCover(T('图像'));
  if (!path) return;
  try {
    coverChanged(blobUrl(await be.loadCover(path)));
  } catch {
    say('暂不支持该文件格式。');
  }
}
function grabFrame() {
  const c = document.createElement('canvas'),
    w = film.width,
    h = film.height,
    k = Math.min(1, 640 / w);
  c.width = Math.round(w * k);
  c.height = Math.round(h * k);
  c.getContext('2d').drawImage(film.frame(), 0, 0, c.width, c.height);
  c.toBlob(
    async (blob) => {
      const bytes = new Uint8Array(await blob.arrayBuffer());
      if (await be.setCover(bytes)) coverChanged(blobUrl(bytes));
    },
    'image/jpeg',
    0.88
  );
}
function factsLine() {
  const at = fileName.lastIndexOf('.'),
    kind = at > 0 ? fileName.slice(at + 1).toUpperCase() : '';
  return [kind, clock(total()), `${film.width} × ${film.height}`].filter(Boolean).join(' · ');
}
function drawInfo() {
  infoPanel.textContent = '';
  if (!film) return;
  // a film that's only played (AVI, WMV) can't take a title or cover, so none is offered
  if (!file.facts.can_tag) {
    infoPanel.append(el('h3', '', T('信息')), el('p', 'facts', factsLine()));
    return;
  }
  const pic = el('button', 'cover-big' + (cover ? '' : ' none'));
  pic.type = 'button';
  pic.id = 'coverBtn';
  pic.addEventListener('mousedown', (e) => e.preventDefault());
  pic.addEventListener('click', () => {
    sound();
    chooseCover();
  });
  if (cover) {
    const i = el('img');
    i.src = cover;
    i.alt = '';
    pic.append(i, el('span', 'cover-tip', T('更换封面')));
  } else {
    pic.innerHTML = icon('image');
    pic.appendChild(el('span', '', T('添加封面')));
  }
  const words = el('div', 'info-words'),
    field = el('label', 'field'),
    box = el('span', 'field-in'),
    i = document.createElement('input');
  i.id = 'f-title';
  i.value = title;
  i.spellcheck = false;
  i.autocomplete = 'off';
  i.placeholder = T('视频标题');
  i.setAttribute('aria-label', T('视频标题'));
  i.addEventListener('input', () => {
    title = i.value;
    titleEdited = true;
    metaDirty = true;
    syncTop();
  });
  box.appendChild(i);
  field.appendChild(box);
  words.appendChild(field);
  inkfall(i);
  const row = el('div', 'side-row info-actions'),
    use = el('button', 'btn soft', T('设为当前帧画面'));
  use.type = 'button';
  use.id = 'useFrame';
  use.addEventListener('mousedown', (e) => e.preventDefault());
  use.addEventListener('click', () => {
    sound();
    grabFrame();
  });
  row.appendChild(use);
  infoPanel.append(el('h3', '', T('信息')), pic, row, words, el('p', 'facts', factsLine()));
}

// ---------- the little dot: the family's, told only when what would be saved has changed ----------
let lastSig = '';
function syncTop() {
  const now = sig();
  if (now !== lastSig || !dirty()) shell.changed();
  lastSig = now;
}

// ---------- save as: the family's card; the film keeps its format, and nothing is re-encoded ----------
const saveRequest = () => ({
  from_key: keyA > 0 ? keyA : null,
  to_key: keyB,
  title: titleEdited ? title : null,
  cover: coverEdited,
});
async function write(path) {
  const req = saveRequest(),
    f = file,
    at = film.time,
    shift = trimA,
    keep = { subs, subsName, subsOn };
  if (film.playing) {
    film.pause();
    syncPlay();
  }
  let r;
  try {
    r = await be.saveAs(path, req);
  } catch (e) {
    r = { status: 'failed', kind: 'other', detail: String(e) };
  }
  // the file changed under the film since it was read: the row says so, and nothing is mixed (the card goes as it
  // would after a failure; the family's 'ask' leaves the row to us)
  if (r.status === 'changed') {
    shell.showNotice('changed');
    return { status: 'ask' };
  }
  if (r.status !== 'saved') return r;
  savedSig = sig();
  // the window moves to the file just written; what was showing stays on screen, as far on in the new film
  if (file === f) {
    const next = await prepare(r.path);
    if (next.status === 'ok') shell.opened({ ...next, after: { at: Math.max(0, at - shift), shift, keep } });
    else return { ...r, exported: true };
  }
  return r;
}

// ---------- opening ----------
// what Rust read, played to its first picture before it takes over the window: a film that can't play leaves the old one alone
let opening = 0,
  waiting = null;
const letGo = () => {
  if (waiting) {
    waiting.dispose();
    waiting = null;
  }
};
async function prepare(path) {
  const ticket = ++opening;
  letGo();
  let r;
  try {
    r = await be.openPath(path);
  } catch {
    r = { status: 'failed' };
  }
  if (r.status !== 'ok') return r.status === 'failed' ? { status: 'cantplay' } : r;
  // nothing on a Mac plays WMV, and the page says so rather than that a decoder is missing
  if (platform === 'mac' && r.facts.container === 'wmv') return { status: ticket === opening ? 'nowmv' : 'abandoned' };
  // WKWebView's WebM player never answers about an AV1 picture the Mac has no decoder for (no picture, no error), so it
  // is asked, and a film it can't play is refused at once rather than after 30 s
  if (
    platform === 'mac'
    && r.facts.video === 'V_AV1'
    && !document.createElement('video').canPlayType('video/webm; codecs="av01.0.04M.08"')
  )
    return { status: ticket === opening ? 'cantplay' : 'abandoned' };
  // AVI and WMV are played by Windows rather than WebView2, and only played (the Mac plays every film in the page: an
  // MKV or AVI repackaged for it by the Rust side)
  const next = r.facts.system ? new SystemFilm(r.gen, be.systemFilm) : new RealFilm(be.filmUrl(r.gen));
  try {
    await next.ready();
  } catch {
    next.dispose();
    return { status: ticket === opening ? 'cantplay' : 'abandoned' };
  }
  if (ticket !== opening) {
    next.dispose();
    return { status: 'abandoned' };
  }
  await be.commitOpen(r.gen);
  const coverBytes = r.facts.has_cover ? await be.readCover() : null;
  // loaded again from disk, the film doesn't start over by itself
  const again = !!file && file.path === r.path;
  waiting = next;
  return { ...r, next, coverBytes, after: again ? {} : { play: true } };
}
function dropFilm() {
  if (film) {
    film.pause();
    film.dispose();
  }
  film = null;
  file = null;
  subs = [];
  subsName = '';
  showCover('');
  title = '';
  subline.hidden = true;
}
function hold(r) {
  const next = r.next,
    after = r.after || {};
  if (waiting === next) waiting = null;
  dropFilm();
  title = r.facts.title || r.name.replace(/\.[^.]+$/, '');
  if (r.coverBytes) showCover(blobUrl(r.coverBytes));
  if (after.keep) {
    subs = after.keep.subs.map((c) => ({ ...c, a: c.a - after.shift, b: c.b - after.shift })).filter((c) => c.b > 0);
    subsName = after.keep.subsName;
    subsOn = after.keep.subsOn;
  }
  film = next;
  file = r;
  fileName = r.name;
  fileSize = r.size;
  const at = fileName.lastIndexOf('.');
  FORMAT.film.ext = at > 0 ? fileName.slice(at) : '';
  stage.replaceChildren(next.el);
  next.onended = () => {
    syncPlay();
    wake();
  };
  paintVol();
  setRate(rate);
  keyA = 0;
  keyB = null;
  trimA = 0;
  trimB = next.duration;
  metaDirty = false;
  titleEdited = false;
  coverEdited = false;
  savedSig = sig();
  lastSig = savedSig;
  shell.setCount(`${next.width} × ${next.height} · ${money(fileSize)}`);
  exitCut(true);
  stripFor = null;
  setTimeout(paintStrip, 300);
  drawSubs();
  drawInfo();
  closeRate();
  syncTrim();
  syncTop();
  syncPlay();
  syncTime();
  wake();
  if (!reduce) next.el.animate([{ opacity: 0 }, { opacity: 1 }], MOVE);
  navPrev.hidden = navNext.hidden = r.beside < 2;
  soundChecked = false;
  if (after.at) {
    film.time = Math.min(after.at, film.duration);
    syncTime();
  }
  if (after.play) autoPlay();
}
// a stream the system can't read just stays silent, so someone has to notice: after a second of playing, nothing has come out
let soundChecked = false;
function checkSound() {
  if (soundChecked || !film || !file.facts.audio || !film.playing || film.time < 1) return;
  soundChecked = true;
  if (!film.soundBytes) say('无法播放该视频的声音，系统可能缺少相应的音频解码器。');
}
function closeFilm() {
  opening++;
  letGo();
  exitCut(true);
  dropFilm();
  stage.replaceChildren();
  closeRate();
  root.classList.remove('resting');
}
// a subtitle file dropped on a film is its subtitles; anything else is the family's open
async function dropped(paths) {
  const p = paths[0];
  if ((await be.canOpen(p)) === 'subtitle') {
    if (film) loadSubs(p);
    else say('暂不支持该文件格式。');
    return;
  }
  shell.openRequest(p);
}
// a film whose file has changed can't be trusted to keep playing the same film
be.on('file-changed', () => {
  if (film && film.playing) {
    film.pause();
    syncPlay();
  }
});

// the family's keys come first (open, save as, Escape puts away what's open); then Video's
function keyDown(e) {
  const k = e.key,
    typing = /INPUT|TEXTAREA/.test(document.activeElement.tagName) && document.activeElement.type !== 'range';
  if (k === 'Escape') {
    // the Mac: an ⎋ that puts something away is used up here; one with nothing to put away goes on to the system, whose
    // ⎋ leaves full screen (asking for it here as well took the window out and straight back in)
    if (closeRate()) return used(e);
    if (cutting) {
      exitCut();
      return used(e);
    }
    if (fullOn && !mac) be.windowControl('unfull');
    return;
  }
  if (typing || !film) return;
  // the Mac's ⌘ and ⌃ keys are the system's (⌘M minimizes, ⌃⌘F is full screen), not M's mute or F's full screen
  if (mac && (e.metaKey || e.ctrlKey)) return;
  if (k === ' ') {
    e.preventDefault();
    togglePlay();
    return;
  }
  if (document.activeElement.type === 'range') return;
  if (k === 'ArrowRight' || k === 'ArrowLeft') {
    e.preventDefault();
    seek(film.time + (k === 'ArrowRight' ? 5 : -5));
    wake();
  } else if (k === 'ArrowUp' || k === 'ArrowDown') {
    e.preventDefault();
    level = clamp(level + (k === 'ArrowUp' ? 0.05 : -0.05), 0, 1);
    muted = !level;
    paintVol();
    wake();
  } else if (k.toLowerCase() === 'm') {
    setMuted(!muted);
    wake();
  } else if (k.toLowerCase() === 'f') {
    toggleFull();
  }
}

// whatever the family opens (a panel, the card, settings, a menu, the prompt, a notice) brings the top and the dock back
const wakeOn = new MutationObserver((list) => {
  if (list.some((m) => m.target.classList.contains('on'))) wake();
});
document
  .querySelectorAll('.side, .set, .pop, .dlg, .ask-wrap, .notice')
  .forEach((x) => wakeOn.observe(x, { attributes: true, attributeFilter: ['class'] }));

// ---------- words: the family's translate in place; Video's own parts too ----------
shell.translates(dockRow);
shell.onLang(() => {
  syncPlay();
  paintVol();
  rateBtn.textContent = rateWord();
  if (film) {
    drawSubs();
    drawInfo();
    syncTrim();
  }
});

// ---------- begin ----------
paintVol();
if (boot.test) {
  window.__v = {
    get film() {
      return film;
    },
    get file() {
      return file;
    },
    seek,
    get trim() {
      return [trimA, trimB, keyA, keyB];
    },
    get cutting() {
      return cutting;
    },
    get subs() {
      return subs.length;
    },
    get title() {
      return title;
    },
    get cover() {
      return cover;
    },
    dirty,
    openRequest: (p) => dropped([p]),
    setTrim,
    loadSubs,
    saveRequest,
    coverChanged,
    chooseCoverFrom: async (p) => coverChanged(blobUrl(await be.loadCover(p))),
    get sure() {
      return sureSize;
    },
    get full() {
      return fullOn;
    },
    get resting() {
      return root.classList.contains('resting');
    },
    // past the system's save dialog: the card writes straight to this path
    async write(path) {
      shell.test.answerSaveWith(path);
      await shell.openSaveAs();
      document.getElementById('saveAsConfirm').click();
      shell.test.answerSaveWith(null);
    },
  };
}
// the family opens the file the program was started with, then says the window is ready
shell.begin();
