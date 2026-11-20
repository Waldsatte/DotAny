// The one layer between the page and the window around it: title, frame colour, the window's buttons, which parts of
// the top bar move the window, and the Mac's menu bar. Windows and Linux get their buttons here, at the top bar's right;
// macOS keeps its traffic lights, which take the top bar's left through the inset.
import { emit } from '@tauri-apps/api/event';
import { invoke, on, setFrame, setMenuWords, setSnapButton, setTitle, windowControl, windowMenu } from './backend.js';
import { platform } from './keys.js';
import { el, icon, root, sound, tool } from './shared.js';
import { T } from './i18n.js';

// The traffic lights start at the top bar's own left margin (dotany-base's platform/macos.rs `buttons`): three 14px
// buttons 23px apart take 60px (macOS 27; the tests check it), then the bar's own 16px gap
// before its first part
const CONTROLS_INSET = { windows: 0, linux: 0, mac: 76 };
root.dataset.platform = platform;
root.style.setProperty('--frame-inset-left', CONTROLS_INSET[platform] + 'px');

const isDark = () =>
  root.dataset.theme === 'dark'
  || (root.dataset.theme !== 'light' && matchMedia('(prefers-color-scheme: dark)').matches);
export const syncFrame = () => setFrame(isDark());
matchMedia('(prefers-color-scheme: dark)').addEventListener('change', syncFrame);
/** The window's title: the file's name and the tool's. On the Mac also where the file is (`path`), which the system shows
 *  from the title (⌘-click on it in Mission Control and the Window menu); none for a file with no place yet. */
export const setWindowTitle = (name, path) => {
  const title = name ? name + ' - ' + tool.name : tool.name;
  if (platform === 'mac') invoke('set_title', { title, path: (name && path) || null });
  else setTitle(title);
};

/** The top bar's empty parts move the window; its buttons stay buttons. */
export function mountWindowControls(head, meta, plainParts) {
  const moves = [head, meta, ...plainParts];
  moves.forEach((e) => e.setAttribute('data-tauri-drag-region', ''));
  head.addEventListener('contextmenu', (e) => {
    if (!e.target.hasAttribute('data-tauri-drag-region')) return;
    e.preventDefault();
    windowMenu(Math.round(e.screenX), Math.round(e.screenY));
  });
  if (platform === 'mac') {
    // full screen has no traffic lights in the bar. In a window the bar never steps aside on the Mac:
    // the system's buttons are not part of it and could not go with it; a tool whose bar steps aside does
    // so only in full screen (`frame-full` on the page's root)
    on('frame-state', ({ full }) => {
      root.style.setProperty('--frame-inset-left', (full ? 0 : CONTROLS_INSET.mac) + 'px');
      root.classList.toggle('frame-full', !!full);
    });
    if (head.classList.contains('head')) firstClickBar(head);
    // a double-click on the bar's empty parts does what System Settings › Desktop & Dock › "Double-click a window's
    // title bar to" says (zoom, minimize, fill or nothing), as a Mac title bar does, not always zoom: heard before
    // Tauri's own handler, which only zooms. As on a title bar, it counts when the button comes up where it went down
    let at = null;
    const isBar = (e) => e.button === 0 && e.target.hasAttribute && e.target.hasAttribute('data-tauri-drag-region');
    addEventListener(
      'mousedown',
      (e) => {
        if (isBar(e) && e.detail === 2) at = [e.clientX, e.clientY];
      },
      true
    );
    addEventListener(
      'mouseup',
      (e) => {
        if (!isBar(e) || e.detail !== 2) return;
        e.stopImmediatePropagation();
        const still = at && at[0] === e.clientX && at[1] === e.clientY;
        at = null;
        if (still) windowControl('title-double-click');
      },
      true
    );
    return;
  }

  const button = (id, name, label, action) => {
    const b = el('button', 'tb win-btn');
    b.type = 'button';
    b.id = id;
    b.setAttribute('aria-label', label);
    b.dataset.tip = label;
    b.innerHTML = icon(name);
    b.addEventListener('mousedown', (e) => e.preventDefault());
    b.addEventListener('click', () => {
      sound();
      windowControl(action);
    });
    return b;
  };
  const minimize = button('winMin', 'winMin', '最小化', 'minimize');
  const maximize = button('winMax', 'winMax', '最大化', 'toggle');
  const restore = button('winRestore', 'winRestore', '还原', 'toggle');
  const close = button('winClose', 'winClose', '关闭', 'close');
  close.classList.add('win-close');
  restore.classList.add('off');
  meta.append(el('i', 'sep win-sep'), minimize, maximize, restore, close);

  // Windows shows its snap layouts over this one button, so it tells the system where the button is and hears about the pointer back
  const place = () => {
    const shown = maximize.classList.contains('off') ? restore : maximize;
    const r = shown.getBoundingClientRect(),
      s = devicePixelRatio;
    if (r.width)
      setSnapButton(Math.round(r.left * s), Math.round(r.top * s), Math.round(r.width * s), Math.round(r.height * s));
  };
  new ResizeObserver(place).observe(meta);
  addEventListener('resize', place);
  on('snap-hover', (over) => [maximize, restore].forEach((b) => b.classList.toggle('native-hover', over)));
  on('snap-click', () => sound());
  on('frame-state', ({ maximized }) => {
    maximize.classList.toggle('off', maximized);
    restore.classList.toggle('off', !maximized);
    place();
  });
}

/**
 * The Mac: the top bar is the window's toolbar, and a toolbar acts on the click that brings a window behind forward
 * (dotany-base's platform/macos.rs `first_click`, which AppKit asks). Rust is told where the bar is while it shows and
 * nothing covers it, and nothing otherwise: a first click anywhere else only brings the window forward. The bar's
 * empty part is Tauri's drag region, so a window behind can be dragged by it.
 */
function firstClickBar(head) {
  const label = window.__TAURI_INTERNALS__.metadata.currentWindow.label;
  let sent = null,
    queued = 0;
  const onTop = (x, y) => {
    const e = document.elementFromPoint(x, y);
    return !!e && head.contains(e);
  };
  const report = () => {
    queued = 0;
    const r = head.getBoundingClientRect(),
      mid = r.top + r.height / 2;
    // the bar's ends and middle must be the bar, not a dialog's veil or a full-window view over it
    const shows =
      r.width > 0 && r.height > 0 && [r.left + 4, r.left + r.width / 2, r.right - 4].every((x) => onTop(x, mid));
    const rects = shows ? [[r.left, r.top, r.width, r.height]] : [];
    const text = JSON.stringify(rects);
    if (text === sent) return;
    sent = text;
    emit('dotany-first-click', { label, rects });
  };
  // a timer, not an animation frame: a window behind or a locked screen gets no frames, and must still be told
  const soon = () => {
    if (!queued) queued = setTimeout(report, 30);
  };
  new ResizeObserver(soon).observe(head);
  // a bar that slides in or out (Books' top while reading, Video's while a film plays: the bar's holder moves, not the
  // bar itself) is told about once it has stopped
  addEventListener('transitionend', soon, true);
  addEventListener('transitioncancel', soon, true);
  // what covers or hides the bar (a dialog's veil, a full-window view, a class on the page's root) comes and goes by
  // classes or by being in the page
  new MutationObserver(soon).observe(document.documentElement, {
    subtree: true,
    childList: true,
    attributes: true,
    attributeFilter: ['class', 'hidden'],
  });
  addEventListener('resize', soon);
  report();
}

// The Mac's menu bar, item by item, as dotany-base's platform/macos.rs names them (its WORDS lists the same Chinese)
const MENU = {
  about: '关于 {name}::菜单栏',
  settings: '设置…::菜单栏',
  services: '服务::菜单栏',
  hide: '隐藏 {name}::菜单栏',
  hide_others: '隐藏其他::菜单栏',
  show_all: '全部显示::菜单栏',
  quit: '退出 {name}::菜单栏',
  edit: '编辑::菜单栏',
  undo: '撤销::菜单栏',
  redo: '重做::菜单栏',
  cut: '剪切::菜单栏',
  copy: '拷贝::菜单栏',
  paste: '粘贴::菜单栏',
  select_all: '全选::菜单栏',
  window: '窗口::菜单栏',
  minimize: '最小化::菜单栏',
  zoom: '缩放::菜单栏',
  fullscreen: '进入全屏幕::菜单栏',
  unfullscreen: '退出全屏幕::菜单栏',
  bring_all: '前置全部窗口::菜单栏',
  close: '关闭::菜单栏',
  file: '文件::菜单栏',
  open: '打开…::菜单栏',
  open_recent: '打开最近使用::菜单栏',
  clear_recent: '清除菜单::菜单栏',
};
/** The Mac's menu bar: its words in the page's language. Windows has none, and nothing is sent. A tool that makes new
 *  files (`canNew`) has New (新建) in its File menu: its word is sent, and the item goes in. */
export const syncMenu = ({ canNew = false } = {}) => {
  if (platform !== 'mac') return;
  const words = Object.fromEntries(Object.entries(MENU).map(([id, z]) => [id, T(z, { name: tool.name })]));
  if (canNew) words.new = T('新建::菜单栏');
  setMenuWords(words);
};
/** What is chosen in the Mac's menu bar that the page does (About, Settings, Undo, Redo, New, Open…). Never on
 *  Windows. */
export const onAppMenu = (fn) => {
  if (platform === 'mac') on('app-menu', fn);
};
