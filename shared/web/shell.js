// The family's window around a tool's work: the one top bar (file name and its status dot, Settings, Save As, the
// window's buttons), the file menu, the settings panel, the save-as card, the close prompt, the notice row, the empty
// window and the drop sheet.
// A tool calls createShell once with what is its own, and draws its work inside `shell.stage`.
// Each part of it is a module in shell/, made in this order by createShell: the window, the file's state, side panels,
// save as, notices, leaving with unsaved work, opening, the file menu, files from outside, settings, the words, and
// begin. They share one context object (ctx): each part takes what the parts before it put there, and puts there what
// it makes; a part names what it takes at its top, and what it gives at its end.
// These ten load first and in this order: fonts.js starts on the fonts and shared.js on the page
// as they load. The parts import what they use themselves.
import './backend.js';
import './fonts.js';
import { root, LUCIDE, tool } from './shared.js';
import { addTables } from './i18n.js';
import './licences.js';
import './inkfall-input.js';
import './paths.js';
import './window-frame.js';
import './new-panel.js';
import './parts.js';
import { theWindow } from './shell/window.js';
import { fileState } from './shell/state.js';
import { sidePanels } from './shell/side.js';
import { saveAs } from './shell/save-as.js';
import { notices } from './shell/notices.js';
import { leaving } from './shell/leaving.js';
import { fileOpening } from './shell/opening.js';
import { fileMenu } from './shell/file-menu.js';
import { fromOutside } from './shell/drop.js';
import { settings } from './shell/settings.js';
import { language } from './shell/language.js';
import { makeShell } from './shell/begin.js';

export { morph } from './shell/morph.js';

/**
 * cfg is what is the tool's own, all but name, icons, notices and words optional. Read here, as the shell is made:
 * - name: 'DotAny Starter' (also the top bar's while nothing is open, state.js, and About's, settings.js)
 * - icons: the generated LUCIDE table; words: the tool's tables, { en, ja: () => import('./lang/ja.json'), … }
 * - formats: { key: { ext: '.mp3', label: 'MP3' } } for the save card (save-as.js)
 * - alwaysWork: the work area shows with nothing open, instead of the family's empty page (opening.js draws none).
 * Every other switch is read by the part that does it, and explained at the top of that part: shell/window.js (count,
 * focus), state.js (dirty, saveInPlace), side.js (sideMorph), save-as.js (the card's), notices.js, leaving.js (discard,
 * beforeGuard), opening.js (open, show, clear, newFile, keys, …), file-menu.js (print, fileItems), drop.js, settings.js
 * (soundNote, licences, notices) and begin.js (beforeShow, startup). shared/README.md lists them.
 */
export function createShell(cfg) {
  tool.name = cfg.name;
  Object.assign(LUCIDE, cfg.icons);
  if (cfg.words) addTables(cfg.words);
  const boot = window.__DOTANY__ || {};
  const t0 = performance.now();
  const FORMAT = cfg.formats || {};
  if (cfg.alwaysWork) root.classList.add('always-work');
  const ctx = { cfg, boot, t0, FORMAT };
  theWindow(ctx);
  fileState(ctx);
  sidePanels(ctx);
  saveAs(ctx);
  notices(ctx);
  leaving(ctx);
  fileOpening(ctx);
  fileMenu(ctx);
  fromOutside(ctx);
  settings(ctx);
  language(ctx);
  return makeShell(ctx);
}
