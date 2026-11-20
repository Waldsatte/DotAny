// DOS text art on screen: an .nfo or .diz as its characters in a DOS face (Unscii, 8 × 16, public domain), an .ans as
// the picture its colour codes draw (ansi.js). View-only: nothing here edits; Save As writes the characters as UTF-8.
import { parseAnsi, ansiPlain, runs, css, CGA } from './ansi.js';

export function createArtView({ el, parent, before }) {
  const host = el('div', 'arthost');
  host.tabIndex = -1;
  const sheet = el('pre', 'art');
  host.appendChild(sheet);
  parent.insertBefore(host, before);
  let plain = '';

  // the face first, so the characters don't arrive in another one and jump
  const face = () =>
    document.fonts ? document.fonts.load('16px "Unscii 16"', '█╔').catch(() => {}) : Promise.resolve();

  async function show(text, kind) {
    await face();
    sheet.textContent = '';
    sheet.classList.toggle('ans', kind === 'ans');
    // an .ans is drawn for DOS's screen: light grey on black, whatever the window's colours
    sheet.style.background = kind === 'ans' ? CGA[0] : '';
    sheet.style.color = kind === 'ans' ? CGA[7] : '';
    if (kind !== 'ans') {
      sheet.textContent = text;
      plain = text.endsWith('\n') ? text : text + '\n';
    } else {
      const screen = parseAnsi(text);
      const frag = document.createDocumentFragment();
      for (const r of screen.rows) {
        const line = el('div', 'art-row');
        for (const run of runs(r, screen.width)) {
          const s = el('span', '', run.text);
          if (run.fg !== 7) s.style.color = css(run.fg);
          if (run.bg !== 0) s.style.background = css(run.bg);
          line.appendChild(s);
        }
        if (!line.childNodes.length) line.textContent = ' ';
        frag.appendChild(line);
      }
      sheet.style.setProperty('--cols', screen.width);
      sheet.appendChild(frag);
      plain = ansiPlain(screen);
    }
    host.scrollTop = 0;
    host.scrollLeft = 0;
  }
  function clear() {
    sheet.textContent = '';
    plain = '';
  }
  return { host, show, clear, text: () => plain, focus: () => host.focus({ preventScroll: true }) };
}
