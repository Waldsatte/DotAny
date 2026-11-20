// The file menu, and the menu a tool opens from its own buttons: the family's .pop.
// cfg read here:
// - newFile: New (newFile.menuLabel, or its label) under Open…, {Mod+N}.
// - print(): Print… under Save As… while a file is open, {Mod+P}.
// - fileItems(doc): [{ value, label, run }] — the tool's own items while a file is open, after Open… (Video's Load
//   Subtitles…).
// - saveInPlace: the menu is New…, Open…, Save {Mod+S}, Save As… {Mod+Shift+S}, Print…, Close File; with
//   the card open, the file name first puts it away.
import { el, sound, restingRect } from '../shared.js';
import { T } from '../i18n.js';
import { baseName } from '../paths.js';
import { later } from './later.js';

/** Takes from ctx: cfg, the window's nodes, what the menu's items do (opening, saving, the card); calls closeSettings;
 *  reads ctx.doc, ctx.recents and ctx.dlgOpen. Gives closePop and openPop, and ctx.popFor and ctx.popEl. */
export function fileMenu(ctx) {
  const {
    cfg,
    head,
    pop,
    menu,
    fileBtn,
    inPlace,
    focusWork,
    saveByHand,
    closePanels,
    closeSaveAs,
    cardOpen,
    openSaveAs,
    NF,
    newFile,
    openPicker,
    openRequest,
    guard,
    goEmpty,
  } = ctx;
  const { closeSettings } = later(ctx);
  // ---------- the file menu, and the tool's menus: the family's .pop ----------
  let popFor = null,
    popEl = pop;
  function closePop() {
    if (!popFor) return false;
    popEl.classList.remove('on');
    popFor = null;
    fileBtn.removeAttribute('aria-expanded');
    return true;
  }
  /** A menu from a button: [{ value, label, hint, on, raw, id } | { sep } | { title }], or a function that fills it.
   *  opts.up opens it upwards from a bar (the dock), opts.at = [x, y] at the pointer; otherwise it hangs below the button. */
  function openPop(btn, items, pick, opts = {}) {
    if (popFor === btn) {
      closePop();
      return;
    }
    closePop();
    const box = btn === fileBtn ? pop : menu;
    box.textContent = '';
    if (typeof items === 'function') items(box);
    else
      items.forEach((it) => {
        if (it.sep) {
          box.appendChild(el('i', 'pop-sep'));
          return;
        }
        if (it.title) {
          box.appendChild(el('span', 'pop-title', T(it.title)));
          return;
        }
        const b = el('button');
        b.type = 'button';
        b.appendChild(el('span', '', it.raw ? it.label : T(it.label)));
        if (it.hint) b.appendChild(el('span', 'hint', it.hint));
        if (it.id) b.id = it.id;
        if (it.raw) b.dataset.raw = '';
        if (it.on) b.setAttribute('aria-pressed', 'true');
        b.addEventListener('mousedown', (e) => e.preventDefault());
        b.addEventListener('click', () => {
          sound();
          closePop();
          pick(it.value);
        });
        box.appendChild(b);
      });
    const r = opts.at ? null : restingRect(btn);
    if (box === pop) {
      pop.style.left = Math.max(8, Math.round(r.left)) + 'px';
      pop.style.top = head.getBoundingClientRect().height + 8 + 'px';
    } else {
      const x = opts.at ? opts.at[0] : r.left;
      menu.style.left = Math.max(8, Math.min(innerWidth - 8 - menu.offsetWidth, x)) + 'px';
      menu.style.top = opts.up ? 'auto' : (opts.at ? opts.at[1] : r.bottom + 6) + 'px';
      menu.style.bottom = opts.up ? innerHeight - r.top + 14 + 'px' : 'auto';
      menu.style.transformOrigin = opts.up ? '50% 100%' : '50% 0';
    }
    popFor = btn;
    popEl = box;
    requestAnimationFrame(() =>
      requestAnimationFrame(() => {
        if (popFor === btn) box.classList.add('on');
      })
    );
  }
  document.addEventListener('pointerdown', (e) => {
    if (popFor && !popEl.contains(e.target) && !(popFor.contains && popFor.contains(e.target))) closePop();
  });
  fileBtn.addEventListener('mousedown', (e) => e.preventDefault());
  fileBtn.addEventListener('click', () => {
    sound();
    // with the card open, the file name first puts it away
    if (ctx.dlgOpen && inPlace) {
      closeSaveAs();
      return;
    }
    closePanels();
    closeSaveAs(true);
    closeSettings();
    const others = ctx.recents.filter((p) => !ctx.doc || p !== ctx.doc.path).slice(0, 5);
    const printItem = cfg.print ? [{ value: 'print', label: '打印…', hint: T('{Mod+P}'), id: 'menuPrint' }] : [];
    const newItem = NF ? [{ value: 'new', label: NF.menuLabel || NF.label, hint: T('{Mod+N}'), id: 'menuNew' }] : [];
    const recentItems = others.length
      ? [
          { sep: true },
          { title: '最近打开' },
          ...others.map((p) => ({ value: 'recent:' + p, label: baseName(p), raw: true })),
        ]
      : [];
    // the tool's own items while a file is open, after Open… (打开…) (Video's Load Subtitles…)
    const ownItems = ctx.doc && cfg.fileItems ? cfg.fileItems(ctx.doc) : [];
    // a tool that saves in place has Save {Mod+S} and Save As… {Mod+Shift+S}; the others have Save As… on {Mod+S}
    const items = inPlace
      ? [
          ...newItem,
          { value: 'open', label: '打开…', hint: T('{Mod+O}') },
          ...ownItems,
          ...(ctx.doc
            ? [
                { sep: true },
                { value: 'save', label: '保存', hint: T('{Mod+S}'), id: 'menuSave' },
                { value: 'as', label: '另存为…', hint: T('{Mod+Shift+S}') },
                ...printItem,
                { sep: true },
                { value: 'close', label: '关闭文件' },
              ]
            : []),
          ...recentItems,
        ]
      : [
          { value: 'open', label: '打开…', hint: T('{Mod+O}') },
          ...newItem,
          ...ownItems,
          ...(ctx.doc
            ? [
                { value: 'as', label: '另存为…', hint: T('{Mod+S}') },
                ...printItem,
                { sep: true },
                { value: 'close', label: '关闭文件' },
              ]
            : []),
          ...recentItems,
        ];
    openPop(fileBtn, items, (v) => {
      const own = ownItems.find((it) => it.value === v);
      if (own) own.run();
      else if (v === 'open') openPicker();
      else if (v === 'new') newFile();
      else if (v === 'save') saveByHand();
      else if (v === 'as') openSaveAs(fileBtn);
      else if (v === 'print') cfg.print();
      else if (v === 'close') guard(goEmpty);
      else openRequest(v.slice(7));
      if (!cardOpen()) focusWork();
    });
    if (popFor === fileBtn) fileBtn.setAttribute('aria-expanded', 'true');
  });
  Object.assign(ctx, { closePop, openPop });
  Object.defineProperties(ctx, {
    popFor: { get: () => popFor },
    popEl: { get: () => popEl },
  });
}
