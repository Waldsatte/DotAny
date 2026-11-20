// Settings: the family's panel (Appearance, Interface, Sound, Language, About and its licences), a tool's own pages and
// rows, and settings changed in another window. Then New's panel, which lies after it in the page.
// Appearance, sound and language are the family's (shared/rust/src/family.rs): set here or anywhere, every window follows.
// cfg read here:
// - soundNote: the sound row's line; name: About's name, and the licence list's first row, the program itself
// - licence: the program's own licence on that row, when it isn't the family's MIT (Books: GPL-3.0-or-later; the same
//   as its LICENSE, which shared/scripts/own-licence.mjs prints at the top of the long text)
// - licences: [[name, role, licence, where], …], the tool's own rows: where 'font' (with the fonts), 'core' (before
//   Tauri) or nothing (after Tauri, before Additional Open-Source Components)
// - notices: () => import('./notices.generated.js'), the long list of every library, loaded when the licences are opened
// - focus: the settings panel never takes the focus from the work (Text's caret).
// - newFile.groups: New's panel (opening.js).
// shell.settingsPage(key, title, before): a settings page of the tool's own, before the page named (Text's Save).
// shell.settingRow(page, { … }): a switch of the tool's own at the end of a page.
import * as be from '../backend.js';
import { el, root, reduce, store, sound, EASE, icon, restingRect } from '../shared.js';
import { T, LANGS } from '../i18n.js';
import { HARMONY_LICENCE } from '../licences.js';
import { onAppMenu, syncFrame } from '../window-frame.js';
import { createNewPanel } from '../new-panel.js';
import { languageRow } from '../parts.js';
import { later } from './later.js';

/** Takes from ctx: cfg, boot, setBtn, menu, focusWork, openPop and the close functions, NF; calls applyLang and
 *  translate (language.js); writes ctx.panelNew. Gives the functions below, the about page's licence rows, and
 *  ctx.setOpen. */
export function settings(ctx) {
  const { cfg, boot, setBtn, menu, focusWork, openPop, closePanels, closeSaveAs, closePop, NF } = ctx;
  const { applyLang, translate } = later(ctx);
  // ---------- settings: the family's panel ----------
  const panel = el('div', 'set');
  panel.setAttribute('role', 'dialog');
  const nav = el('nav', 'set-nav'),
    navPill = el('i', 'set-pl'),
    pagesEl = el('div', 'set-pages');
  nav.appendChild(navPill);
  panel.append(nav, pagesEl);
  document.body.appendChild(panel);
  const face = (v) =>
    '<span class="face face-'
    + v
    + '"><i class="top"><i class="seg-m"></i></i><i class="h"></i><i class="l"></i><i class="l s"></i><i class="cr"></i></span>';
  const preview = (v) => '<span class="pv">' + (v === 'system' ? face('light') + face('dark') : face(v)) + '</span>';
  const savedTheme = store.get('dotany-theme');
  if (savedTheme === 'light' || savedTheme === 'dark') root.dataset.theme = savedTheme;
  if (store.get('dotany-labels') === 'off') root.classList.add('no-labels');
  const mk = (title) => {
    const p = el('section', 'set-pg');
    p.appendChild(el('h2', '', title));
    return p;
  };
  const appearance = mk('外观'),
    cards = el('div', 'cards');
  const themeCards = [
    ['system', '跟随系统'],
    ['light', '浅色'],
    ['dark', '深色'],
  ].map(([v, t]) => {
    const b = el('button', 'card');
    b.type = 'button';
    b.dataset.v = v;
    b.innerHTML = preview(v) + '<span>' + t + '</span>';
    b.addEventListener('click', () => {
      if (v === 'system') delete root.dataset.theme;
      else root.dataset.theme = v;
      store.set('dotany-theme', v === 'system' ? null : v);
      syncFrame();
      syncSet();
    });
    cards.appendChild(b);
    return b;
  });
  appearance.appendChild(cards);
  const iface = mk('界面'),
    labelRow = el('button', 'row');
  labelRow.type = 'button';
  labelRow.setAttribute('role', 'switch');
  labelRow.id = 'labelsRow';
  labelRow.innerHTML =
    '<span class="row-t"><b>显示顶栏按钮标签</b><small>在顶栏图标旁常驻显示文字标签。关闭后仅保留图标，悬停时显示名称。</small></span><i class="tog"></i>';
  labelRow.addEventListener('click', () => {
    const off = !root.classList.contains('no-labels');
    root.classList.toggle('no-labels', off);
    store.set('dotany-labels', off ? 'off' : null);
    syncSet();
  });
  iface.appendChild(labelRow);
  const soundPage = mk('声音'),
    soundRow = el('button', 'row');
  soundRow.type = 'button';
  soundRow.setAttribute('role', 'switch');
  soundRow.id = 'soundRow';
  soundRow.innerHTML = '<span class="row-t"><b>交互音效</b><small></small></span><i class="tog"></i>';
  soundRow.querySelector('small').textContent = cfg.soundNote || '触发按钮操作或完成保存时播放提示音。';
  soundRow.addEventListener('click', () => {
    store.set('dotany-sound', store.get('dotany-sound') === 'on' ? null : 'on');
    syncSet();
  });
  soundPage.appendChild(soundRow);
  // one row and the family's menu: System, then every language, each named in its own language
  const language = mk('语言'),
    langRow = languageRow({
      LANGS,
      get: () => store.get('dotany-lang'),
      pick: (v) => {
        store.set('dotany-lang', v);
        applyLang();
        syncSet();
      },
      openPop,
    });
  language.appendChild(langRow.row);
  const about = el('section', 'set-pg'),
    aboutMain = el('div'),
    aboutLic = el('div');
  aboutLic.hidden = true;
  aboutMain.appendChild(el('h2', '', '关于'));
  const licOpen = el('button', 'btn soft', '开源许可与字体协议');
  licOpen.type = 'button';
  licOpen.id = 'openLicences';
  // the about page says the version and has the licences; it has no update check
  const ver = el('p', 'about-ver dyn');
  ver.id = 'aboutVer';
  function renderAbout() {
    ver.textContent = T('版本 {version}', { version: boot.version || '1.0.0' });
  }
  const ab = el('div', 'about'),
    abBtns = el('div', 'about-btns');
  abBtns.append(licOpen);
  ab.append(el('p', 'about-name', cfg.name), ver, abBtns);
  aboutMain.appendChild(ab);
  const licBack = el('button', 'lic-back');
  licBack.type = 'button';
  licBack.innerHTML = icon('back');
  licBack.setAttribute('aria-label', '返回');
  const licHead = el('div', 'lic-head');
  licHead.append(licBack, el('h2', '', '开源许可与字体协议'));
  const licList = el('div', 'lic-list');
  // a tool's own rows: [name, role, licence, where], where 'font' (with the fonts), 'core' (before Tauri) or nothing (after it)
  const own = (w) => (cfg.licences || []).filter((x) => (x[3] || '') === w);
  // the first row is the program itself: the family is under MIT, a tool says otherwise with cfg.licence (Books: GPL)
  [
    [cfg.name, '本程序', cfg.licence || 'MIT'],
    ['HarmonyOS Sans', '中文字体', 'HarmonyOS Sans Fonts License'],
    ['Stack Sans', '西文与数字字体', 'SIL OFL 1.1'],
    ['Noto Sans JP', '日文字体', 'SIL OFL 1.1'],
    ['Pretendard', '韩文字体', 'SIL OFL 1.1'],
    ...own('font'),
    ['Lucide', '图标', 'ISC'],
    ...own('core'),
    ['Tauri', '应用框架', 'MIT / Apache-2.0'],
    ...own(''),
    ['其他开源组件', '完整清单与协议见下方', 'MIT, Apache-2.0…'],
  ].forEach(([n, r, l]) => {
    const row = el('div', 'lic-row'),
      left = el('span', 'lic-n');
    left.append(el('b', n === '其他开源组件' ? '' : 'dyn', n), el('small', '', r));
    row.append(left, el('span', 'lic-l dyn', l));
    licList.appendChild(row);
  });
  const ownRow = licList.firstElementChild,
    harmonyRow = ownRow.nextElementSibling,
    stackRow = harmonyRow.nextElementSibling;
  const licText = el('pre', 'lic-text dyn', HARMONY_LICENCE);
  licText.id = 'licText';
  licText.tabIndex = 0;
  aboutLic.append(licHead, licList, licText);
  about.append(aboutMain, aboutLic);
  function showLicences(on) {
    const was = aboutLic.hidden ? aboutMain : aboutLic,
      now = on ? aboutLic : aboutMain;
    [aboutMain, aboutLic].forEach((e) => {
      e.getAnimations().forEach((a) => a.cancel());
      e.classList.remove('leaving-pg');
    });
    aboutMain.hidden = on;
    aboutLic.hidden = !on;
    if (was !== now && !reduce && !about.hidden) {
      was.hidden = false;
      was.classList.add('leaving-pg');
      const out = was.animate(
        [
          { opacity: 1, transform: 'none' },
          { opacity: 0, transform: 'translateX(' + (on ? -12 : 12) + 'px)' },
        ],
        { duration: 160, easing: EASE, fill: 'forwards' }
      );
      out.onfinish = () => {
        was.classList.remove('leaving-pg');
        was.hidden = true;
        out.cancel();
      };
      now.animate(
        [
          { opacity: 0, transform: 'translateX(' + (on ? 16 : -16) + 'px)' },
          { opacity: 1, transform: 'none' },
        ],
        { duration: 320, delay: 50, easing: EASE, fill: 'backwards' }
      );
    }
    fitPages();
  }
  // the long list of every library arrives when the page is first opened
  let noticesAdded = false;
  licOpen.addEventListener('click', () => {
    showLicences(true);
    if (noticesAdded || !cfg.notices) return;
    noticesAdded = true;
    // the program's own licence first (own-licence.mjs writes it with its separating line)
    cfg.notices().then(({ OWN = '', NOTICES: all }) => {
      licText.textContent = OWN + HARMONY_LICENCE + '\n\n' + all;
    });
  });
  licBack.addEventListener('click', () => showLicences(false));
  const PAGES = [
    ['appearance', '外观', appearance],
    ['interface', '界面', iface],
    ['sound', '声音', soundPage],
    ['language', '语言', language],
    ['about', '关于', about],
  ];
  const navTab = ([k, t]) => {
    const b = el('button', 'set-tab', t);
    b.type = 'button';
    b.dataset.page = k;
    b.addEventListener('click', () => showPage(k));
    return b;
  };
  const navBtns = PAGES.map((pg) => {
    const b = navTab(pg);
    nav.appendChild(b);
    pagesEl.appendChild(pg[2]);
    return b;
  });
  /** A page of the tool's own, before the page named (Text's Save goes before Language). Returns the
   *  page; settingRow(key, …) puts rows on it. */
  function settingsPage(key, title, before = 'language') {
    const pg = mk(title),
      at = PAGES.findIndex((x) => x[0] === before),
      entry = [key, title, pg],
      b = navTab(entry);
    PAGES.splice(at, 0, entry);
    navBtns.splice(at, 0, b);
    nav.insertBefore(b, navBtns[at + 1]);
    pagesEl.insertBefore(pg, PAGES[at + 1][2]);
    translate(b);
    translate(pg);
    return pg;
  }
  let curPage = 'appearance',
    setOpen = false;
  function placeNav() {
    const on = navBtns[PAGES.findIndex((p) => p[0] === curPage)],
      r = document.createRange();
    const longest = Math.max(
        ...navBtns.map((b) => {
          r.selectNodeContents(b);
          return r.getBoundingClientRect().width;
        })
      ),
      w = Math.ceil(longest) + 48;
    panel.style.setProperty('--nav-w', w + 'px');
    navPill.style.transform = `translate(${on.offsetLeft}px, ${on.offsetTop}px)`;
    navPill.style.width = (getComputedStyle(nav).flexDirection === 'column' ? w - 20 : on.offsetWidth) + 'px';
    navPill.style.height = on.offsetHeight + 'px';
  }
  function fitPages() {
    pagesEl.style.height = PAGES.find((p) => p[0] === curPage)[2].offsetHeight + 'px';
  }
  let shownPage = null;
  function showPage(k) {
    if (k !== 'about') showLicences(false);
    const idx = (x) => PAGES.findIndex((p) => p[0] === x),
      next = PAGES[idx(k)][2],
      dir = idx(k) > idx(curPage) ? 1 : -1,
      was = shownPage;
    curPage = k;
    PAGES.forEach(([key, , p], i) => {
      p.getAnimations().forEach((a) => a.cancel());
      p.classList.remove('leaving-pg');
      p.hidden = key !== k;
      navBtns[i].setAttribute('aria-current', String(key === k));
    });
    shownPage = next;
    if (was && was !== next && setOpen && !reduce) {
      was.hidden = false;
      was.classList.add('leaving-pg');
      const out = was.animate([{ opacity: 1 }, { opacity: 0, transform: `translateY(${-10 * dir}px)` }], {
        duration: 160,
        easing: EASE,
        fill: 'forwards',
      });
      out.onfinish = () => {
        was.classList.remove('leaving-pg');
        was.hidden = was !== shownPage;
        out.cancel();
      };
      next.animate(
        [
          { opacity: 0, transform: `translateY(${14 * dir}px)` },
          { opacity: 1, transform: 'none' },
        ],
        { duration: 320, delay: 50, easing: EASE, fill: 'backwards' }
      );
    }
    placeNav();
    fitPages();
  }
  /** A switch row of the tool's own at the end of a settings page ('appearance', 'interface', 'sound', 'language'), as
   *  the family's rows are made: { id, title, note, get, set }. get() answers on or off (or a promise of it); set(on)
   *  makes it so, and the row then shows what get() says. */
  const ownRows = [];
  function settingRow(page, { id, title, note, get, set }) {
    const row = el('button', 'row');
    row.type = 'button';
    row.setAttribute('role', 'switch');
    if (id) row.id = id;
    row.innerHTML = '<span class="row-t"><b></b><small></small></span><i class="tog"></i>';
    row.querySelector('b').textContent = title;
    row.querySelector('small').textContent = note || '';
    const sync = async () => {
      row.setAttribute('aria-checked', String(!!(await get())));
    };
    row.addEventListener('click', async () => {
      await set(row.getAttribute('aria-checked') !== 'true');
      await sync();
    });
    PAGES.find((x) => x[0] === page)[2].appendChild(row);
    ownRows.push(sync);
    sync();
    translate(row);
    if (setOpen) fitPages();
    return row;
  }
  function syncSet() {
    ownRows.forEach((f) => f());
    const c = root.dataset.theme || 'system';
    themeCards.forEach((b) => b.setAttribute('aria-pressed', String(b.dataset.v === c)));
    labelRow.setAttribute('aria-checked', String(!root.classList.contains('no-labels')));
    soundRow.setAttribute('aria-checked', String(store.get('dotany-sound') === 'on'));
    langRow.sync();
  }
  function closeSettings() {
    if (!setOpen) return false;
    setOpen = false;
    panel.classList.remove('on');
    setBtn.removeAttribute('aria-pressed');
    focusWork();
    return true;
  }
  // a tool with a caret keeps it where it was while settings are used (Text)
  if (cfg.focus)
    panel.addEventListener('mousedown', (e) => {
      if (!e.target.closest('.lic-text')) e.preventDefault();
    });
  function openSettings() {
    closePanels();
    closeSaveAs(true);
    closePop();
    setOpen = true;
    setBtn.setAttribute('aria-pressed', 'true');
    syncSet();
    const r = restingRect(setBtn);
    panel.style.top = Math.round(r.bottom + 8) + 'px';
    panel.style.right = Math.max(8, Math.round(innerWidth - r.right)) + 'px';
    showPage(curPage);
    requestAnimationFrame(() =>
      requestAnimationFrame(() => {
        if (setOpen) {
          placeNav();
          panel.classList.add('on');
        }
      })
    );
  }
  setBtn.addEventListener('click', () => {
    sound();
    if (closeSettings()) return;
    openSettings();
  });
  // the Mac's menu bar (Windows has none): Settings opens what the top bar's Settings opens, About the same at its
  // About page
  onAppMenu((id) => {
    // Undo and Redo chosen in the menu bar act as ⌘Z and ⇧⌘Z: the tool's own undo, or else a text field's
    if (id === 'undo' || id === 'redo') {
      const at = document.activeElement || document.body;
      const key = new KeyboardEvent('keydown', {
        key: 'z',
        code: 'KeyZ',
        metaKey: true,
        shiftKey: id === 'redo',
        bubbles: true,
        cancelable: true,
      });
      if (
        at.dispatchEvent(key)
        && at.closest
        && at.closest('input, textarea, [contenteditable]:not([contenteditable="false"])')
      )
        document.execCommand(id);
      return;
    }
    // (Open… and New are opening.js's)
    if (id !== 'about' && id !== 'settings') return;
    if (id === 'about') showPage('about');
    if (!setOpen) openSettings();
  });
  document.addEventListener('pointerdown', (e) => {
    if (setOpen && !panel.contains(e.target) && !setBtn.contains(e.target) && !menu.contains(e.target)) closeSettings();
  });
  panel.addEventListener('click', (e) => {
    if (e.target.closest('button')) sound();
  });

  // every family setting is the family's (appearance, labels, sound, language, autosave): a change in DotAny or in any
  // other tool reaches this window at once, as if it were made here
  const settingsHooks = [];
  function tookSettings(values) {
    const was = store.get('dotany-lang');
    Object.entries(values).forEach(([k, v]) => store.took(k, v));
    const theme = store.get('dotany-theme');
    if (theme === 'light' || theme === 'dark') root.dataset.theme = theme;
    else delete root.dataset.theme;
    root.classList.toggle('no-labels', store.get('dotany-labels') === 'off');
    syncFrame();
    if (store.get('dotany-lang') !== was) applyLang();
    else syncSet();
    settingsHooks.forEach((f) => f(values));
  }
  be.on('shared-settings', tookSettings);
  if (NF && NF.groups)
    ctx.panelNew = createNewPanel({
      groups: NF.groups,
      create: (f) => NF.create(f),
      before: () => {
        closePop();
        closeSettings();
        closeSaveAs(true);
      },
      after: focusWork,
    });
  Object.assign(ctx, {
    panel,
    settingsPage,
    showPage,
    settingRow,
    syncSet,
    closeSettings,
    tookSettings,
    settingsHooks,
    renderAbout,
    placeNav,
    fitPages,
    licList,
    ownRow,
    harmonyRow,
    stackRow,
  });
  Object.defineProperties(ctx, {
    setOpen: { get: () => setOpen },
  });
}
