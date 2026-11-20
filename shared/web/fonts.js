// The family's fonts are installed once and shared by every DotAny program (shared/rust/src/fonts.rs says where they
// are found). They come from the program's own font address, which each system spells differently. When they can't be
// found the address answers nothing, and the font lists in family.css fall through to the system's fonts.
//
// HarmonyOS Sans SC (17 MB) is first asked of Windows: the installer makes the installed files the person's own Windows
// fonts (installer/src-tauri/src/reg.rs), and Windows keeps one copy for every program instead of each page loading
// its own. A program that isn't installed (a development build, a copied folder) gets the file from its font address.
//
// Windows' copy is taken only when it is the version DotAny ships. Every version has the same name, and a computer may
// already have another one (1.0 sets the full-width brackets （）《》「」【】 a whole character wide where 2.400 sets them
// half): before the window shows, each weight is loaded from Windows on its own and 「（」 measured. Half a character wide
// is DotAny's 2.400 and the page uses Windows' copy; anything else, or none, and it loads its own file. So the page looks
// the same either way.
import { fileSrc } from './backend.js';

const face = (family, file, weight, format = 'truetype', local = []) =>
  `@font-face { font-family: "${family}"; src: ${[...local.map((n) => `local("${n}")`), `url("${fileSrc(file, 'dotfont')}") format("${format}")`].join(', ')}; font-weight: ${weight}; font-display: block; }`;
const style = document.createElement('style');
style.textContent =
  face('Stack Sans Text', 'StackSansText-wght.ttf', '200 700')
  + face('Stack Sans Headline', 'StackSansHeadline-wght.ttf', '200 700')
  // one language's own; a face is only fetched when family.css's font list for <html lang> reaches it
  + face('HarmonyOS Sans TC', 'HarmonyOS_Sans_TC.ttf', '100 900')
  + face('Noto Sans JP DotAny', 'NotoSansJP-DotAny-Regular.ttf', 400)
  + face('Noto Sans JP DotAny', 'NotoSansJP-DotAny-Bold.ttf', 700)
  + face('Pretendard', 'Pretendard-Regular.otf', 400, 'opentype')
  + face('Pretendard', 'Pretendard-Bold.otf', 700, 'opentype');
document.head.appendChild(style);

const HARMONY = [
  { file: 'HarmonyOS_SansSC_Regular.ttf', weight: 400, local: ['HarmonyOS Sans SC', 'HarmonyOS_Sans_SC'] },
  { file: 'HarmonyOS_SansSC_Bold.ttf', weight: 700, local: ['HarmonyOS Sans SC Bold', 'HarmonyOS_Sans_SC_Bold'] },
];

// Is Windows' copy of this weight the one DotAny ships? Loaded by the same local() names the page would use, so it is
// the very file the page would get.
async function ours({ weight, local }) {
  if (typeof FontFace === 'undefined' || !document.fonts) return false;
  const probe = new FontFace(`DotAny probe ${weight}`, local.map((n) => `local("${n}")`).join(', '), {
    weight: String(weight),
  });
  try {
    await probe.load();
    document.fonts.add(probe);
    const c = document.createElement('canvas').getContext('2d');
    c.font = `${weight} 100px "${probe.family}"`;
    return Math.abs(c.measureText(String.fromCharCode(0xff08)).width - 50) < 1; // the full-width left bracket, measured, never shown
  } catch {
    return false; // not a Windows font here
  } finally {
    document.fonts.delete(probe);
  }
}

const atMost = (ms) => new Promise((done) => setTimeout(() => done(false), ms));

/** Settled once HarmonyOS Sans SC's faces are in: awaited before a window shows (shell/begin.js, the hub). Never fails. */
export const fontsChosen = Promise.all(
  HARMONY.map((h) => Promise.race([ours(h), atMost(1500)]).catch(() => false))
).then((windows) => {
  const more = document.createElement('style');
  more.textContent = HARMONY.map((h, i) =>
    face('HarmonyOS Sans SC', h.file, h.weight, 'truetype', windows[i] ? h.local : [])
  ).join('');
  document.head.appendChild(more);
});
