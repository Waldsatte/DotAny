// DotAny Recorder: one line of sound across the window, the time above it, one round button among plain ones below it,
// and everything recorded so far always at the right. Everything around the work, from the top bar to the settings
// panel and the save card, is the family's shell (shared/web).
import { createShell } from '@dotany/web/shell.js';
import { el, root, reduce, store, icon, EASE, app as family } from '@dotany/web/shared.js';
import { T, size as sizeText } from '@dotany/web/i18n.js';
import { chips } from '@dotany/web/parts.js';
import { convertFileSrc } from '@tauri-apps/api/core';
import { LUCIDE } from './icons.js';
import * as own from './backend.js';

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
  wav: { ext: '.wav', label: 'WAV' },
  flac: { ext: '.flac', label: 'FLAC' },
  mp3: { ext: '.mp3', label: 'MP3' },
};
const MOVE = { duration: 240, easing: EASE };
const money = (b) => sizeText(b, { zero: true, gb: true });
const ease = (t) => 1 - Math.pow(1 - t, 3);
const clockText = (s, tenths = true) => {
  const m = Math.floor(s / 60),
    r = s - m * 60;
  return `${String(m).padStart(2, '0')}:${String(Math.floor(r)).padStart(2, '0')}${tenths ? '.' + Math.floor((r % 1) * 10) : ''}`;
};

// ---------- what's on screen ----------
let phase = 'idle'; // idle, rec, paused, review
// a button's sound never lands in a recording
family.quiet = () => phase === 'rec';
// the take being recorded or listened to: { path, name, rate, frames, peaks, marks, created, gen }
let take = null;
const seconds = () => (take ? take.frames / take.rate : 0);
let fmtKey = 'wav',
  toneKey = 'hi';

const shell = createShell({
  name: 'DotAny Recorder',
  icons: LUCIDE,
  words: WORDS,
  notices: () => import('./notices.generated.js'),
  fileKind: '音频',
  soundNote: '触发按钮操作时播放提示音，录音期间保持绝对静默。',
  licences: [
    ['LAME', '音频编码', 'LGPL-2.0-or-later'],
    ['libFLAC', '音频编码', 'BSD-3-Clause'],
  ],
  formats: FORMATS,
  formatOf: () => fmtKey,
  open: own.openFile,
  show,
  clear,
  // what's recorded is already kept; Save As is for putting a copy somewhere, so nothing here is ever unsaved
  dirty: () => false,
  estimate(fmt) {
    if (!take) return null;
    const s = seconds(),
      ch = take.channels || 1;
    return fmt === 'wav'
      ? take.frames * 2 * ch + 44
      : fmt === 'flac'
        ? take.frames * ch * 1.1
        : s * { std: 128, hi: 192, top: 320 }[toneKey] * 125 + 418;
  },
  saveRows(box, fmt, estimate) {
    fmtKey = fmt;
    if (fmt !== 'mp3') return;
    const w = el('div');
    w.append(
      el('span', 'side-label dyn', T('录音品质')),
      chips({
        id: 'toneChips',
        options: [
          ['std', '标准'],
          ['hi', '高'],
          ['top', '极高'],
        ],
        get: () => toneKey,
        pick: (v) => {
          toneKey = v;
          estimate();
        },
      })
    );
    box.appendChild(w);
  },
  save: (path, fmt) => own.saveFile(path, fmt, toneKey),
  saved(r) {
    // the take is listed by the name it was saved as
    const name = r.name.replace(/\.[^.]*$/, '');
    if (take && take.isTake && name !== take.name) {
      take.name = name;
      own.renameTake(take.path, name).then(refreshTakes);
    }
  },
  keys,
  // with nothing open the recorder is still the recorder: its work stays, and the family's empty page doesn't come
  alwaysWork: true,
});
const say = (t) => shell.say(t);
const sound = shell.sound;

// ---------- the work: the time, the line, the dock ----------
const stage = shell.stage,
  clockBox = el('div', 'clock-box'),
  clockEl = el('div', 'clock', '00:00.0'),
  writtenEl = el('div', 'written'),
  band = el('div', 'band'),
  canvas = document.createElement('canvas');
canvas.id = 'wave';
band.appendChild(canvas);
clockBox.append(clockEl, writtenEl);
stage.append(clockBox, band);
stage.tabIndex = -1;
const dot = shell.head.querySelector('.dot');

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
const leftSide = el('div', 'dock-side'),
  rightSide = el('div', 'dock-side right');
const micBtn = el('button', 'tb dock-word');
micBtn.type = 'button';
micBtn.id = 'micBtn';
micBtn.setAttribute('aria-label', '输入设备');
micBtn.dataset.tip = '输入设备';
micBtn.innerHTML = icon('mic') + '<span class="lab-t dyn"></span>' + icon('chevron');
micBtn.addEventListener('mousedown', (e) => e.preventDefault());
// a take goes to DotAny Audio from its row in the list; the dock's left keeps the microphone
async function openInAudio(t) {
  if (take && t.path === take.path) stopPlay();
  const r = await own.openInAudio(t.path);
  if (r === 'missing') say(T('未检测到 DotAny Audio 组件。'));
  else if (r === 'failed') say(T('无法唤起 DotAny Audio。'));
}
const levelEl = el('div', 'level'),
  levelFill = el('i');
levelEl.appendChild(levelFill);
levelEl.id = 'level';
const sizeEl = el('span', 'dock-num dyn');
sizeEl.id = 'takeSize';
const leftBtn = barBtn('leftBtn', 'pause', '暂停 · 空格', () => leftAction()),
  rightBtn = barBtn('rightBtn', 'flag', '标记节点 · M', () => mark()),
  trashBtn = barBtn('trashBtn', 'trash', '删除此段录音', () => askDelete(currentRow()));
const big = el('button', 'big');
big.type = 'button';
big.id = 'recBtn';
big.innerHTML = '<i class="mark"></i>';
big.addEventListener('mousedown', (e) => e.preventDefault());
big.addEventListener('click', () => {
  sound();
  bigAction();
});
const playBtn = barBtn('playBtn', 'play', '播放 · 空格', () => togglePlay());
const dockMid = el('div', 'dock-mid');
dockMid.append(leftBtn, playBtn, big, rightBtn, trashBtn);
const pill = el('div', 'pill rec-dock');
pill.append(leftSide, el('i', 'sep'), dockMid, el('i', 'sep'), rightSide);
const dockRow = el('div', 'dock-row');
dockRow.appendChild(pill);
const side = el('aside', 'side right on');
side.id = 'takesPanel';
shell.place(side, dockRow);
// the microphone stays at the dock's left, whatever is happening
leftSide.appendChild(micBtn);
// at phone width there's no room for the list to stay, so it comes when asked
const listBtn = shell.headButton('listBtn', 'list', '录音列表');
listBtn.setAttribute('aria-pressed', 'false');
function closeList() {
  if (!root.classList.contains('panel-open')) return false;
  root.classList.remove('panel-open');
  listBtn.setAttribute('aria-pressed', 'false');
  return true;
}
listBtn.addEventListener('click', () => {
  sound();
  if (closeList()) return;
  shell.closeAll();
  root.classList.add('panel-open');
  listBtn.setAttribute('aria-pressed', 'true');
});
function setBoth(box, node) {
  if (box.firstElementChild === node) return;
  box.replaceChildren(node);
  if (!reduce) node.animate([{ opacity: 0 }, { opacity: 1 }], MOVE);
}

// ---------- drawing: only while something changes ----------
// a bar every 4 pixels, moving 60 pixels a second: one pixel a frame on an ordinary screen, which is what looks smooth
const BAR = 4,
  STEP_S = BAR / 60,
  MARGIN = 48;
const cx = canvas.getContext('2d');
let W = 0,
  H = 0,
  DPR = 1,
  colors = {},
  raf = 0,
  morphAt = 0,
  playhead = 0,
  areaW = 1,
  fontName = '';
function syncColors() {
  const s = getComputedStyle(root);
  const v = (n) => s.getPropertyValue(n).trim(),
    text = v('--text');
  const probe = document.createElement('i');
  probe.style.color = `color-mix(in srgb, ${text} 34%, transparent)`;
  document.body.appendChild(probe);
  const dim = getComputedStyle(probe).color;
  probe.style.color = v('--glass-edge');
  const edge = getComputedStyle(probe).color;
  probe.remove();
  colors = { text, dim, edge, now: v('--accent') };
}
function size() {
  const r = band.getBoundingClientRect();
  DPR = devicePixelRatio || 1;
  W = Math.max(1, r.width);
  H = Math.max(1, r.height);
  canvas.width = Math.round(W * DPR);
  canvas.height = Math.round(H * DPR);
}
const railW = () => parseFloat(getComputedStyle(root).getPropertyValue('--rail')) || 0;
function measureArea() {
  areaW = Math.max(1, W - railW());
  fontName = getComputedStyle(root).getPropertyValue('--font-display');
}
// while recording, the line moves with the clock, held to what the disk has heard so it never drifts away from it
let heardAt = 0,
  heardWhen = 0,
  shownAt = 0;
const liveAt = () => {
  if (phase !== 'rec') return seconds();
  shownAt = Math.max(shownAt, Math.min(heardAt + (performance.now() - heardWhen) / 1000, heardAt + 0.12));
  return shownAt;
};
const liveX = (i, at) => areaW / 2 - ((at - i * STEP_S) / STEP_S) * BAR;
const fitX = (i, n) => MARGIN + (areaW - MARGIN * 2) * (n > 1 ? i / (n - 1) : 0);
const atMix = () => {
  if (!morphAt) return phase === 'review' ? 1 : 0;
  return ease(Math.min(1, (performance.now() - morphAt) / 420));
};
function draw() {
  cx.setTransform(DPR, 0, 0, DPR, 0, 0);
  cx.clearRect(0, 0, W, H);
  const mid = H / 2,
    maxH = Math.min(H - 40, 260),
    peaks = take ? take.peaks : [],
    n = peaks.length,
    mix = atMix(),
    at = liveAt();
  cx.fillStyle = colors.edge;
  cx.fillRect(0, Math.round(mid), W, 1);
  if (n) {
    const whole = mix === 1,
      fit = whole && n > (areaW - MARGIN * 2) / BAR,
      total = fit ? Math.floor((areaW - MARGIN * 2) / BAR) : n,
      pos = mix > 0 ? playhead : 1;
    // moving, only the bars on screen are drawn, and the newest rises instead of appearing
    for (let b = mix === 0 ? total - 1 : 0; b >= 0 && b < total; mix === 0 ? b-- : b++) {
      let p,
        x,
        grow = 1;
      if (fit) {
        const a = Math.floor((b * n) / total),
          z = Math.max(a + 1, Math.floor(((b + 1) * n) / total));
        p = 0;
        for (let k = a; k < z; k++) p = Math.max(p, peaks[k]);
        x = fitX(b, total);
      } else {
        p = peaks[b];
        const lx = liveX(b, at),
          fx = fitX(b, n);
        x = lx + (fx - lx) * mix;
        if (mix === 0 && x < -4) break;
        if (phase === 'rec' && !reduce) grow = Math.min(1, Math.max(0.15, (at - (b + 1) * STEP_S) / 0.14 + 0.15));
      }
      if (x < -4 || x > W + 4) continue;
      const h = Math.max(2, Math.pow(Math.min(1, p), 0.8) * maxH * grow);
      cx.fillStyle = mix > 0 && (b + 0.5) / total > pos ? colors.dim : colors.text;
      cx.fillRect(whole ? Math.round(x) : x, mid - h / 2, 2, h);
    }
    take.marks.forEach((s, i) => {
      const bars = s / STEP_S,
        lx = liveX(bars, at),
        fx = fitX(Math.min(bars, n - 1), n),
        x = lx + (fx - lx) * mix;
      if (x < -8 || x > W + 8) return;
      cx.fillStyle = colors.text;
      cx.fillRect(whole ? Math.round(x) : x, Math.round(mid - maxH / 2 - 14), 1, maxH + 18);
      cx.font = '500 11px ' + fontName;
      cx.textBaseline = 'alphabetic';
      cx.fillText(String(i + 1), x + 5, Math.round(mid - maxH / 2 - 6));
    });
  }
  // now, in the family's orange
  if (phase !== 'idle') {
    const nx = MARGIN + (areaW - MARGIN * 2) * playhead,
      x = areaW / 2 + (nx - areaW / 2) * mix;
    cx.fillStyle = colors.now;
    cx.fillRect(Math.round(x) - 1, Math.round(mid - maxH / 2 - 8), 2, maxH + 16);
  }
}
let levelNow = 0;
function tick() {
  raf = 0;
  let again = false;
  if (phase === 'rec') again = true;
  if (morphAt) {
    if (performance.now() - morphAt >= 420) morphAt = 0;
    else again = true;
  }
  if (playing) {
    const s = player.currentTime;
    playhead = Math.min(1, s / Math.max(seconds(), 0.001));
    clockEl.textContent = clockText(s);
    again = true;
  }
  if (phase === 'rec') {
    const c = clockText(seconds());
    if (clockEl.textContent !== c) clockEl.textContent = c;
    const w = written();
    if (writtenEl.textContent !== w) writtenEl.textContent = w;
    levelNow *= 0.86;
    levelFill.style.transform = `scaleX(${Math.min(1, levelNow * 1.3).toFixed(3)})`;
    levelEl.classList.toggle('hot', levelNow >= 0.98);
    const len = side.querySelector('.take.live .take-len'),
      l = clockText(seconds(), false);
    if (len && len.textContent !== l) len.textContent = l;
  }
  draw();
  if (again) wakeDraw();
}
function wakeDraw() {
  if (!raf) raf = requestAnimationFrame(tick);
}

// ---------- the microphone ----------
let mics = [];
const chosenMic = () => {
  const id = store.get('recorder-mic');
  return mics.some((m) => m.id === id) ? id : null;
};
// the device's name is the system's (it may end with … in the dock): the tip says it whole, after Input Device (输入设备)
const syncMicName = () => {
  const m = mics.find((x) => x.id === chosenMic()),
    name = m ? m.name : T('系统默认麦克风');
  micBtn.querySelector('.lab-t').textContent = name;
  micBtn.setAttribute('aria-label', T('输入设备') + ' · ' + name);
  micBtn.dataset.tip = T('输入设备') + ' · ' + name;
};
async function refreshMics() {
  try {
    mics = await own.listMics();
  } catch {
    mics = [];
  }
  syncMicName();
}
// a menu that rises from the dock: the family's, opened upwards
micBtn.addEventListener('click', async () => {
  const was = shell.menuOpen;
  sound();
  shell.closeAll();
  closeList();
  if (was) return;
  await refreshMics();
  const id = chosenMic();
  shell.openPop(
    micBtn,
    [
      { value: '', label: '系统默认麦克风', on: !id },
      ...mics.map((m) => ({ value: m.id, label: m.name, raw: true, on: m.id === id })),
    ],
    (v) => {
      store.set('recorder-mic', v || null);
      syncMicName();
    },
    { up: true }
  );
});

// ---------- the one flow ----------
function paintDock() {
  const review = phase === 'review',
    rec = phase === 'rec',
    paused = phase === 'paused';
  root.classList.toggle('review', review);
  big.classList.toggle('on', rec);
  big.classList.toggle('paused', paused);
  const recTip = rec || paused ? '结束' : '开始录音';
  big.setAttribute('aria-label', T(recTip));
  big.dataset.tip = T(recTip);
  const setBar = (b, ic, tip, off) => {
    b.innerHTML = icon(ic);
    b.setAttribute('aria-label', T(tip));
    b.dataset.tip = T(tip);
    b.disabled = !!off;
  };
  setBar(playBtn, playing ? 'pause' : 'play', playing ? '暂停 · 空格' : '播放 · 空格', !review);
  if (review) setBar(leftBtn, 'start', '回到开头');
  else setBar(leftBtn, paused ? 'play' : 'pause', paused ? '继续 · 空格' : '暂停 · 空格', phase === 'idle');
  setBar(rightBtn, 'flag', '标记节点 · M', phase === 'idle' || (review && !take.isTake));
  setBar(trashBtn, 'trash', '删除此段录音', !review || !take.isTake);
  setBoth(rightSide, review ? sizeEl : levelEl);
  sizeEl.textContent = review ? money(take.size || take.frames * 2 + 44) : '';
  if (phase === 'idle' || paused) {
    levelFill.style.transform = 'scaleX(0)';
    levelEl.classList.remove('hot');
  }
  micBtn.disabled = rec || paused;
  // the family's dot: orange while recording, which is what's happening now; dark while paused
  dot.className = rec ? 'dot now' : paused ? 'dot edited' : 'dot';
}
function stamp() {
  const d = new Date(),
    p = (n) => String(n).padStart(2, '0');
  return T('录音 {time}', {
    time: `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())} ${p(d.getHours())}.${p(d.getMinutes())}`,
  });
}
let starting = false;
async function startRecording() {
  if (starting) return;
  starting = true;
  shell.closeAll();
  closeList();
  stopPlay();
  const r = await own
    .startRecording(stamp(), chosenMic(), heard)
    .catch((e) => ({ status: 'failed', detail: String(e) }));
  starting = false;
  if (r.status !== 'ok') {
    sound();
    say(T(r.status === 'nomic' ? '未检测到可用的麦克风设备。' : '无法启动录音。'));
    return;
  }
  take = {
    path: r.path,
    name: r.title,
    rate: r.rate,
    frames: 0,
    peaks: [],
    marks: [],
    created: r.created,
    isTake: true,
    channels: 1,
  };
  phase = 'rec';
  heardAt = 0;
  heardWhen = performance.now();
  shownAt = 0;
  playhead = 0;
  levelNow = 0;
  morphAt = 0;
  clockEl.textContent = '00:00.0';
  writtenEl.textContent = '';
  // the take being made is the open file: the top bar names it
  await shell.openPath(r.path);
  paintDock();
  shell.setState('录音中');
  refreshTakes();
  wakeDraw();
}
// what the writing thread says, about twenty times a second
function heard(m) {
  if (!take || (phase !== 'rec' && phase !== 'paused')) return;
  if (m.peaks.length) take.peaks.push(...m.peaks);
  take.frames = m.frames;
  heardAt = m.frames / take.rate;
  heardWhen = performance.now();
  if (m.level > levelNow) levelNow = m.level;
  if (m.lost) {
    say(T('麦克风连接已断开，此前录制的音频已自动保存。'));
    stopRecording();
  } else if (m.failed) {
    say(T('无法写入磁盘，已录制部分已保存。'));
    stopRecording();
  }
}
// presses in quick succession each count, one after another
let pauses = Promise.resolve();
function pauseResume() {
  pauses = pauses.then(pauseOrResume, pauseOrResume);
  return pauses;
}
async function pauseOrResume() {
  if (phase === 'rec') {
    phase = 'paused';
    await own.pauseRecording(true);
    levelNow = 0;
    paintDock();
    shell.setState('已暂停');
    draw();
  } else if (phase === 'paused') {
    await own.pauseRecording(false);
    phase = 'rec';
    shownAt = heardAt;
    heardWhen = performance.now();
    paintDock();
    shell.setState('录音中');
    wakeDraw();
  }
}
let stopping = false;
async function stopRecording() {
  if (stopping || (phase !== 'rec' && phase !== 'paused')) return;
  stopping = true;
  const path = await own.stopRecording();
  stopping = false;
  if (!path) return;
  // the line spreads out into the whole take, over --t-3
  morphFrom = true;
  await shell.openPath(path);
}
let morphFrom = false;
async function mark() {
  if (phase === 'rec') {
    const m = await own.addMark(null);
    if (m) take.marks = m;
  } else if (phase === 'review' && take.isTake) {
    const m = await own.addMark(playing ? player.currentTime : playhead * seconds());
    if (m) take.marks = m;
    refreshTakes();
  } else return;
  if (!reduce) {
    writtenEl.classList.remove('swap');
    void writtenEl.offsetWidth;
    writtenEl.classList.add('swap');
  }
  draw();
}
function bigAction() {
  if (phase === 'rec' || phase === 'paused') stopRecording();
  else startRecording();
}
function leftAction() {
  if (phase === 'review') {
    stopPlay();
    playhead = 0;
    player.currentTime = 0;
    clockEl.textContent = clockText(0);
    draw();
  } else pauseResume();
}

// the shell opened a file: the take being made, or one to listen to
function show(r) {
  stopPlay();
  if (r.live) return;
  const was = take && take.path === r.path && morphFrom;
  morphFrom = false;
  take = {
    path: r.path,
    name: r.title,
    rate: r.rate,
    channels: r.channels,
    frames: r.frames,
    peaks: r.peaks,
    marks: r.marks,
    created: r.created,
    isTake: r.take,
    size: r.size,
    gen: r.gen,
  };
  phase = 'review';
  playhead = 0;
  player.src = convertFileSrc(String(r.gen), 'take');
  clockEl.textContent = clockText(0);
  writtenEl.textContent = clockText(seconds());
  paintDock();
  shell.setState('');
  refreshTakes();
  if (was && !reduce) morphAt = performance.now();
  else if (!reduce) canvas.animate([{ opacity: 0 }, { opacity: 1 }], MOVE);
  wakeDraw();
}
// nothing open: ready to record
function clear() {
  stopPlay();
  player.removeAttribute('src');
  if (phase === 'rec' || phase === 'paused') own.stopRecording().then(refreshTakes);
  take = null;
  phase = 'idle';
  playhead = 0;
  morphAt = 0;
  clockEl.textContent = '00:00.0';
  writtenEl.textContent = '';
  paintDock();
  refreshTakes();
  draw();
}

// ---------- listening ----------
// 'metadata', not 'auto': the length and the first bytes are read when a take opens, but WebView2 starts its playing
// machinery only when it plays. Started up front it would hold memory in the GPU process for as long as the window
// lives, even for a take nobody listens to
const player = new Audio();
player.preload = 'metadata';
let playing = false;
async function togglePlay() {
  if (playing) {
    stopPlay();
    return;
  }
  if (phase !== 'review' || !take) return;
  if (playhead >= 1) playhead = 0;
  player.currentTime = playhead * seconds();
  try {
    await player.play();
  } catch {
    return;
  }
  playing = true;
  paintDock();
  wakeDraw();
}
function stopPlay(ended) {
  if (!playing) return;
  playing = false;
  player.pause();
  if (ended) {
    playhead = 1;
    clockEl.textContent = clockText(seconds());
  }
  if (phase === 'review') paintDock();
  draw();
}
player.addEventListener('ended', () => stopPlay(true));
function seekTo(clientX) {
  const r = canvas.getBoundingClientRect(),
    p = Math.max(0, Math.min(1, (clientX - r.left - MARGIN) / Math.max(1, areaW - MARGIN * 2))),
    was = playing;
  stopPlay();
  playhead = p;
  player.currentTime = p * seconds();
  clockEl.textContent = clockText(p * seconds());
  draw();
  if (was) togglePlay();
}
canvas.addEventListener('pointerdown', (e) => {
  if (phase !== 'review') return;
  seekTo(e.clientX);
  canvas.setPointerCapture(e.pointerId);
});
canvas.addEventListener('pointermove', (e) => {
  if (phase === 'review' && e.buttons) seekTo(e.clientX);
});

// ---------- recordings: the side panel, a row for each take ----------
let takes = [],
  listPill = null;
const currentRow = () => take && takes.find((t) => t.path === take.path);
let listing = 0;
async function refreshTakes() {
  const seq = ++listing;
  const r = await own.listTakes();
  if (seq !== listing) return;
  takes = r.rows;
  paintTakes();
}
function whenText(t) {
  if (t.live) return T('正在录音');
  if (t.recovered) return T('异常中断恢复');
  if (t.incomplete) return T('不完整');
  const d = new Date(t.created),
    p = (n) => String(n).padStart(2, '0'),
    hm = `${p(d.getHours())}:${p(d.getMinutes())}`;
  const day = (x) => new Date(x.getFullYear(), x.getMonth(), x.getDate()).getTime(),
    today = day(new Date()),
    that = day(d);
  if (that === today) return `${T('今天')} ${hm}`;
  if (that === today - 86400000) return `${T('昨天')} ${hm}`;
  return `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())} ${hm}`;
}
function paintTakes() {
  const y = side.querySelector('.side-body')?.scrollTop || 0;
  side.textContent = '';
  const top = el('div', 'take-head'),
    clearBtn = el('button', 'text-btn', T('清空'));
  clearBtn.type = 'button';
  clearBtn.id = 'clearTakes';
  clearBtn.classList.toggle('off', !takes.some((t) => !t.live));
  clearBtn.addEventListener('mousedown', (e) => e.preventDefault());
  clearBtn.addEventListener('click', () => {
    sound();
    askClear();
  });
  top.append(el('h3', '', T('录音列表')), clearBtn);
  const body = el('div', 'side-body'),
    list = el('div', 'takes');
  listPill = el('i', 'list-pl');
  list.appendChild(listPill);
  side.append(top, body);
  body.appendChild(list);
  if (!takes.length) body.appendChild(el('p', 'take-none', T('暂无录音记录。')));
  takes.forEach((t) => {
    const b = el('div', 'take' + (t.live ? ' live' : ''));
    b.setAttribute('role', 'button');
    b.tabIndex = 0;
    b.setAttribute('aria-current', String(!!take && t.path === take.path));
    b._take = t;
    b.title = t.path;
    const w = el('span', 'take-t');
    w.append(el('b', '', t.name), el('small', '', whenText(t)));
    const end = el('span', 'take-end');
    end.appendChild(el('span', 'take-len', clockText(t.frames / t.rate, false)));
    // on hover or focus, the length makes way for what can be done with this take: open it in Audio, delete it
    if (!t.live) {
      const act = (cls, n, tip, run) => {
        const d = el('button', 'take-act ' + cls);
        d.type = 'button';
        d.innerHTML = icon(n);
        d.setAttribute('aria-label', T(tip));
        d.addEventListener('mousedown', (e) => e.preventDefault());
        d.addEventListener('click', (e) => {
          e.stopPropagation();
          sound();
          run();
        });
        return d;
      };
      const acts = el('span', 'take-acts');
      acts.append(
        act('take-audio', 'audio', '在 Audio 中编辑', () => openInAudio(t)),
        act('take-del', 'trash', '删除此段录音', () => askDelete(t))
      );
      end.appendChild(acts);
    }
    b.append(w, end);
    const pick = () => {
      if ((!take || t.path !== take.path) && !t.live && phase !== 'rec' && phase !== 'paused') {
        sound();
        closeList();
        shell.openPath(t.path);
      }
    };
    b.addEventListener('mousedown', (e) => {
      if (!e.target.closest('.take-act')) e.preventDefault();
    });
    b.addEventListener('click', pick);
    b.addEventListener('keydown', (e) => {
      if (e.key === 'Enter' && e.target === b) pick();
    });
    list.appendChild(b);
  });
  body.scrollTop = y;
  placeListPill();
}
// the dark block follows its row
function placeListPill() {
  if (!listPill) return;
  const on = side.querySelector('.take[aria-current="true"]');
  if (!on) {
    listPill.style.opacity = 0;
    return;
  }
  const first = listPill.style.opacity !== '1';
  if (first) listPill.style.transition = 'opacity var(--t-1) var(--e-out)';
  listPill.style.opacity = '1';
  listPill.style.transform = `translateY(${on.offsetTop}px)`;
  listPill.style.height = on.offsetHeight + 'px';
  if (first) {
    void listPill.offsetWidth;
    listPill.style.transition = '';
  }
}
// rows that go fold away; the rest close up
async function removeTakes(gone) {
  const paths = gone.map((t) => t.path),
    current = !!take && paths.includes(take.path);
  if (current) {
    stopPlay();
    shell.goEmpty();
  }
  const removed = await own.deleteTakes(paths);
  const rows = [...side.querySelectorAll('.take')].filter((r) => removed.includes(r._take.path));
  const done = () => refreshTakes();
  if (reduce || !rows.length) {
    done();
    return;
  }
  if (listPill && current) listPill.style.opacity = 0;
  let left = rows.length;
  rows.forEach((r) => {
    r.style.pointerEvents = 'none';
    r.style.overflow = 'hidden';
    const a = r.animate(
      [
        { height: r.offsetHeight + 'px', opacity: 1 },
        { height: 0, opacity: 0, paddingTop: 0, paddingBottom: 0, minHeight: 0 },
      ],
      MOVE
    );
    a.onfinish = () => {
      r.remove();
      if (!--left) done();
    };
  });
}
function askDelete(t) {
  if (!t || t.live) return;
  askIt(T('删除「{name}」？', { name: t.name }), () => removeTakes([t]));
}
function askClear() {
  const all = takes.filter((t) => !t.live);
  if (!all.length) return;
  askIt(T('删除全部 {n} 段录音？', { n: all.length }), () => removeTakes(all));
}

// asking before anything goes: the family's prompt, with one way out and one way on
const ask = el('div', 'ask-wrap');
ask.id = 'askDelete';
ask.setAttribute('role', 'alertdialog');
ask.setAttribute('aria-modal', 'true');
const askBox = el('div', 'ask'),
  askTitle = el('p', 'ask-title'),
  askRow = el('div', 'ask-row'),
  aCancel = el('button', 'btn soft'),
  aYes = el('button', 'btn primary');
aCancel.type = aYes.type = 'button';
aCancel.id = 'delCancel';
aYes.id = 'delYes';
askRow.append(el('i', 'ask-gap'), aCancel, aYes);
askBox.append(askTitle, askRow);
ask.appendChild(askBox);
document.body.appendChild(ask);
let askDo = null;
function askIt(title, then) {
  askTitle.textContent = title;
  aCancel.textContent = T('取消');
  aYes.textContent = T('删除');
  askDo = then;
  ask.classList.add('on');
  aYes.focus();
}
const hideAsk = () => {
  ask.classList.remove('on');
  askDo = null;
};
aCancel.addEventListener('click', () => {
  sound();
  hideAsk();
});
aYes.addEventListener('click', () => {
  sound();
  const d = askDo;
  hideAsk();
  if (d) d();
});
ask.addEventListener('keydown', (e) => {
  e.stopPropagation();
  if (e.key === 'Escape') {
    sound();
    hideAsk();
  }
});

// ---------- keys, after the family's ----------
function keys(e) {
  const k = e.key,
    typing = /INPUT|TEXTAREA/.test(document.activeElement.tagName);
  if (k === 'Escape') {
    closeList();
    return;
  }
  if (typing || e.ctrlKey || e.metaKey || e.altKey || ask.classList.contains('on')) return;
  if (k === ' ') {
    e.preventDefault();
    sound();
    if (phase === 'idle') startRecording();
    else if (phase === 'review') togglePlay();
    else pauseResume();
    return;
  }
  if (k === 'r' || k === 'R') {
    sound();
    bigAction();
    return;
  }
  if (k === 'm' || k === 'M') {
    sound();
    mark();
    return;
  }
  if (k === 'Delete' && phase === 'review') {
    sound();
    askDelete(currentRow());
  }
}

// how much is on disk, in the language on screen; a paused take's line is written again when the language changes
function written() {
  return T('已写入 {size}', { size: money(take.frames * 2 + 44) });
}
shell.onLang(() => {
  syncMicName();
  paintDock();
  paintTakes();
  if (phase === 'rec') shell.setState('录音中');
  else if (phase === 'paused') shell.setState('已暂停');
  if ((phase === 'rec' || phase === 'paused') && take) writtenEl.textContent = written();
});
new ResizeObserver(() => {
  size();
  measureArea();
  draw();
  placeListPill();
}).observe(band);
// the list stays below the top bar, and moves down with it while a notice row is open, so the notice's buttons stay in reach
new ResizeObserver(() => root.style.setProperty('--chrome-now', shell.chrome.offsetHeight + 'px')).observe(
  shell.chrome
);
new MutationObserver(() => {
  syncColors();
  draw();
}).observe(root, { attributes: true, attributeFilter: ['data-theme'] });
matchMedia('(prefers-color-scheme: dark)').addEventListener('change', () => {
  syncColors();
  draw();
});

// with DOTANY_TEST set, shell.test.rec exposes the recording state and its actions, so that an automated test can read and drive it
shell.test.rec = {
  get phase() {
    return phase;
  },
  get take() {
    return take;
  },
  get takes() {
    return takes;
  },
  get playhead() {
    return playhead;
  },
  get playing() {
    return playing;
  },
  player,
  refreshTakes,
  bars: () => (take ? take.peaks.length : 0),
};

// ---------- begin ----------
syncColors();
size();
measureArea();
paintDock();
draw();
if (document.fonts && document.fonts.ready)
  document.fonts.ready.then(() => {
    measureArea();
    draw();
  });
refreshMics();
refreshTakes();
shell.begin();
