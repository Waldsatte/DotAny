// every icon is Lucide's own drawing (ISC licence): one 24px grid, one stroke, round ends
export const icon = (inner) => `<svg class="i" viewBox="0 0 24 24" aria-hidden="true">${inner}</svg>`;
export const p = (...ds) => ds.map((d) => `<path d="${d}"/>`).join('');
export const ICON = {
  bold: icon(p('M6 12h9a4 4 0 010 8H7a1 1 0 01-1-1V5a1 1 0 011-1h7a4 4 0 010 8')),
  italic: icon(
    '<line x1="19" x2="10" y1="4" y2="4"/><line x1="14" x2="5" y1="20" y2="20"/><line x1="15" x2="9" y1="4" y2="20"/>'
  ),
  underline: icon('<path d="M6 4v6a6 6 0 0 0 12 0V4"/><line x1="4" x2="20" y1="20" y2="20"/>'),
  color: icon('<path class="bar" d="M4 20h16"/>' + p('m6 16 6-12 6 12', 'M8 12h8')),
  left: icon(p('M21 5H3', 'M15 12H3', 'M17 19H3')),
  center: icon(p('M21 5H3', 'M17 12H7', 'M19 19H5')),
  right: icon(p('M21 5H3', 'M21 12H9', 'M21 19H7')),
  justify: icon(p('M3 5h18', 'M3 12h18', 'M3 19h18')),
  lineHeight: icon(p('M3 5h8', 'M3 12h8', 'M3 19h8', 'm15 8 3-3 3 3', 'm15 16 3 3 3-3')),
  list: icon(p('M8 5h13', 'M8 12h13', 'M8 19h13') + '<path class="dt" d="M3 5h.01M3 12h.01M3 19h.01"/>'),
  ordered: icon(
    p(
      'M11 5h10',
      'M11 12h10',
      'M11 19h10',
      'M4 4h1v5',
      'M4 9h2',
      'M6.5 20H3.4c0-1 2.6-1.925 2.6-3.5a1.5 1.5 0 0 0-2.6-1.02'
    )
  ),
  image: icon(
    '<rect width="18" height="18" x="3" y="3" rx="2" ry="2"/><circle cx="9" cy="9" r="2"/><path d="m21 15-3.086-3.086a2 2 0 0 0-2.828 0L6 21"/>'
  ),
  table: icon(
    '<path d="M12 3v18"/><rect width="18" height="18" x="3" y="3" rx="2"/><path d="M3 9h18"/><path d="M3 15h18"/>'
  ),
  pages: icon('<rect width="18" height="18" x="3" y="3" rx="2"/><path d="M3 12h18"/>'),
  sun: icon(
    '<circle cx="12" cy="12" r="4"/>'
      + p(
        'M12 2v2',
        'M12 20v2',
        'm4.93 4.93 1.41 1.41',
        'm17.66 17.66 1.41 1.41',
        'M2 12h2',
        'M20 12h2',
        'm6.34 17.66-1.41 1.41',
        'm19.07 4.93-1.41 1.41'
      )
  ),
  settings: icon(
    '<path d="M14 17H5"/><path d="M19 7h-9"/><circle cx="17" cy="17" r="3"/><circle cx="7" cy="7" r="3"/>'
  ),
  monitor: icon(
    '<rect width="20" height="14" x="2" y="3" rx="2"/><line x1="8" x2="16" y1="21" y2="21"/><line x1="12" x2="12" y1="17" y2="21"/>'
  ),
  saveAs: icon(
    p(
      'M4.226 20.925A2 2 0 0 0 6 22h12a2 2 0 0 0 2-2V8a2.4 2.4 0 0 0-.706-1.706l-3.588-3.588A2.4 2.4 0 0 0 14 2H6a2 2 0 0 0-2 2v3.127',
      'M14 2v5a1 1 0 0 0 1 1h5',
      'm5 11-3 3',
      'm5 17-3-3h10'
    )
  ),
  moon: icon(
    p('M20.985 12.486a9 9 0 1 1-9.473-9.472c.405-.022.617.46.402.803a6 6 0 0 0 8.268 8.268c.344-.215.825-.004.803.401')
  ),
};
import { LUCIDE } from './code-icons.js';
import { LUCIDE as FAMILY_ICONS, tool } from '@dotany/web/shared.js';

export { LUCIDE };
// the family's window buttons (shared/web/window-frame.js) draw with these, and the window's title says Text's name
tool.name = 'DotAny Text';
Object.assign(FAMILY_ICONS, {
  winMin: LUCIDE.minimize,
  winMax: LUCIDE.maximize,
  winRestore: LUCIDE.restore,
  winClose: LUCIDE.close,
});
