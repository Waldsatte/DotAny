// Begin: the shell a tool gets back from createShell, and begin(), which opens what the program was started with and
// says the window is ready.
// cfg read here:
// - beforeShow(), startup(): awaited before the window shows (Text: its fonts), and more for the start-up measures.
// Started with --new (DotAny's New), a tool with newFile runs it as the window opens; --new=csv passes 'csv' to run.
import * as be from '../backend.js';
import { fontsChosen } from '../fonts.js';
import { root, sound } from '../shared.js';
import { lang, langReady, loadLang, testLanguage } from '../i18n.js';
import { setWindowTitle, syncFrame } from '../window-frame.js';
import { fitChips } from '../parts.js';
import { morph } from './morph.js';

/** Takes everything the parts gave ctx; reads ctx.doc, ctx.fileName, ctx.noticeKind, ctx.panelNew, ctx.popFor,
 *  ctx.popEl, ctx.setOpen and writes ctx.doc, ctx.fileName and ctx.testSave. Gives the shell. */
export function makeShell(ctx) {
  const {
    cfg,
    boot,
    t0,
    app,
    stage,
    chrome,
    head,
    metaEl,
    dot,
    nameEl,
    toast,
    emptyView,
    menu,
    countEl,
    focusWork,
    dirty,
    setState,
    changed,
    syncName,
    savedInPlace,
    saveReady,
    headButton,
    sidePanel,
    closeAll,
    openSaveAs,
    closeSaveAs,
    cardOpen,
    say,
    retrySaveAs,
    showNotice,
    hideNotice,
    saveFailed,
    makeNotice,
    ask,
    guard,
    askEncoding,
    openDoc,
    openPath,
    openRequest,
    openPicker,
    goEmpty,
    NF,
    newFile,
    step,
    closePop,
    openPop,
    showPage,
    settingRow,
    settingsPage,
    closeSettings,
    tookSettings,
    settingsHooks,
    placeNav,
    fitPages,
    translate,
    translated,
    langHooks,
    applyLang,
  } = ctx;
  // ---------- begin ----------
  const shell = {
    app,
    stage,
    chrome,
    head,
    metaEl,
    sound,
    say,
    setState,
    changed,
    guard,
    step,
    morph,
    openPath,
    openRequest,
    openPicker,
    goEmpty,
    openSaveAs,
    closeAll,
    headButton,
    sidePanel,
    openPop,
    settingRow,
    notice: makeNotice,
    settingsPage,
    askEncoding,
    saveReady,
    savedInPlace,
    dot,
    closeSettings,
    closeSaveAs,
    retrySaveAs,
    showNotice,
    hideNotice,
    saveFailed,
    get noticeKind() {
      return ctx.noticeKind;
    },
    /** A file the tool made or opened itself (a new file not saved yet): { path, name, dir }, as cfg.open answers */
    opened: (r) => openDoc(r),
    /** The open file is now at another path or has another name (the tool saved it there itself) */
    named({ path, name }) {
      if (!ctx.doc) return;
      if (path) {
        ctx.doc.path = path;
        ctx.doc.dir = path.replace(/[\\/][^\\/]*$/, '');
      }
      if (name && name !== ctx.fileName) {
        ctx.fileName = ctx.doc.name = name;
        syncName();
        setWindowTitle(name, ctx.doc.path);
      }
    },
    /** Settings changed in another window of the same tool, as shared-settings does: { key: value } */
    tookSettings,
    onSettings(fn) {
      settingsHooks.push(fn);
    },
    get cardOpen() {
      return cardOpen();
    },
    get askOpen() {
      return ask.classList.contains('on');
    },
    get newOpen() {
      return !!ctx.panelNew && ctx.panelNew.isOpen();
    },
    newPanel: ctx.panelNew,
    openNew: newFile,
    applyLang,
    translate,
    /** Puts away the tool's menu (not the file menu); true when there was one. */
    closePop: () => (ctx.popEl === menu ? closePop() : false),
    get menuOpen() {
      return !!ctx.popFor && ctx.popEl === menu;
    },
    get doc() {
      return ctx.doc;
    },
    get fileName() {
      return ctx.fileName;
    },
    /** The number before Settings (cfg.count), or nothing. */
    count: countEl,
    setCount(text) {
      if (countEl && countEl.textContent !== text) countEl.textContent = text;
    },
    /** The tool's floating parts (its pill, a selection bar) go above the work and under the top bar. */
    place(...parts) {
      parts.forEach((x) => app.insertBefore(x, emptyView));
    },
    /** The tool's fixed parts that translate in place, like the top bar's. */
    translates(...parts) {
      translated.push(...parts);
    },
    onLang(fn) {
      langHooks.push(fn);
    },
    // with DOTANY_TEST set, shell.test exposes the shell's state and actions, so that an automated test can read and drive it
    test: {
      be,
      openPath,
      openRequest,
      goEmpty,
      get doc() {
        return ctx.doc;
      },
      get dirty() {
        return dirty();
      },
      answerSaveWith(path) {
        ctx.testSave = path === null ? null : () => path;
      },
    },
    /** After the tool has drawn its own parts: open the file the program was started with, then say the window is ready. */
    begin() {
      if (boot.test) window.__a = shell.test;
      // a layout check shows the window in a made-up language and back, whatever is stored
      if (boot.test)
        window.__lang = async (code) => {
          await testLanguage(code);
          applyLang();
        };
      changed();
      applyLang();
      showPage('appearance');
      nameEl.textContent = cfg.name;
      if (document.fonts && document.fonts.ready)
        document.fonts.ready.then(() => {
          if (ctx.setOpen) placeNav();
        });
      // a language's own font arrives after its words (繁體中文, 日本語, 한국어 the first time): measure the pages' names again
      if (document.fonts)
        document.fonts.addEventListener('loadingdone', () => {
          if (ctx.setOpen) {
            placeNav();
            fitPages();
          }
          refit();
        });
      // chips whose words don't fit take more of their row (parts.js fitChips): whenever chips are drawn or the room changes
      // a row shown later (a side panel opening) is measured when it gets its size
      let fitting = 0;
      const sized = new ResizeObserver(() => refit());
      const refit = () => {
        if (!fitting)
          fitting = requestAnimationFrame(() => {
            fitting = 0;
            document.querySelectorAll('.chips').forEach((b) => sized.observe(b));
            fitChips();
          });
      };
      const hasChips = (n) => n.nodeType === 1 && (n.matches('.chips, .chip') || !!n.querySelector('.chips'));
      new MutationObserver((rs) => {
        if (rs.some((r) => [...r.addedNodes].some(hasChips))) refit();
      }).observe(document.body, { childList: true, subtree: true });
      addEventListener('resize', refit);
      langHooks.push(refit);
      refit();
      (async () => {
        syncFrame();
        if (boot.start) {
          root.classList.remove('is-empty');
          const opened = await openPath(boot.start);
          if (!opened) {
            const why = toast.textContent;
            goEmpty();
            if (why) say(why);
          }
        } else {
          goEmpty();
          // started from DotAny's New (--new, or --new=csv): a tool that makes new files starts one; one that asks
          // which kind shows its panel, or makes the kind named
          if (boot.new != null && NF) {
            if (ctx.panelNew) {
              const f = boot.new && ctx.panelNew.byId(boot.new);
              if (f) await NF.create(f);
              else ctx.panelNew.open();
            } else NF.run(boot.new);
          }
        }
        // a language whose table loads when chosen is in before the window shows (Chinese and English always are)
        if (!langReady()) {
          await loadLang();
          applyLang();
        }
        const opened = performance.now();
        // what has to be there before the window shows (Text: its fonts)
        // which HarmonyOS Sans SC the page uses (Windows' or its own, fonts.js), so the window never shows the other first
        await fontsChosen;
        if (cfg.beforeShow) await cfg.beforeShow();
        const before = performance.now();
        await be.windowReady();
        const shown = performance.now();
        focusWork();
        const focused = performance.now();
        let firstFrame = 0;
        requestAnimationFrame(() => {
          firstFrame = performance.now();
          requestAnimationFrame(() => {
            root.dataset.ready = '1';
            be.startupDone({
              navigationStartToScript_ms: t0,
              scriptToOpened_ms: opened - t0,
              beforeShow_ms: before - opened,
              showCall_ms: shown - before,
              focusCall_ms: focused - shown,
              firstFrame_ms: firstFrame,
              toFirstFrame_ms: firstFrame - focused,
              toSecondFrame_ms: performance.now() - firstFrame,
              painted_ms: performance.now(),
              file: ctx.fileName || null,
              lang,
              platform: root.dataset.platform,
              systemDark: matchMedia('(prefers-color-scheme: dark)').matches,
              ...(cfg.startup ? cfg.startup() : {}),
            });
          });
        });
      })();
    },
  };
  return shell;
}
