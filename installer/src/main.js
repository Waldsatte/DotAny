// DotAny's installer page: the installer's four steps, and the uninstaller's two.
// The Rust side does the work (src-tauri/src/app.rs); this page draws it as it really goes, waiting on nothing.
'use strict';
const { invoke, convertFileSrc } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;
const SETUP = window.__SETUP__ || {};
const root = document.documentElement;
if (SETUP.theme === 'dark' || SETUP.theme === 'light') root.dataset.theme = SETUP.theme;
const EASE = 'cubic-bezier(.2,.7,.2,1)';
const still = () => matchMedia('(prefers-reduced-motion: reduce)').matches;

// ---------- the fonts: the family's own, read from the installer's payload (or, uninstalling, from the install) ----------
const face = (family, file, weight, format = 'truetype') =>
  `@font-face { font-family: "${family}"; src: url("${convertFileSrc(file, 'dotfont')}") format("${format}"); font-weight: ${weight}; font-display: block; }`;
const fontSheet = document.createElement('style');
fontSheet.textContent =
  face('Stack Sans Text', 'StackSansText-wght.ttf', '200 700')
  + face('Stack Sans Headline', 'StackSansHeadline-wght.ttf', '200 700')
  + face('HarmonyOS Sans SC', 'HarmonyOS_SansSC_Regular.ttf', 400)
  + face('HarmonyOS Sans SC', 'HarmonyOS_SansSC_Bold.ttf', 700)
  // the language's own (Rust hands over only the one this run speaks)
  + face('HarmonyOS Sans TC', 'HarmonyOS_Sans_TC.ttf', '100 900')
  + face('Noto Sans JP DotAny', 'NotoSansJP-DotAny-Regular.ttf', 400)
  + face('Noto Sans JP DotAny', 'NotoSansJP-DotAny-Bold.ttf', 700)
  + face('Pretendard', 'Pretendard-Regular.otf', 400, 'opentype')
  + face('Pretendard', 'Pretendard-Bold.otf', 700, 'opentype');
document.head.appendChild(fontSheet);

// ---------- words: Chinese is the source; the other languages are tables (installer/src/lang/<code>.json) ----------
// The installer isn't on the family's base: its Rust picks the language the way every DotAny program does (words.rs)
// and hands this page that table and the English under it. Keys as in shared/web/i18n.js: {n} a value,
// {n, plural, one {…} other {…}} a count (# is the number).
const LANG = SETUP.lang || 'zh';
const HTML = {
  zh: 'zh-CN',
  'zh-Hant': 'zh-Hant-TW',
  en: 'en',
  ja: 'ja',
  ko: 'ko',
  'es-419': 'es-419',
  'pt-BR': 'pt-BR',
  fr: 'fr',
  de: 'de',
};
root.lang = HTML[LANG] || 'en';
const WORDS = LANG === 'zh' ? {} : { ...(SETUP.en || {}), ...(SETUP.words || {}) };
const plural = new Intl.PluralRules(LANG === 'zh' ? 'zh-CN' : LANG);
function choose(s, vars) {
  let out = '',
    i = 0;
  for (;;) {
    const at = s.indexOf(', plural,', i);
    if (at < 0) return out + s.slice(i);
    const open = s.lastIndexOf('{', at),
      name = s.slice(open + 1, at).trim(),
      forms = {};
    let j = at + 9;
    for (;;) {
      while (s[j] === ' ') j++;
      if (s[j] === '}' || j >= s.length) {
        j++;
        break;
      }
      const sel = /^(=?\w+)\s*\{/.exec(s.slice(j));
      if (!sel) break;
      j += sel[0].length;
      let d = 1,
        k = j;
      for (; k < s.length && d; k++) {
        if (s[k] === '{') d++;
        else if (s[k] === '}') d--;
      }
      forms[sel[1]] = s.slice(j, k - 1);
      j = k;
    }
    const n = Number(vars[name]);
    out += s.slice(i, open) + (forms['=' + n] ?? forms[plural.select(n)] ?? forms.other ?? '').replace(/#/g, String(n));
    i = j;
  }
}
const T = (z, vars) => {
  let s = LANG === 'zh' ? z : (WORDS[z] ?? z);
  if (!vars) return s;
  if (s.includes(', plural,')) s = choose(s, vars);
  return s.replace(/\{(\w+)\}/g, (m, k) => (k in vars ? String(vars[k]) : m));
};
const SAY = {
  about: (n) => T('约 {n} MB', { n }),
  need: (n) => T('需要约 {n} MB', { n }),
  all: (n) => T('全部 {n} 个工具', { n }),
  installed: (n, mb) => T('已安装 {n} 个程序，约 {mb} MB', { n, mb }),
  willRemove: (n, mb) => T('将移除 {n} 个程序，约 {mb} MB。', { n, mb }),
  removed: (n, mb) => T('已移除 {n} 个程序，约 {mb} MB。', { n, mb }),
  space: (need, free) => T('空间不足：需要约 {need} MB，当前仅剩 {free} MB。', { need, free }),
  denied: T('当前位置缺少写入权限，请选择其他目录。'),
  other: (detail) => T('未能安装：{detail}', { detail }),
  busy: (names) => T('请先关闭 {names}。', { names: names.join(T('、')) }),
  left: T('部分文件正在使用中，未能删除。'),
  couldnt: (detail) => T('未能卸载：{detail}', { detail }),
};
for (const el of document.querySelectorAll('[data-t]')) el.textContent = T(el.dataset.t);
for (const el of document.querySelectorAll('[data-t-label]')) el.setAttribute('aria-label', T(el.dataset.tLabel));
const $ = (id) => document.getElementById(id);

// ---------- the cards' drawings, from Lucide ----------
const ICON = {
  hub: '<path d="M15 21v-8a1 1 0 0 0-1-1h-4a1 1 0 0 0-1 1v8"/><path d="M3 10a2 2 0 0 1 .709-1.528l7-6a2 2 0 0 1 2.582 0l7 6A2 2 0 0 1 21 10v9a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z"/>',
  text: '<path d="M6 22a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h8a2.4 2.4 0 0 1 1.704.706l3.588 3.588A2.4 2.4 0 0 1 20 8v12a2 2 0 0 1-2 2z"/><path d="M14 2v5a1 1 0 0 0 1 1h5"/><path d="M10 9H8"/><path d="M16 13H8"/><path d="M16 17H8"/>',
  image:
    '<rect width="18" height="18" x="3" y="3" rx="2" ry="2"/><circle cx="9" cy="9" r="2"/><path d="m21 15-3.086-3.086a2 2 0 0 0-2.828 0L6 21"/>',
  audio:
    '<path d="M21 16V3a1 1 0 0 0-1.28-.96l-10 2.917a1 1 0 0 0-.72.96V19"/><circle cx="18" cy="16" r="3"/><circle cx="6" cy="19" r="3"/>',
  video:
    '<rect width="18" height="18" x="3" y="3" rx="2"/><path d="M7 3v18"/><path d="M3 7.5h4"/><path d="M3 12h18"/><path d="M3 16.5h4"/><path d="M17 3v18"/><path d="M17 7.5h4"/><path d="M17 16.5h4"/>',
  pdf: '<path d="M6 22a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h8a2.4 2.4 0 0 1 1.704.706l3.588 3.588A2.4 2.4 0 0 1 20 8v12a2 2 0 0 1-2 2z"/><path d="M14 2v5a1 1 0 0 0 1 1h5"/><path d="M11 18h2"/><path d="M12 12v6"/><path d="M9 13v-.5a.5.5 0 0 1 .5-.5h5a.5.5 0 0 1 .5.5v.5"/>',
  archive:
    '<path d="M13.659 22H18a2 2 0 0 0 2-2V8a2.4 2.4 0 0 0-.706-1.706l-3.588-3.588A2.4 2.4 0 0 0 14 2H6a2 2 0 0 0-2 2v11.5"/><path d="M14 2v5a1 1 0 0 0 1 1h5"/><path d="M8 12v-1"/><path d="M8 18v-2"/><path d="M8 7V6"/><circle cx="8" cy="20" r="2"/>',
  sheets: '<path d="M3 9h18"/><path d="M9 3v18"/><rect x="3" y="3" width="18" height="18" rx="2"/>',
  slides: '<path d="M2 3h20"/><path d="M21 3v11a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V3"/><path d="m7 21 5-5 5 5"/>',
  recorder:
    '<path d="M12 19v3"/><path d="M19 10v2a7 7 0 0 1-14 0v-2"/><rect x="9" y="2" width="6" height="13" rx="3"/>',
  books:
    '<path d="M12 5v16"/><path d="M20.001 19A2 2 0 0022 17V5a2 2 0 00-1.999-2L16 3.002A5 5 0 0012 5a5 5 0 00-4-2H4a2 2 0 00-2 2v12a2 2 0 001.999 2H8a5 5 0 014 2 5 5 0 014-2z"/>',
  model:
    '<path d="M21 8a2 2 0 0 0-1-1.73l-7-4a2 2 0 0 0-2 0l-7 4A2 2 0 0 0 3 8v8a2 2 0 0 0 1 1.73l7 4a2 2 0 0 0 2 0l7-4A2 2 0 0 0 21 16Z"/><path d="m3.3 7 8.7 5 8.7-5"/><path d="M12 22V12"/>',
};
const svg = (id) => `<svg viewBox="0 0 24 24" aria-hidden="true">${ICON[id] || ''}</svg>`;
const esc = (s) => String(s).replace(/[&<>"]/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;' })[c]);

// words that change where they stand fade in, so nothing jumps; the tool being installed changes too often for that
function put(el, text) {
  if (el.textContent === text) return;
  const had = el.textContent !== '';
  el.textContent = text;
  if (had && el.isConnected && !still()) el.animate([{ opacity: 0 }, { opacity: 1 }], { duration: 240, easing: EASE });
}

// ---------- steps: the old one fades out (140ms), then the new one fades in (240ms) ----------
const body = $('body');
let step = null,
  later = 0;
function show(next, then) {
  clearTimeout(later);
  const swap = () => {
    for (const s of body.children) s.hidden = s.id !== next;
    step = next;
    if (then) then();
    body.classList.remove('leaving');
  };
  if (!step || still()) {
    swap();
    return;
  }
  body.classList.add('leaving');
  later = setTimeout(swap, 140);
}

// ---------- state ----------
let boot = null,
  mode = 'full',
  root0 = '',
  picked = {},
  auto = true,
  message = '',
  committing = false;
const FIRST_PICK = {
  text: true,
  image: true,
  audio: true,
  video: true,
  pdf: true,
  archive: true,
  sheets: true,
  slides: true,
  recorder: false,
  books: false,
  model: false,
};
const tools = () => boot.programs.filter((p) => !p.required);
const chosenIds = () =>
  mode === 'update'
    ? boot.ids
    : boot.programs.filter((p) => p.required || mode === 'full' || picked[p.id]).map((p) => p.id);
const customIds = () => boot.programs.filter((p) => p.required || picked[p.id]).map((p) => p.id);
let customMb = 0,
  needSeq = 0;

async function refreshCustom() {
  const seq = ++needSeq;
  const mb = await invoke('need', { programs: customIds() });
  if (seq !== needSeq) return;
  customMb = mb;
  drawChoose();
  put($('customNeed'), SAY.need(customMb));
}

function drawChoose() {
  const full = mode === 'full';
  $('full').setAttribute('aria-pressed', String(full));
  $('pick').setAttribute('aria-pressed', String(!full));
  $('fullMb').textContent = SAY.about(boot.full);
  $('pickMb').textContent = SAY.about(customMb);
  $('path').textContent = root0;
  $('path').title = root0;
  put($('need'), message || SAY.need(full ? boot.full : customMb));
  put($('nextWord'), T(full ? '安装' : '继续'));
}

function drawTools() {
  const box = $('tools');
  if (!box.children.length) {
    box.innerHTML = boot.programs
      .map((p) =>
        p.required
          ? `<button type="button" class="card tool fixed" aria-pressed="true" data-id="${p.id}"><span>${svg(p.id)}<span class="sub">${esc(T('核心组件'))}</span></span><b>${esc(p.name)}</b></button>`
          : `<button type="button" class="card b tool" data-id="${p.id}"><span>${svg(p.id)}<span class="sub num">${p.mb} MB</span></span><b>${esc(p.name)}</b></button>`
      )
      .join('');
    for (const el of box.querySelectorAll('.tool:not(.fixed)'))
      el.addEventListener('click', () => {
        picked[el.dataset.id] = !picked[el.dataset.id];
        message = '';
        drawTools();
        refreshCustom();
      });
  }
  for (const el of box.querySelectorAll('.tool:not(.fixed)'))
    el.setAttribute('aria-pressed', String(!!picked[el.dataset.id]));
}

// ---------- step 3: the line goes as far as the files are unpacked; a tool is ticked once its files are in ----------
// The line's width eases with the family's --t-2 (styles.css), so the real steps read as one movement; nothing waits.
let pct = 0,
  staged = new Set(),
  stagingDone = null,
  rows = [],
  frozen = false;
function drawInstalling() {
  const n = rows.length;
  // a test's still picture (freeze) ticks the rows by the line; otherwise by what is really in
  const firstOpen = rows.findIndex((r) => !staged.has(r.id));
  const at = frozen ? Math.min(n - 1, Math.floor((pct / 100) * n)) : firstOpen < 0 ? n - 1 : firstOpen;
  rows.forEach((r, i) => {
    const done = pct >= 100 || (frozen ? i < at : staged.has(r.id)),
      now = !done && i === at;
    r.el.classList.toggle('on', done || now);
    if (done && !r.el.querySelector('.tick'))
      r.el.insertAdjacentHTML(
        'beforeend',
        `<svg class="tick" viewBox="0 0 24 24" role="img" aria-label="${esc(T('安装完成'))}"><path d="M4 12l5 5L20 6"/></svg>`
      );
  });
  const p = rows[at];
  $('nowName').textContent = pct >= 100 ? T('完成') : 'DotAny ' + (p.required ? T('中枢') : p.name);
  $('pct').textContent = Math.round(pct) + '%';
  $('bar').style.width = pct + '%';
}

async function startInstall() {
  const ids = chosenIds();
  rows = boot.programs.filter((p) => ids.includes(p.id)).map((p) => ({ ...p }));
  pct = 0;
  staged = new Set();
  stagingDone = null;
  committing = false;
  frozen = false;
  show('installing', () => {
    $('rows').innerHTML = rows.map((r) => `<div class="row"><span>${esc(r.name)}</span></div>`).join('');
    rows.forEach((r, i) => {
      r.el = $('rows').children[i];
    });
    drawInstalling();
  });
  try {
    await invoke('install', { root: root0, programs: ids.filter((id) => id !== 'hub') });
  } catch (e) {
    fail({ kind: 'other', detail: String(e) });
    return;
  }
}
// how far the unpacking is; 100 only once staging has said it is done
function unpacked(done, total) {
  if (step !== 'installing' || frozen || committing) return;
  const next = Math.min(99.9, total ? (done / total) * 100 : 0);
  if (next > pct) {
    pct = next;
    drawInstalling();
  }
}
// all in: the line is full for one frame, then the commit
function stagedAll() {
  if (step !== 'installing' || frozen || committing) return;
  pct = 100;
  drawInstalling();
  committing = true;
  requestAnimationFrame(() => requestAnimationFrame(finish));
}

async function finish() {
  committing = true;
  try {
    const r = await invoke('commit', { lang: LANG, autostart: auto });
    committing = false;
    // the update mode has no last step: DotAny itself opens, which says it is done
    if (mode === 'update') {
      invoke('quit');
      return;
    }
    show('done', () => drawDone(r));
  } catch (f) {
    committing = false;
    fail(f);
  }
}

function fail(f) {
  const kind = f && f.kind;
  if (kind === 'cancelled') return;
  message =
    kind === 'space'
      ? SAY.space(Math.ceil(f.need / 1048576), Math.floor(f.free / 1048576))
      : kind === 'denied'
        ? SAY.denied
        : SAY.other((f && f.detail) || String(f));
  if (mode === 'update') {
    show('update', drawUpdate);
    return;
  }
  show('choose', drawChoose);
}

async function cancelInstall() {
  if (committing) return;
  await invoke('cancel');
  if (mode === 'update') {
    show('update', drawUpdate);
    return;
  }
  show('choose', drawChoose);
}

// ---------- step 4 ----------
function shelf(ids) {
  return boot.programs
    .filter((p) => ids.includes(p.id))
    .map((p) => `<span>${svg(p.id)}${esc(p.name)}</span>`)
    .join('');
}
function drawDone(r) {
  $('doneWord').textContent = SAY.installed(r.programs, r.mb);
  $('shelf').innerHTML = shelf(r.ids);
  $('auto').setAttribute('aria-checked', String(auto));
}

// ---------- the update mode: the programs still open are closed first; it goes on by itself once they are ----------
let openNow = [],
  openTimer = 0;
const openNames = () => openNow.map((p) => (p.id ? 'DotAny ' + p.name : p.name));
function drawUpdate() {
  $('updateWord').hidden = !openNow.length;
  if (openNow.length) put($('updateWord'), SAY.busy(openNames()));
  $('updateShelf').innerHTML = openNow.map((p) => `<span>${svg(p.id)}${esc(p.name)}</span>`).join('');
  $('updateNeed').textContent = message;
}
async function lookAgain(go) {
  openNow = await invoke('open_now');
  if (step !== 'update') return;
  if (!openNow.length && (go || !message)) {
    clearInterval(openTimer);
    message = '';
    startInstall();
    return;
  }
  drawUpdate();
}
function waitForClosed() {
  show('update', drawUpdate);
  clearInterval(openTimer);
  openTimer = setInterval(() => {
    if (step === 'update' && !message) lookAgain(false);
  }, 1000);
}

// ---------- the uninstaller ----------
function drawRemove() {
  $('removeWord').textContent = SAY.willRemove(boot.programs.length, boot.mb);
  $('removeShelf').innerHTML = shelf(boot.programs.map((p) => p.id));
  $('removeNeed').textContent = message;
}

// ---------- wiring ----------
$('min').addEventListener('click', () => invoke('minimize'));
$('close').addEventListener('click', () => {
  if (!committing) invoke('quit');
});
$('full').addEventListener('click', () => {
  mode = 'full';
  message = '';
  drawChoose();
});
$('pick').addEventListener('click', () => {
  mode = 'custom';
  message = '';
  drawChoose();
});
$('change').addEventListener('click', async () => {
  const p = await invoke('pick_folder', { current: root0 });
  if (p) {
    root0 = p;
    message = '';
    drawChoose();
  }
});
$('next').addEventListener('click', () => {
  message = '';
  if (mode === 'full') startInstall();
  else
    show('custom', () => {
      drawTools();
      put($('customNeed'), SAY.need(customMb));
    });
});
$('back').addEventListener('click', () => show('choose', drawChoose));
$('install').addEventListener('click', () => startInstall());
$('cancel').addEventListener('click', cancelInstall);
$('auto').addEventListener('click', () => {
  auto = !auto;
  $('auto').setAttribute('aria-checked', String(auto));
  invoke('set_autostart', { on: auto }).catch(() => {});
});
$('finish').addEventListener('click', () => invoke('quit'));
$('openHub').addEventListener('click', () => invoke('open_hub'));
$('settings').addEventListener('click', () => {
  const on = $('settings').getAttribute('aria-checked') !== 'true';
  $('settings').setAttribute('aria-checked', String(on));
});
$('keep').addEventListener('click', () => invoke('quit'));
$('drop').addEventListener('click', async () => {
  try {
    const r = await invoke('uninstall', { removeSettings: $('settings').getAttribute('aria-checked') === 'true' });
    show('removed', () => {
      $('removedWord').textContent = SAY.removed(r.programs, r.mb) + (r.left.length ? ' ' + SAY.left : '');
    });
  } catch (f) {
    message = f && f.kind === 'busy' ? SAY.busy(f.names) : SAY.couldnt((f && f.detail) || String(f));
    put($('removeNeed'), message);
  }
});
$('bye').addEventListener('click', () => invoke('quit'));
$('updateCancel').addEventListener('click', () => invoke('quit'));
$('updateGo').addEventListener('click', () => {
  message = '';
  lookAgain(true);
});
listen('staged', (e) => {
  staged.add(e.payload);
  if (step === 'installing' && !frozen && !committing) drawInstalling();
});
listen('unpacked', (e) => unpacked(e.payload.done, e.payload.total));
listen('staging-done', (e) => {
  if (e.payload.ok) {
    stagingDone = true;
    stagedAll();
  } else fail(e.payload.failure);
});

// ---------- start: draw the first step, then show the window once the fonts are in ----------
(async () => {
  boot = await invoke('boot');
  if (boot.mode === 'install') {
    root0 = boot.root;
    if (boot.existing) {
      // installing over an earlier install: start from what it has
      picked = Object.fromEntries(tools().map((p) => [p.id, boot.existing.programs.includes(p.id)]));
      mode = tools().every((p) => picked[p.id]) ? 'full' : 'custom';
    } else picked = { ...FIRST_PICK };
    $('fullNote').textContent = SAY.all(tools().length);
    customMb = await invoke('need', { programs: customIds() });
    show('choose', drawChoose);
  } else if (boot.mode === 'update') {
    mode = 'update';
    root0 = boot.root;
    auto = boot.autostart;
    openNow = boot.open;
    if (openNow.length) waitForClosed();
    else startInstall();
  } else {
    show('remove', drawRemove);
  }
  try {
    await Promise.race([document.fonts.ready, new Promise((r) => setTimeout(r, 1500))]);
  } catch {
    /* shown anyway */
  }
  root.dataset.ready = '1';
  invoke('ready');
})();
window.__installer = {
  state: () => ({
    step,
    mode,
    pct,
    picked,
    root: root0,
    message,
    rows: rows.map((r) => r.id),
    staged: [...staged],
    done: !!stagingDone,
    committing,
  }),
};
// a test can hold step 3 still at a given point, for a screenshot
if (SETUP.test)
  window.__installer.freeze = (p) => {
    frozen = true;
    pct = p;
    drawInstalling();
  };
