// The words on screen: every fixed word translates in place, and choosing a language redraws what the parts draw
// themselves.
import { root, store } from '../shared.js';
import { T, lang, setLang, pickLang, langInfo, langReady, loadLang, allWords } from '../i18n.js';
import { setWindowTitle, syncMenu } from '../window-frame.js';

/** Takes from ctx: the parts' nodes and the functions that redraw them; reads ctx.panelNew, ctx.setOpen, ctx.dlgOpen,
 *  ctx.fmtBtns and reads and writes ctx.doc and ctx.fileName. Gives translate, translated, langHooks and applyLang. */
export function language(ctx) {
  const {
    head,
    panel,
    dropView,
    dlg,
    ask,
    pop,
    menu,
    licList,
    ownRow,
    harmonyRow,
    stackRow,
    renderAbout,
    drawEmpty,
    syncSet,
    renderState,
    drawNotice,
    toolNotices,
    syncName,
    placeNav,
    fitPages,
    drawFormats,
    drawRows,
    syncDlg,
  } = ctx;
  // every fixed word translates in place; each element remembers its Chinese. The language page's System too
  const srcText = new WeakMap(),
    srcAttr = new WeakMap();
  function translate(top) {
    const w = document.createTreeWalker(top, NodeFilter.SHOW_TEXT);
    for (let n = w.nextNode(); n; n = w.nextNode()) {
      if (n.parentElement.closest('.name, .dyn, .state, .ext, .warn-t, .upd, .notice-in, .dlg .seg, [data-raw]'))
        continue;
      if (!srcText.has(n)) srcText.set(n, n.nodeValue);
      n.nodeValue = T(srcText.get(n));
    }
    // a field's placeholder too (New's Search formats, Text's find)
    top.querySelectorAll('input[placeholder]').forEach((e) => {
      if (!e.dataset.ph) e.dataset.ph = e.placeholder;
      e.placeholder = T(e.dataset.ph);
    });
    [top, ...top.querySelectorAll('[aria-label], [data-tip]')].forEach((e) => {
      const a = e.getAttribute('aria-label'),
        t = e.dataset.tip;
      if (!a && !t) return;
      if (!srcAttr.has(e)) srcAttr.set(e, [a, t]);
      const o = srcAttr.get(e);
      if (o[0]) e.setAttribute('aria-label', T(o[0]));
      if (o[1]) e.dataset.tip = T(o[1]);
    });
  }
  // the tool's own fixed parts translate the same way; what it draws itself it redraws in onLang
  // the menus too, open or closing: a menu is drawn in the language it was opened in (a file's own words are [data-raw])
  const translated = [head, panel, dropView, dlg, ask, pop, menu, ...(ctx.panelNew ? [ctx.panelNew.wrap] : [])];
  const langHooks = [];
  let shownLang = null;
  function applyLang() {
    setLang(pickLang(store.get('dotany-lang')));
    // a language's table is loaded the first time it is chosen; until it is here its words are English's
    if (!langReady()) {
      const want = lang;
      loadLang(want).then(() => {
        if (lang === want) applyLang();
      });
    }
    root.lang = langInfo().html;
    // the Mac's menu bar in the same language (nothing on Windows), with New for a tool that makes new files
    syncMenu({ canNew: !!ctx.cfg.newFile });
    translated.forEach(translate);
    renderAbout();
    // the program itself stays first; after it, in a language other than Chinese the Latin font comes first
    if (lang.startsWith('zh')) ownRow.after(harmonyRow);
    else stackRow.after(harmonyRow);
    if (shownLang && shownLang !== lang) {
      panel.classList.remove('lang-swap');
      void panel.offsetWidth;
      panel.classList.add('lang-swap');
    }
    shownLang = lang;
    drawEmpty();
    syncSet();
    renderState();
    drawNotice();
    toolNotices.forEach((n) => n.draw());
    // a new file not saved yet keeps the name the program gave it in the language on screen: 未命名.zip, Untitled.zip
    const untitled =
      ctx.doc
      && !ctx.doc.path
      && new RegExp(
        '^(?:'
          + allWords('未命名')
            .map((w) => w.replace(/[.*+?^${}()|[\]\\]/g, '\\$&'))
            .join('|')
          + ')(\\.[\\w.]+)$'
      ).exec(ctx.fileName);
    if (untitled) {
      ctx.fileName = ctx.doc.name = T('未命名') + untitled[1];
      syncName();
      setWindowTitle(ctx.fileName, ctx.doc.path);
    }
    langHooks.forEach((f) => f());
    if (ctx.setOpen) {
      placeNav();
      fitPages();
    }
    drawFormats(ctx.fmtBtns.map((b) => b.dataset.k));
    if (ctx.dlgOpen) {
      drawRows();
      syncDlg(false);
    }
  }
  Object.assign(ctx, { translate, translated, langHooks, applyLang });
}
