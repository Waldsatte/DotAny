// DotAny Text: the page. The window is the family's (shell.js, createShell); each of Text's parts lives in its own module,
// and this one joins them up and opens the first file.
import './boot.js';
import { app, store } from '@dotany/web/shared.js';
import { FOLLOW } from '@dotany/web/core.js';
import { T } from './i18n.js';
import { icon, LUCIDE } from './icons.js';
import { on, windowLabel } from './backend.js';
import { shell, appEl, chrome, findBtn, scroller, closePop, saveAsBtn } from './shell.js';
import { motion, onBars, resetBar, schedulePages, syncCount, view } from './doc-editor.js';
import { createDocFind } from './doc-find.js';
import { el, sound } from '@dotany/web/shared.js';
import { cur, renameUntitled, printFile } from './files.js';
import './save-as.js';

app.kind = '';
app.closeSettings = () => shell.closeSettings();
app.openNew = () => shell.openNew();
app.printFile = printFile;
// the family's save card (shell.js) for Text's other parts
app.openSaveAs = (anchor, done) => shell.openSaveAs(anchor, done);
app.closeSaveAs = (quiet) => shell.closeSaveAs(quiet);
app.dlgOpen = () => shell.cardOpen;
app.saveAsBtn = saveAsBtn;
app.docFind = createDocFind({
  getView: () => view,
  el,
  ic: (n) => icon(LUCIDE[n]),
  sound,
  parent: appEl,
  before: chrome,
  T,
  onClose: () => app.focus(),
  pressed: (on_) => findBtn.setAttribute('aria-pressed', String(on_)),
});
shell.translates(app.docFind.box);

// ---------- the words: the family's turn them (shell.js); what Text draws itself, it draws again ----------
let codeTranslated = false;
// the code view arrives when it's first needed, and its parts are translated from then on
app.applyLang = () => {
  if (app.code && !codeTranslated) {
    codeTranslated = true;
    shell.translates(...app.code.translatable);
  }
  shell.applyLang();
};
shell.onLang(() => {
  if (motion) {
    shell.translate(motion.bub);
    shell.translate(motion.tbl);
  }
  onBars((m) => {
    shell.translate(m.bub);
    shell.translate(m.tbl);
  });
  view.dom.setAttribute('aria-label', T('正文编辑区'));
  syncCount();
  resetBar();
  renameUntitled();
  if (app.docFind.isOpen()) app.docFind.sync();
  if (app.code) app.code.refresh();
  closePop();
});

// ---------- settings: the family's, with Text's own page for saving ----------
shell.settingsPage('saving', '保存', 'language');
shell.settingRow('saving', {
  title: '实时自动保存',
  note: '开启后，停止输入片刻即自动保存至本地；关闭后，需使用 {Mod+S} 手动保存。',
  get: () => app.autoSave,
  set: (on_) => {
    store.set('dotany-autosave', on_ ? 'on' : null);
    if (on_) app.save();
    app.syncSaveBtn();
  },
});
// a setting changed in another Text window, or in another program of the family (shared-settings, the shell's)
shell.onSettings(() => app.syncSaveBtn());
on('setting', ({ from, key, value }) => {
  if (from !== windowLabel && store.get(key) !== value) shell.tookSettings({ [key]: value });
});

new ResizeObserver(() => {
  if (motion) motion.relayout(FOLLOW);
  schedulePages();
}).observe(scroller);
if (document.fonts && document.fonts.ready)
  document.fonts.ready.then(() => {
    resetBar();
    schedulePages();
    if (motion) motion.relayout(FOLLOW);
  });

// ---------- start: the shell opens the first file, then waits for the fonts before the window shows ----------
let fontsMs = 0;
// the first words on screen need their fonts, so the window waits for them instead of showing a flash of something else
app.beforeShow = async () => {
  const t = performance.now();
  await Promise.all([
    document.fonts.load('400 14px "Stack Sans Text"'),
    document.fonts.load('400 14px "HarmonyOS Sans SC"', '中'),
  ]);
  fontsMs = performance.now() - t;
};
app.startup = () => ({
  fonts_ms: fontsMs,
  visibility: document.visibilityState,
  file: cur.name || null,
  kind: cur.kind || null,
  editable: cur.kind
    ? !!(
        document.activeElement
        && (document.activeElement.isContentEditable || document.activeElement.classList.contains('ls'))
      )
    : false,
});
shell.begin();
