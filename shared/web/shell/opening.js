// Opening: a file, the next one in its folder, the empty window with its recent files, New, and the window's keys.
// cfg read here:
// - open(path): the tool's open command, answering { status: 'ok', path, dir, name, … } or { status: 'refused' | … };
//   show(r): draw what was opened; clear(): back to nothing; cantOpen: more of what open can answer
// - canOpen(path): the tool's own look before anything is asked ('ok' when it decides on opening).
// - dropTitle: the empty window's title ("Drop an audio file here"); fileKind: the open dialog's filter name (Audio)
// - openLabel: the empty window's first button, Open Local Image for Open Local File.
// - alwaysWork (createShell): the empty window draws nothing.
// - newFile: { label: '新建压缩包', run() } — the empty window's second button, New under Open… (file-menu.js) and
//   {Mod+N}. run is asked about unsaved work first.
// - newFile.groups: [{ key, label, items: [{ id, label, ext, also }] }] — New first asks which kind (new-panel.js,
//   made in settings.js); newFile.create(item) makes it, newFile.menuLabel is the menu's word (New…).
// - multiWindow: New while a file is open makes the new file in another window of the program, so nothing is asked.
//   The Rust side opens it with dotany_base::open_window_as.
// - keys(e): the tool's own shortcuts, after the family's ({Mod+O}, {Mod+N}, {Mod+S}, {Mod+P}, Escape)
// - print(): {Mod+P} prints while a file is open (Print… in the menu: file-menu.js).
// - saveInPlace: {Mod+S} saves and {Mod+Shift+S} is Save As.
// - editors: where the tool's own editors are, so the keys the window blocks reach them first (Text's ProseMirror and
//   CodeMirror); textMenus: more of the tool's own where the right-click menu is kept.
import * as be from '../backend.js';
import { el, root, sound, icon } from '../shared.js';
import { T } from '../i18n.js';
import { baseName } from '../paths.js';
import { onAppMenu, setWindowTitle } from '../window-frame.js';
import { later } from './later.js';
import { modKey, platform, used } from '../keys.js';

/** Takes from ctx: cfg, boot, the window's nodes, the file's state, closeAll, say, openSaveAs, the notices' functions,
 *  guard; calls closePop; reads and writes ctx.doc, ctx.fileName, ctx.kept and reads ctx.noticeKind. Gives the
 *  functions below, NF (cfg.newFile), ctx.recents, and ctx.panelNew, which settings.js makes. */
export function fileOpening(ctx) {
  const {
    cfg,
    boot,
    emptyView,
    inPlace,
    saveAsBtn,
    setState,
    changed,
    syncName,
    saveByHand,
    closeAll,
    say,
    openSaveAs,
    kinds,
    hideNotice,
    toolNotices,
    guard,
  } = ctx;
  const { closePop } = later(ctx);
  // ---------- opening ----------
  let recents = boot.recents || [];
  const CANT_OPEN = {
    refused: '暂不支持该文件格式。',
    damaged: '无法读取该文件。',
    missing: '未找到该文件，可能已被移动或删除。',
    ...(cfg.cantOpen || {}),
  };
  function openDoc(r) {
    closeAll();
    if (!kinds()[ctx.noticeKind]) hideNotice();
    ctx.kept = false;
    ctx.doc = { ...r };
    ctx.fileName = r.name;
    root.classList.remove('is-empty');
    syncName();
    setWindowTitle(r.name, r.path);
    if (cfg.show) cfg.show(r);
    changed();
  }
  let opening = 0;
  async function openPath(path) {
    const seq = ++opening;
    setState('正在读取…');
    const r = await cfg.open(path);
    if (seq !== opening || r.status === 'abandoned') return false;
    if (r.status === 'ok') {
      setState('');
      openDoc(r);
      return true;
    }
    if (r.status === 'missing' && recents.includes(path)) be.forgetRecent(path);
    setState('');
    sound();
    say(T(CANT_OPEN[r.status] || CANT_OPEN.damaged));
    return false;
  }
  // a file the tool won't open doesn't get to interrupt anything: it's refused before unsaved work is asked about
  async function openRequest(path) {
    const verdict = cfg.canOpen ? await cfg.canOpen(path) : await be.canOpen(path);
    if (verdict !== 'ok') {
      sound();
      say(T(CANT_OPEN[verdict] || CANT_OPEN.refused));
      if (verdict === 'missing' && recents.includes(path)) be.forgetRecent(path);
      return;
    }
    guard(() => openPath(path));
  }
  async function openPicker() {
    const path = await be.pickOpen(T(cfg.fileKind || '文件'));
    if (path) openRequest(path);
  }
  function goEmpty() {
    opening++;
    be.closeDocument();
    ctx.doc = null;
    ctx.fileName = '';
    ctx.kept = false;
    closeAll();
    hideNotice();
    toolNotices.forEach((n) => n.hide());
    if (cfg.clear) cfg.clear();
    root.classList.add('is-empty');
    syncName();
    setWindowTitle('', null);
    changed();
    drawEmpty();
    setState('');
  }
  // a tool that makes new files has a second button, a menu item and a key for it. One that asks which kind first has
  // a panel for that; one with more windows makes the new file in another window while a file is open
  const NF = cfg.newFile || null;
  // (made after the settings panel, in settings.js, so it lies after it in the page)
  let panelNew = null;
  const newFile = () => {
    if (!NF) return;
    if (panelNew) panelNew.open();
    else if (cfg.multiWindow && ctx.doc) NF.run();
    else guard(() => NF.run());
  };
  const newQuick = () => {
    if (!NF) return;
    if (panelNew) {
      closePop();
      panelNew.quick();
    } else newFile();
  };
  function drawEmpty() {
    emptyView.textContent = '';
    if (cfg.alwaysWork) return;
    const open = el('button', 'btn primary', T(cfg.openLabel || '打开本地文件'));
    open.type = 'button';
    open.id = 'emptyOpen';
    open.addEventListener('click', () => {
      sound();
      openPicker();
    });
    const actions = el('div', 'empty-actions');
    actions.appendChild(open);
    if (cfg.newFile) {
      const fresh = el('button', 'btn soft', T(cfg.newFile.label));
      fresh.type = 'button';
      fresh.id = 'emptyNew';
      fresh.addEventListener('click', () => {
        sound();
        newFile();
      });
      actions.appendChild(fresh);
    }
    const box = el('div', 'empty-recent');
    if (recents.length) {
      box.appendChild(el('p', 'empty-label', T('最近打开')));
      // at most five, here and in the file menu
      recents.slice(0, 5).forEach((p) => {
        const b = el('button', 'recent-row');
        b.type = 'button';
        b.innerHTML = icon('file');
        b.appendChild(el('span', 'recent-name', baseName(p)));
        b.title = p;
        b.addEventListener('click', () => {
          sound();
          openRequest(p);
        });
        box.appendChild(b);
      });
    }
    // with no recent files no room is kept for them: the title and buttons sit in the middle
    emptyView.append(
      el('p', 'empty-title', T(cfg.dropTitle || '拖拽文件至此')),
      actions,
      ...(recents.length ? [box] : [])
    );
  }
  be.on('recents', (list) => {
    recents = list;
    if (!ctx.doc) drawEmpty();
  });
  // a file the Mac handed over (opened in Finder, dropped on the Dock icon): as if dropped on the window. Windows starts a
  // program on each file instead, so nothing listens there
  if (platform === 'mac') be.on('open-path', (path) => openRequest(path));
  // the Mac's File menu's New, chosen with this window in front: as the window's own {Mod+N} (its Open… is the Mac
  // side's
  // own: a sheet on this window, the file going where a file from Finder goes)
  onAppMenu((id) => {
    if (id === 'new') newQuick();
  });
  // a new document of a kind, asked for by DotAny's New (the tool's address, dotany-<tool>://new) and given to this
  // window by the Mac's side: as if the window had been started with --new=<kind>
  if (platform === 'mac')
    be.on('new-file', (kind) => {
      if (!NF) return;
      if (panelNew) {
        const f = kind && panelNew.byId(kind);
        if (f) NF.create(f);
        else panelNew.open();
      } else if (cfg.multiWindow && ctx.doc) NF.run(kind);
      else guard(() => NF.run(kind));
    });
  // the next or previous file in this one's folder, by name as people read names
  function step(d) {
    if (!ctx.doc) return;
    guard(async () => {
      const path = await be.neighbour(d);
      if (path) await openPath(path);
    });
  }

  addEventListener('keydown', (e) => {
    const mod = modKey(e),
      k = e.key;
    // the Mac's ⌘O and ⌘N are its File menu's (Open…, New): the page lets them go there, and the menu does the same
    if (platform === 'mac' && mod && !e.shiftKey && !e.altKey && ['o', 'n'].includes(k.toLowerCase())) return;
    if (mod && k.toLowerCase() === 'o') {
      e.preventDefault();
      openPicker();
      return;
    }
    if (mod && k.toLowerCase() === 'n' && cfg.newFile) {
      e.preventDefault();
      newQuick();
      return;
    }
    if (mod && k.toLowerCase() === 's' && inPlace) {
      // in place: {Mod+S} saves and {Mod+Shift+S} is Save As, unless the tool's editor took it, or it's typed in a
      // field
      if (e.altKey || e.defaultPrevented || (e.target.closest && e.target.closest('input, textarea'))) return;
      e.preventDefault();
      if (ctx.doc) {
        if (e.shiftKey) openSaveAs(saveAsBtn);
        else saveByHand();
      }
      return;
    }
    if (mod && k.toLowerCase() === 's') {
      e.preventDefault();
      if (ctx.doc) openSaveAs(saveAsBtn);
      return;
    }
    if (mod && k.toLowerCase() === 'p' && cfg.print) {
      e.preventDefault();
      if (ctx.doc) cfg.print();
      return;
    }
    if (k === 'Escape' && closeAll()) return used(e);
    if (cfg.keys) cfg.keys(e);
  });
  // WebView2's own find bar, print preview and reload don't belong in a DotAny window, and a reload would lose unsaved
  // work: blocked before anything else sees the key. The keys still reach the tool. In the tool's
  // own editors (cfg.editors) they're blocked after the editor has had them: an editor leaves alone a key
  // already blocked, and its find, its multiple carets and the rest are on these keys
  // On the Mac the page sees ⌘ keys before the menu bar does, so the system's own (⌘H hides the program) are never blocked;
  // and only ⌘ counts there (modKey): ⌃E, ⌃K, ⌃D, ⌃F, ⌃P are a text field's own moves on the Mac, never blocked
  const blockedKeys = platform === 'mac' ? 'rfgjudkep' : 'rfghjudkep';
  const blocked = (e) => {
    const k = e.key.toLowerCase();
    // (⌃⌘F is the Mac's full screen, the window menu's: never blocked)
    const system = platform === 'mac' && e.ctrlKey;
    return k === 'f5' || (modKey(e) && !e.altKey && !system && k.length === 1 && blockedKeys.includes(k));
  };
  const inEditor = (e) => cfg.editors && e.target.closest && e.target.closest(cfg.editors);
  addEventListener(
    'keydown',
    (e) => {
      if (blocked(e) && !inEditor(e)) e.preventDefault();
    },
    true
  );
  if (cfg.editors)
    addEventListener('keydown', (e) => {
      if (blocked(e) && inEditor(e)) e.preventDefault();
    });
  // and its right-click menu has Back, Refresh and Print in it; only what can be typed in keeps one, for cut, copy and paste
  // (cfg.textMenus: more of the tool's own where it's kept, such as Text's lines of a long document)
  addEventListener('contextmenu', (e) => {
    if (!(
      e.target.closest
      && e.target.closest(
        'input, textarea, [contenteditable]:not([contenteditable="false"]), .lic-text'
          + (cfg.textMenus ? ', ' + cfg.textMenus : '')
      )
    ))
      e.preventDefault();
  });
  Object.assign(ctx, { openDoc, openPath, openRequest, openPicker, goEmpty, NF, newFile, drawEmpty, step });
  Object.defineProperties(ctx, {
    recents: { get: () => recents },
    panelNew: {
      get: () => panelNew,
      set: (v) => {
        panelNew = v;
      },
    },
  });
}
