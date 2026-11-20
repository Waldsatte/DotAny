// Files from outside: the drop sheet, and what a drop does.
// cfg read here:
// - drop(paths): every dropped path goes here, and the tool decides (shell.openRequest(path) is the family's open).
// - dropLabel(): what the drop sheet says now (Release to add).
import * as be from '../backend.js';
import { el } from '../shared.js';
import { T } from '../i18n.js';

/** Takes from ctx: cfg, openRequest. Gives dropView. */
export function fromOutside(ctx) {
  const { cfg, openRequest } = ctx;
  // ---------- files from outside ----------
  const dropView = el('div', 'drop-view'),
    dropText = el('p', 'drop-text', '松开以打开文件');
  dropView.appendChild(dropText);
  document.body.appendChild(dropView);
  if (cfg.dropLabel) dropText.classList.add('dyn');
  // the words are set as it comes: what a drop does can depend on what's open
  be.onWebview('tauri://drag-enter', () => {
    if (cfg.dropLabel) dropText.textContent = T(cfg.dropLabel());
    dropView.classList.add('on');
  });
  be.onWebview('tauri://drag-leave', () => dropView.classList.remove('on'));
  be.onWebview('tauri://drag-drop', (p) => {
    dropView.classList.remove('on');
    const paths = (p && p.paths) || [];
    if (!paths.length) return;
    if (cfg.drop) cfg.drop(paths);
    else openRequest(paths[0]);
  });
  Object.assign(ctx, { dropView });
}
