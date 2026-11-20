// Slides as pictures (PNG): each slide is drawn by the page's own painter (paint.js), then the page draws that onto a canvas through an
// SVG picture of it, at the width asked for. Nothing leaves the page but the finished PNG, and nothing flashes on screen.
// The Mac doesn't come here: WebKit draws such an SVG picture wrong (src-tauri/src/mac.rs says how), so there the slides
// are printed as for a PDF and each sheet is drawn by the system (main.js printPdf, mac.rs pdf_pictures).
import { paintSlide } from './paint.js';

// the sheet rules a slide needs, taken from the page's own sheet; what only helps while editing is left out
function slideRules() {
  const out = [];
  for (const sheet of document.styleSheets) {
    let rules;
    try {
      rules = sheet.cssRules;
    } catch {
      continue;
    }
    for (const r of rules)
      if (
        r.selectorText
        && /^\.slide\b/.test(r.selectorText)
        && !/contenteditable|::selection|\.x-top|\.x-hint/.test(r.selectorText)
      )
        out.push(r.cssText);
  }
  // an empty placeholder's hint and a film's play button are for the screen only
  out.push('.x-hint, .x-play, .x-line, .x-player { display: none !important; }');
  return out.join('\n');
}

const asData = (blob) =>
  new Promise((ok, no) => {
    const f = new FileReader();
    f.onload = () => ok(f.result);
    f.onerror = no;
    f.readAsDataURL(blob);
  });
// a picture inside an SVG picture can't reach the page's own addresses: each one goes in as its bytes
async function inline(slide) {
  const seen = new Map(),
    data = async (u) => {
      if (!seen.has(u))
        seen.set(
          u,
          fetch(u)
            .then((r) => r.blob())
            .then(asData)
            .catch(() => '')
        );
      return seen.get(u);
    };
  const jobs = [];
  for (const e of [slide, ...slide.querySelectorAll('*')]) {
    if (e.tagName === 'IMG' && /^blob:/.test(e.src))
      jobs.push(
        data(e.src).then((d) => {
          e.src = d;
        })
      );
    if (e.localName === 'image') {
      const h = e.getAttribute('href') || e.getAttribute('xlink:href');
      if (h && /^blob:/.test(h))
        jobs.push(
          data(h).then((d) => {
            e.setAttribute('href', d);
            e.removeAttribute('xlink:href');
          })
        );
    }
    const bg = e.style && e.style.backgroundImage;
    if (bg && /blob:/.test(bg)) {
      const u = /url\("?(blob:[^")]+)"?\)/.exec(bg)[1];
      jobs.push(
        data(u).then((d) => {
          e.style.backgroundImage = `url("${d}")`;
        })
      );
    }
  }
  await Promise.all(jobs);
}

/** One slide as a PNG `width` pixels wide. */
export async function slidePng(deck, i, width) {
  const slide = paintSlide(deck, i);
  await inline(slide);
  const W = Math.round(width),
    H = Math.round((width * deck.h) / deck.w);
  const html = new XMLSerializer().serializeToString(slide);
  const svg = `<svg xmlns="http://www.w3.org/2000/svg" width="${W}" height="${H}" viewBox="0 0 ${deck.w} ${deck.h}"><foreignObject x="0" y="0" width="${deck.w}" height="${deck.h}"><div xmlns="http://www.w3.org/1999/xhtml" style="margin:0;width:${deck.w}px;height:${deck.h}px"><style>${slideRules().replace(/</g, '\\3c ')}</style>${html}</div></foreignObject></svg>`;
  const img = new Image();
  img.decoding = 'sync';
  img.src = 'data:image/svg+xml;charset=utf-8,' + encodeURIComponent(svg);
  await img.decode();
  const c = document.createElement('canvas');
  c.width = W;
  c.height = H;
  const g = c.getContext('2d');
  g.fillStyle = '#fff';
  g.fillRect(0, 0, W, H);
  g.drawImage(img, 0, 0, W, H);
  const blob = await new Promise((ok) => c.toBlob(ok, 'image/png'));
  return new Uint8Array(await blob.arrayBuffer());
}

/** Where `count` pictures go: one where the save dialog said; several beside it, numbered (名字-01.png, 名字-02.png …). */
export function pngPaths(path, count) {
  const base = path.replace(/\.png$/i, ''),
    pad = String(count).length < 2 ? 2 : String(count).length;
  return Array.from({ length: count }, (_, n) =>
    count === 1 ? path : `${base}-${String(n + 1).padStart(pad, '0')}.png`
  );
}

/**
 * The chosen slides as PNGs, written to `pngPaths`. Answers the shell's save result for the first one, or the first
 * failure.
 */
export async function exportPng(deck, list, width, path, save) {
  const paths = pngPaths(path, list.length);
  let first = null;
  for (let n = 0; n < list.length; n++) {
    let bytes;
    try {
      bytes = await slidePng(deck, list[n], width);
    } catch (e) {
      return { status: 'failed', kind: 'picture', detail: String(e) };
    }
    const to = paths[n];
    const r = await save(to, bytes, true);
    if (r.status !== 'saved') return r;
    if (!first) first = r;
  }
  return first;
}
