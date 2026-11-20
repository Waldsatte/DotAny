// drawing a slide as real elements: 1 unit is 1 point, and the page scales the whole thing. Text stays text, so it can be selected and found
import { kids, kid, num, str, unwrap, phOf } from './deck.js';
import { drawChart } from './chart.js';

const EMU = 12700,
  SVGNS = 'http://www.w3.org/2000/svg';
const RNS = 'http://schemas.openxmlformats.org/officeDocument/2006/relationships';
const rid = (n, a) => (n ? n.getAttributeNS(RNS, a) || n.getAttribute('r:' + a) : null);
const div = (c) => {
  const e = document.createElement('div');
  if (c) e.className = c;
  return e;
};
const svg = (t, a = {}) => {
  const e = document.createElementNS(SVGNS, t);
  for (const k in a) e.setAttribute(k, a[k]);
  return e;
};
let uid = 0;

// ---------- colour ----------
const PRESET = {
  black: '000000',
  white: 'FFFFFF',
  red: 'FF0000',
  green: '008000',
  blue: '0000FF',
  yellow: 'FFFF00',
  gray: '808080',
  grey: '808080',
  orange: 'FFA500',
  navy: '000080',
  silver: 'C0C0C0',
  purple: '800080',
};
const hex2 = (h) => [parseInt(h.slice(0, 2), 16), parseInt(h.slice(2, 4), 16), parseInt(h.slice(4, 6), 16)];
function toHsl([r, g, b]) {
  r /= 255;
  g /= 255;
  b /= 255;
  const a = Math.max(r, g, b),
    c = Math.min(r, g, b),
    l = (a + c) / 2;
  if (a === c) return [0, 0, l];
  const d = a - c,
    s = l > 0.5 ? d / (2 - a - c) : d / (a + c),
    h = a === r ? (g - b) / d + (g < b ? 6 : 0) : a === g ? (b - r) / d + 2 : (r - g) / d + 4;
  return [h / 6, s, l];
}
function toRgb([h, s, l]) {
  if (!s) {
    const v = l * 255;
    return [v, v, v];
  }
  const f = (p, q, t) => {
    t = (t + 1) % 1;
    return t < 1 / 6 ? p + (q - p) * 6 * t : t < 0.5 ? q : t < 2 / 3 ? p + (q - p) * (2 / 3 - t) * 6 : p;
  };
  const q = l < 0.5 ? l * (1 + s) : l + s - l * s,
    p = 2 * l - q;
  return [f(p, q, h + 1 / 3) * 255, f(p, q, h) * 255, f(p, q, h - 1 / 3) * 255];
}
const toLin = (c) => {
  c /= 255;
  return c <= 0.04045 ? c / 12.92 : Math.pow((c + 0.055) / 1.055, 2.4);
};
const fromLin = (c) => 255 * (c <= 0.0031308 ? c * 12.92 : 1.055 * Math.pow(c, 1 / 2.4) - 0.055);
const COLORS = new Set(['srgbClr', 'schemeClr', 'sysClr', 'prstClr', 'hslClr', 'scrgbClr']);
export function colorOf(parent, ctx, ph) {
  const n = kids(parent).find((c) => COLORS.has(c.localName));
  if (!n) return null;
  let rgb,
    a = 1;
  if (n.localName === 'srgbClr') rgb = hex2(str(n, 'val') || '000000');
  else if (n.localName === 'sysClr') rgb = hex2(str(n, 'lastClr') || '000000');
  else if (n.localName === 'prstClr') rgb = hex2(PRESET[str(n, 'val')] || '000000');
  else if (n.localName === 'hslClr')
    rgb = toRgb([num(n, 'hue', 0) / 21600000, num(n, 'sat', 0) / 100000, num(n, 'lum', 0) / 100000]);
  else if (n.localName === 'scrgbClr')
    rgb = [num(n, 'r', 0), num(n, 'g', 0), num(n, 'b', 0)].map((v) => (255 * v) / 100000);
  else {
    let v = str(n, 'val');
    if (v === 'phClr' && ph) rgb = ph.rgb;
    else {
      const map = ctx.clrMap;
      if (map[v]) v = map[v];
      rgb = hex2(ctx.theme.colors[v] || '000000');
    }
    if (v === 'phClr' && ph) a = ph.a;
  }
  for (const m of kids(n)) {
    const k = m.localName,
      val = num(m, 'val', 0) / 100000;
    if (k === 'alpha') a = val;
    else if (k === 'alphaMod') a *= val;
    else if (k === 'alphaOff') a += val;
    else if (k === 'lumMod' || k === 'lumOff' || k === 'satMod' || k === 'satOff' || k === 'hueMod' || k === 'hueOff') {
      const h = toHsl(rgb);
      if (k === 'lumMod') h[2] *= val;
      else if (k === 'lumOff') h[2] += val;
      else if (k === 'satMod') h[1] *= val;
      else if (k === 'satOff') h[1] += val;
      else if (k === 'hueMod') h[0] = (h[0] * val) % 1;
      else h[0] = (h[0] + num(m, 'val', 0) / 21600000) % 1;
      h[1] = Math.min(1, Math.max(0, h[1]));
      h[2] = Math.min(1, Math.max(0, h[2]));
      rgb = toRgb(h);
    } else if (k === 'tint') rgb = rgb.map((c) => fromLin(toLin(c) * val + (1 - val)));
    else if (k === 'shade') rgb = rgb.map((c) => fromLin(toLin(c) * val));
    else if (k === 'comp') {
      const h = toHsl(rgb);
      h[0] = (h[0] + 0.5) % 1;
      rgb = toRgb(h);
    } else if (k === 'inv') rgb = rgb.map((c) => 255 - c);
    else if (k === 'gray') {
      const g = rgb[0] * 0.3 + rgb[1] * 0.59 + rgb[2] * 0.11;
      rgb = [g, g, g];
    }
  }
  return { rgb, a };
}
export const css = (c) =>
  c
    ? `rgba(${c.rgb.map((v) => Math.round(Math.min(255, Math.max(0, v)))).join(',')},${+c.a.toFixed(3)})`
    : 'transparent';

// ---------- fills ----------
const FILLS = new Set(['solidFill', 'gradFill', 'blipFill', 'pattFill', 'noFill', 'grpFill']);
export function fillFrom(node, ctx, ph) {
  const f = kids(node).find((c) => FILLS.has(c.localName));
  if (!f) return null;
  return readFill(f, ctx, ph);
}
function readFill(f, ctx, ph) {
  const k = f.localName;
  if (k === 'noFill') return { none: true };
  if (k === 'grpFill') return ctx.groupFill || { none: true };
  if (k === 'solidFill') return { color: colorOf(f, ctx, ph) };
  if (k === 'pattFill') return { color: colorOf(kid(f, 'bgClr') || f, ctx, ph) };
  if (k === 'gradFill') {
    const stops = kids(kid(f, 'gsLst'), 'gs')
        .map((g) => ({ at: num(g, 'pos', 0) / 1000, color: colorOf(g, ctx, ph) }))
        .sort((a, b) => a.at - b.at),
      lin = kid(f, 'lin'),
      path = kid(f, 'path'),
      rect = path && kid(path, 'fillToRect'),
      fl = num(rect, 'l', 50000) / 1000,
      fr = num(rect, 'r', 50000) / 1000,
      ft = num(rect, 't', 50000) / 1000,
      fb = num(rect, 'b', 50000) / 1000;
    return {
      stops,
      angle: lin ? num(lin, 'ang', 0) / 60000 : 0,
      radial: !!path,
      fx: 100 - fr < fl ? fl : (fl + 100 - fr) / 2,
      fy: 100 - fb < ft ? ft : (ft + 100 - fb) / 2,
    };
  }
  if (k === 'blipFill') {
    const b = kid(f, 'blip');
    return {
      url: urlOf(ctx, rid(b, 'embed'), b),
      tile: !!kid(f, 'tile'),
      alpha: b && kid(b, 'alphaModFix') ? num(kid(b, 'alphaModFix'), 'amt', 100000) / 100000 : 1,
      crop: kid(f, 'srcRect'),
    };
  }
  return null;
}
const MEDIA_MIME = {
  mp4: 'video/mp4',
  m4v: 'video/mp4',
  mov: 'video/mp4',
  webm: 'video/webm',
  wmv: 'video/x-ms-wmv',
  avi: 'video/x-msvideo',
  mp3: 'audio/mpeg',
  wav: 'audio/wav',
  m4a: 'audio/mp4',
  aac: 'audio/aac',
  ogg: 'audio/ogg',
  flac: 'audio/flac',
  wma: 'audio/x-ms-wma',
};
const MIME = {
  png: 'image/png',
  jpg: 'image/jpeg',
  jpeg: 'image/jpeg',
  gif: 'image/gif',
  bmp: 'image/bmp',
  svg: 'image/svg+xml',
  webp: 'image/webp',
  tif: 'image/tiff',
  tiff: 'image/tiff',
};
function urlOf(ctx, id, node) {
  if (!id) return null;
  const r = (node ? ctx.deck.relsOfNode(node) : ctx.rels).get(id);
  if (!r || r.external) return null;
  const d = ctx.deck,
    have = d.urls.get(r.target);
  if (have !== undefined) return have;
  const f = d.files[r.target],
    ext = r.target.split('.').pop().toLowerCase(),
    mime = MIME[ext];
  if (!f || !mime) {
    d.urls.set(r.target, null);
    d.unshown = (d.unshown || 0) + 1;
    return null;
  }
  const u = URL.createObjectURL(new Blob([f], { type: mime }));
  d.urls.set(r.target, u);
  return u;
}
function cssFill(f) {
  if (!f || f.none) return 'transparent';
  if (f.color) return css(f.color);
  if (f.stops) {
    const s = f.stops.map((g) => `${css(g.color)} ${g.at}%`).join(',');
    return f.radial
      ? `radial-gradient(ellipse farthest-corner at ${f.fx}% ${f.fy}%,${s})`
      : `linear-gradient(${f.angle + 90}deg,${s})`;
  }
  return 'transparent';
}
function svgPaint(f, defs, box) {
  if (!f || f.none) return 'none';
  if (f.color) return css(f.color);
  if (f.stops) {
    const id = 'g' + ++uid;
    let g;
    if (f.radial) g = svg('radialGradient', { id, cx: '50%', cy: '50%', r: '70%' });
    else {
      const a = (f.angle * Math.PI) / 180,
        c = Math.cos(a),
        s = Math.sin(a);
      g = svg('linearGradient', { id, x1: 0.5 - c / 2, y1: 0.5 - s / 2, x2: 0.5 + c / 2, y2: 0.5 + s / 2 });
    }
    for (const st of f.stops) {
      const col = st.color || { rgb: [0, 0, 0], a: 1 };
      g.appendChild(
        svg('stop', {
          offset: st.at + '%',
          'stop-color': `rgb(${col.rgb.map(Math.round).join(',')})`,
          'stop-opacity': col.a,
        })
      );
    }
    defs.appendChild(g);
    return `url(#${id})`;
  }
  if (f.url) {
    const id = 'p' + ++uid,
      p = svg('pattern', { id, patternUnits: 'objectBoundingBox', width: 1, height: 1 }),
      im = svg('image', { href: f.url, width: box.w, height: box.h, preserveAspectRatio: 'none' });
    p.appendChild(im);
    defs.appendChild(p);
    return `url(#${id})`;
  }
  return 'none';
}

// ---------- shapes' outlines ----------
const adj = (geom, name, d) => {
  const list = kid(geom, 'avLst');
  if (list)
    for (const g of kids(list, 'gd'))
      if (str(g, 'name') === name) {
        const m = /val\s+(-?\d+)/.exec(str(g, 'fmla') || '');
        if (m) return +m[1];
      }
  return d;
};
function poly(pts) {
  return 'M' + pts.map((p) => p.join(' ')).join('L') + 'Z';
}
const ell = (w, h) => `M0 ${h / 2}A${w / 2} ${h / 2} 0 1 1 ${w} ${h / 2}A${w / 2} ${h / 2} 0 1 1 0 ${h / 2}Z`;
function round(w, h, r) {
  return `M${r} 0H${w - r}A${r} ${r} 0 0 1 ${w} ${r}V${h - r}A${r} ${r} 0 0 1 ${w - r} ${h}H${r}A${r} ${r} 0 0 1 0 ${h - r}V${r}A${r} ${r} 0 0 1 ${r} 0Z`;
}
function star(w, h, n, inner) {
  const pts = [];
  for (let i = 0; i < n * 2; i++) {
    const a = -Math.PI / 2 + (i * Math.PI) / n,
      r = i % 2 ? inner : 1;
    pts.push([w / 2 + (Math.cos(a) * r * w) / 2, h / 2 + (Math.sin(a) * r * h) / 2]);
  }
  return poly(pts);
}
export function prstPath(name, w, h, g) {
  const ss = Math.min(w, h),
    a = (n, d) => adj(g, n, d);
  switch (name) {
    case 'ellipse':
    case 'flowChartConnector':
      return ell(w, h);
    case 'roundRect':
    case 'flowChartAlternateProcess':
      return round(w, h, Math.min((ss * a('adj', 16667)) / 100000, ss / 2));
    case 'flowChartTerminator':
      return round(w, h, ss / 2);
    case 'round2SameRect': {
      const r = (ss * a('adj1', 16667)) / 100000,
        b = (ss * a('adj2', 0)) / 100000;
      return `M${r} 0H${w - r}A${r} ${r} 0 0 1 ${w} ${r}V${h - b}A${b} ${b} 0 0 1 ${w - b} ${h}H${b}A${b} ${b} 0 0 1 0 ${h - b}V${r}A${r} ${r} 0 0 1 ${r} 0Z`;
    }
    case 'round1Rect': {
      const r = (ss * a('adj', 16667)) / 100000;
      return `M0 0H${w - r}A${r} ${r} 0 0 1 ${w} ${r}V${h}H0Z`;
    }
    case 'snip1Rect': {
      const r = (ss * a('adj', 16667)) / 100000;
      return poly([
        [0, 0],
        [w - r, 0],
        [w, r],
        [w, h],
        [0, h],
      ]);
    }
    case 'snip2SameRect': {
      const r = (ss * a('adj1', 16667)) / 100000;
      return poly([
        [r, 0],
        [w - r, 0],
        [w, r],
        [w, h],
        [0, h],
        [0, r],
      ]);
    }
    case 'triangle':
    case 'flowChartExtract': {
      const x = (w * a('adj', 50000)) / 100000;
      return poly([
        [x, 0],
        [w, h],
        [0, h],
      ]);
    }
    case 'flowChartMerge':
      return poly([
        [0, 0],
        [w, 0],
        [w / 2, h],
      ]);
    case 'rtTriangle':
      return poly([
        [0, 0],
        [w, h],
        [0, h],
      ]);
    case 'diamond':
    case 'flowChartDecision':
      return poly([
        [w / 2, 0],
        [w, h / 2],
        [w / 2, h],
        [0, h / 2],
      ]);
    case 'parallelogram':
    case 'flowChartInputOutput': {
      const x = (ss * a('adj', 25000)) / 100000;
      return poly([
        [x, 0],
        [w, 0],
        [w - x, h],
        [0, h],
      ]);
    }
    case 'trapezoid': {
      const x = (ss * a('adj', 25000)) / 100000;
      return poly([
        [x, 0],
        [w - x, 0],
        [w, h],
        [0, h],
      ]);
    }
    case 'pentagon':
      return poly([
        [w / 2, 0],
        [w, h * 0.382],
        [w * 0.809, h],
        [w * 0.191, h],
        [0, h * 0.382],
      ]);
    case 'hexagon': {
      const x = (ss * a('adj', 25000)) / 100000;
      return poly([
        [x, 0],
        [w - x, 0],
        [w, h / 2],
        [w - x, h],
        [x, h],
        [0, h / 2],
      ]);
    }
    case 'octagon': {
      const x = (ss * a('adj', 29289)) / 100000;
      return poly([
        [x, 0],
        [w - x, 0],
        [w, x],
        [w, h - x],
        [w - x, h],
        [x, h],
        [0, h - x],
        [0, x],
      ]);
    }
    case 'homePlate':
    case 'flowChartOffpageConnector': {
      const x = (ss * a('adj', 50000)) / 100000;
      return poly([
        [0, 0],
        [w - x, 0],
        [w, h / 2],
        [w - x, h],
        [0, h],
      ]);
    }
    case 'chevron': {
      const x = (ss * a('adj', 50000)) / 100000;
      return poly([
        [0, 0],
        [w - x, 0],
        [w, h / 2],
        [w - x, h],
        [0, h],
        [x, h / 2],
      ]);
    }
    case 'rightArrow': {
      const t = (h * a('adj1', 50000)) / 200000,
        x = (ss * a('adj2', 50000)) / 100000;
      return poly([
        [0, h / 2 - t],
        [w - x, h / 2 - t],
        [w - x, 0],
        [w, h / 2],
        [w - x, h],
        [w - x, h / 2 + t],
        [0, h / 2 + t],
      ]);
    }
    case 'leftArrow': {
      const t = (h * a('adj1', 50000)) / 200000,
        x = (ss * a('adj2', 50000)) / 100000;
      return poly([
        [w, h / 2 - t],
        [x, h / 2 - t],
        [x, 0],
        [0, h / 2],
        [x, h],
        [x, h / 2 + t],
        [w, h / 2 + t],
      ]);
    }
    case 'upArrow': {
      const t = (w * a('adj1', 50000)) / 200000,
        y = (ss * a('adj2', 50000)) / 100000;
      return poly([
        [w / 2 - t, h],
        [w / 2 - t, y],
        [0, y],
        [w / 2, 0],
        [w, y],
        [w / 2 + t, y],
        [w / 2 + t, h],
      ]);
    }
    case 'downArrow': {
      const t = (w * a('adj1', 50000)) / 200000,
        y = (ss * a('adj2', 50000)) / 100000;
      return poly([
        [w / 2 - t, 0],
        [w / 2 - t, h - y],
        [0, h - y],
        [w / 2, h],
        [w, h - y],
        [w / 2 + t, h - y],
        [w / 2 + t, 0],
      ]);
    }
    case 'leftRightArrow': {
      const t = (h * a('adj1', 50000)) / 200000,
        x = (ss * a('adj2', 50000)) / 100000;
      return poly([
        [0, h / 2],
        [x, 0],
        [x, h / 2 - t],
        [w - x, h / 2 - t],
        [w - x, 0],
        [w, h / 2],
        [w - x, h],
        [w - x, h / 2 + t],
        [x, h / 2 + t],
        [x, h],
      ]);
    }
    case 'plus':
    case 'mathPlus':
    case 'flowChartSummingJunction': {
      const x = (ss * a('adj', 25000)) / 100000;
      return poly([
        [x, 0],
        [w - x, 0],
        [w - x, x],
        [w, x],
        [w, h - x],
        [w - x, h - x],
        [w - x, h],
        [x, h],
        [x, h - x],
        [0, h - x],
        [0, x],
        [x, x],
      ]);
    }
    case 'star4':
      return star(w, h, 4, a('adj', 12500) / 50000);
    case 'star5':
      return star(w, h, 5, 0.382);
    case 'star6':
      return star(w, h, 6, 0.5);
    case 'star8':
      return star(w, h, 8, a('adj', 37500) / 50000);
    case 'donut': {
      const t = (ss * a('adj', 25000)) / 100000;
      return `${ell(w, h)}M${t} ${h / 2}A${w / 2 - t} ${h / 2 - t} 0 1 0 ${w - t} ${h / 2}A${w / 2 - t} ${h / 2 - t} 0 1 0 ${t} ${h / 2}Z`;
    }
    case 'line':
    case 'straightConnector1':
      return `M0 0L${w} ${h}`;
    case 'bentConnector3': {
      const x = (w * a('adj1', 50000)) / 100000;
      return `M0 0H${x}V${h}H${w}`;
    }
    case 'bentConnector2':
      return `M0 0H${w}V${h}`;
    case 'curvedConnector3': {
      const x = (w * a('adj1', 50000)) / 100000;
      return `M0 0C${x} 0 ${x} ${h} ${w} ${h}`;
    }
    case 'wedgeRectCallout':
    case 'wedgeRoundRectCallout': {
      const tx = w / 2 + (w * a('adj1', -20833)) / 100000,
        ty = h / 2 + (h * a('adj2', 62500)) / 100000,
        r = name === 'wedgeRectCallout' ? 0 : ss * 0.1667;
      const s = tx < w / 2 ? w * 0.17 : w * 0.58;
      const body = r ? round(w, h, r) : `M0 0H${w}V${h}H0Z`;
      return `${body}M${s} ${h - 1}L${tx} ${ty}L${s + w * 0.25} ${h - 1}Z`;
    }
    case 'wedgeEllipseCallout': {
      const tx = w / 2 + (w * a('adj1', -20833)) / 100000,
        ty = h / 2 + (h * a('adj2', 62500)) / 100000;
      return `${ell(w, h)}M${w * 0.35} ${h * 0.9}L${tx} ${ty}L${w * 0.6} ${h * 0.92}Z`;
    }
    case 'heart':
      return `M${w / 2} ${h * 0.25}C${w * 0.2} ${-h * 0.15} ${-w * 0.25} ${h * 0.4} ${w / 2} ${h}C${w * 1.25} ${h * 0.4} ${w * 0.8} ${-h * 0.15} ${w / 2} ${h * 0.25}Z`;
    case 'cloud':
    case 'cloudCallout':
      return `M${w * 0.25} ${h * 0.85}C${w * 0.02} ${h * 0.85} ${-w * 0.02} ${h * 0.5} ${w * 0.18} ${h * 0.42}C${w * 0.12} ${h * 0.12} ${w * 0.42} ${h * 0.02} ${w * 0.52} ${h * 0.2}C${w * 0.7} ${-h * 0.04} ${w * 0.92} ${h * 0.12} ${w * 0.84} ${h * 0.38}C${w * 1.04} ${h * 0.46} ${w * 1.02} ${h * 0.85} ${w * 0.76} ${h * 0.85}Z`;
    case 'arc': {
      const s = ((a('adj1', 16200000) / 60000) * Math.PI) / 180,
        e = ((a('adj2', 0) / 60000) * Math.PI) / 180,
        P = (t) => [w / 2 + (Math.cos(t) * w) / 2, h / 2 + (Math.sin(t) * h) / 2],
        [x0, y0] = P(s),
        [x1, y1] = P(e),
        big = (e - s + Math.PI * 2) % (Math.PI * 2) > Math.PI ? 1 : 0;
      return `M${x0} ${y0}A${w / 2} ${h / 2} 0 ${big} 1 ${x1} ${y1}`;
    }
    case 'pie': {
      const s = ((a('adj1', 0) / 60000) * Math.PI) / 180,
        e = ((a('adj2', 16200000) / 60000) * Math.PI) / 180,
        P = (t) => [w / 2 + (Math.cos(t) * w) / 2, h / 2 + (Math.sin(t) * h) / 2],
        [x0, y0] = P(s),
        [x1, y1] = P(e),
        big = (e - s + Math.PI * 2) % (Math.PI * 2) > Math.PI ? 1 : 0;
      return `M${w / 2} ${h / 2}L${x0} ${y0}A${w / 2} ${h / 2} 0 ${big} 1 ${x1} ${y1}Z`;
    }
    case 'frame': {
      const t = (ss * a('adj1', 12500)) / 100000;
      return `M0 0H${w}V${h}H0ZM${t} ${t}V${h - t}H${w - t}V${t}Z`;
    }
    default:
      return `M0 0H${w}V${h}H0Z`;
  }
}
function customPath(geom, w, h) {
  const out = [];
  let px = 0,
    py = 0;
  for (const p of kids(kid(geom, 'pathLst'), 'path')) {
    const pw = num(p, 'w'),
      ph = num(p, 'h'),
      sx = pw ? w / pw : 1 / EMU,
      sy = ph ? h / ph : 1 / EMU;
    const pt = (n) => [num(n, 'x', 0) * sx, num(n, 'y', 0) * sy];
    for (const c of kids(p)) {
      const k = c.localName;
      if (k === 'moveTo') {
        const [x, y] = pt(kids(c)[0]);
        out.push(`M${x} ${y}`);
        px = x;
        py = y;
      } else if (k === 'lnTo') {
        const [x, y] = pt(kids(c)[0]);
        out.push(`L${x} ${y}`);
        px = x;
        py = y;
      } else if (k === 'cubicBezTo') {
        const q = kids(c).map(pt);
        out.push(`C${q.flat().join(' ')}`);
        [px, py] = q[2];
      } else if (k === 'quadBezTo') {
        const q = kids(c).map(pt);
        out.push(`Q${q.flat().join(' ')}`);
        [px, py] = q[1];
      } else if (k === 'arcTo') {
        const wr = num(c, 'wR', 0) * sx,
          hr = num(c, 'hR', 0) * sy,
          st = ((num(c, 'stAng', 0) / 60000) * Math.PI) / 180,
          sw = ((num(c, 'swAng', 0) / 60000) * Math.PI) / 180,
          cx = px - Math.cos(st) * wr,
          cy = py - Math.sin(st) * hr,
          en = st + sw,
          x = cx + Math.cos(en) * wr,
          y = cy + Math.sin(en) * hr;
        out.push(`A${wr} ${hr} 0 ${Math.abs(sw) > Math.PI ? 1 : 0} ${sw > 0 ? 1 : 0} ${x} ${y}`);
        px = x;
        py = y;
      } else if (k === 'close') out.push('Z');
    }
  }
  return out.join('');
}

// ---------- context ----------
function contextFor(deck, s, level) {
  const master = s.master,
    ctx = {
      deck,
      rels: s.rels,
      theme: (master && master.theme) || { colors: {} },
      clrMap: (master && master.clrMap) || {},
      slide: s,
      level,
    };
  const over =
    kid(s.xml, 'clrMapOvr', 'overrideClrMapping') || (s.layout && kid(s.layout.xml, 'clrMapOvr', 'overrideClrMapping'));
  if (over) {
    ctx.clrMap = {};
    for (const a of over.attributes) ctx.clrMap[a.name] = a.value;
  }
  return ctx;
}
const withRels = (ctx, rels) => ({ ...ctx, rels });

function background(deck, s, ctx) {
  const chain = [
    [s.xml, s.rels],
    s.layout && [s.layout.xml, s.layout.rels],
    s.master && [s.master.xml, s.master.rels],
  ].filter(Boolean);
  for (const [d, rels] of chain) {
    const bg = kid(d, 'cSld', 'bg');
    if (!bg) continue;
    const c = withRels(ctx, rels),
      pr = kid(bg, 'bgPr'),
      ref = kid(bg, 'bgRef');
    if (pr) return fillFrom(pr, c);
    if (ref) {
      const i = num(ref, 'idx', 0),
        col = colorOf(ref, c),
        list = ctx.theme.bgFills || [];
      if (i >= 1001 && list[i - 1001]) return readFill(list[i - 1001], c, col);
      return { color: col };
    }
  }
  return { color: { rgb: [255, 255, 255], a: 1 } };
}

// ---------- text ----------
const WING = {
  '§': '▪',
  'ü': '✓',
  'Ø': '➢',
  q: '❑',
  v: '❖',
  n: '■',
  l: '●',
  o: '□',
  p: '◆',
  u: '◆',
  w: '◆',
  'à': '→',
  'Ü': '➔',
  '×': '✕',
  'û': '✕',
  'Þ': '➔',
  'ª': '▪',
};
const ROMAN = (n) => {
  const m = [
    [1000, 'm'],
    [900, 'cm'],
    [500, 'd'],
    [400, 'cd'],
    [100, 'c'],
    [90, 'xc'],
    [50, 'l'],
    [40, 'xl'],
    [10, 'x'],
    [9, 'ix'],
    [5, 'v'],
    [4, 'iv'],
    [1, 'i'],
  ];
  let s = '';
  for (const [v, t] of m)
    while (n >= v) {
      s += t;
      n -= v;
    }
  return s;
};
function autoNum(type, n) {
  const circ = '①②③④⑤⑥⑦⑧⑨⑩⑪⑫⑬⑭⑮⑯⑰⑱⑲⑳',
    cn = '〇一二三四五六七八九十';
  const [body, tail] = /^(arabic|alphaLc|alphaUc|romanLc|romanUc|circleNum|ea1Chs|hebrew|thai)/.test(type)
    ? [
        type.match(/^(arabic|alphaLc|alphaUc|romanLc|romanUc|circleNum\w*?(?=Plain)|ea1Chs)/)
          ? type.match(/^(arabic|alphaLc|alphaUc|romanLc|romanUc|circleNum|ea1Chs)/)[1]
          : 'arabic',
        type,
      ]
    : ['arabic', type];
  const v =
    body === 'alphaLc'
      ? String.fromCharCode(96 + ((n - 1) % 26) + 1)
      : body === 'alphaUc'
        ? String.fromCharCode(64 + ((n - 1) % 26) + 1)
        : body === 'romanLc'
          ? ROMAN(n)
          : body === 'romanUc'
            ? ROMAN(n).toUpperCase()
            : body === 'circleNum'
              ? circ[n - 1] || String(n)
              : body === 'ea1Chs'
                ? n <= 10
                  ? cn[n]
                  : String(n)
                : String(n);
  if (/ParenBoth/.test(tail)) return `(${v})`;
  if (/ParenR/.test(tail)) return `${v})`;
  if (/Period/.test(tail)) return `${v}.`;
  if (/DbPeriod|Comma/.test(tail)) return `${v}、`;
  return v;
}
// from most specific to least: each thing looked up walks this list
function styleChain(ctx, shape, body, phInfo, lvl) {
  const L = 'lvl' + (lvl + 1) + 'pPr',
    out = [],
    add = (n) => {
      if (n) {
        const x = kid(n, L);
        if (x) out.push(x);
      }
    };
  add(body && kid(body, 'lstStyle'));
  if (phInfo) {
    for (const p of phInfo.chain) add(kid(kid(p, 'txBody'), 'lstStyle'));
  }
  out.near = out.length;
  const st = ctx.slide.master && ctx.slide.master.styles;
  if (phInfo && st && !ctx.inTable) {
    const t = phInfo.type,
      list =
        t === 'title' || t === 'ctrTitle'
          ? kid(st, 'titleStyle')
          : t === 'body'
              || t === 'subTitle'
              || t === 'obj'
              || t === 'tbl'
              || t === 'chart'
              || t === 'dgm'
              || t === 'media'
              || t === 'clipArt'
              || t === 'pic'
            ? kid(st, 'bodyStyle')
            : kid(st, 'otherStyle');
    add(list);
  }
  if (!phInfo || ctx.inTable) add(ctx.deck.defaultText);
  if (st && !phInfo && !ctx.inTable) add(kid(st, 'otherStyle'));
  return out;
}
const lookPara = (pPr, chain, a) => {
  if (pPr && pPr.hasAttribute(a)) return pPr.getAttribute(a);
  for (const c of chain) if (c.hasAttribute(a)) return c.getAttribute(a);
  return null;
};
const lookChild = (pPr, chain, names) => {
  for (const c of [pPr, ...chain]) {
    if (!c) continue;
    const hit = kids(c).find((x) => names.includes(x.localName));
    if (hit) return hit;
  }
  return null;
};
const defRs = (chain) => chain.map((c) => kid(c, 'defRPr')).filter(Boolean);
const lookRun = (list, a) => {
  for (const r of list) if (r && r.hasAttribute(a)) return r.getAttribute(a);
  return null;
};
const lookRunKid = (list, n) => {
  for (const r of list) {
    const k = r && kid(r, n);
    if (k) return k;
  }
  return null;
};
const COLORFILL = ['solidFill', 'gradFill', 'noFill'];
const lookFill = (list) => {
  for (const r of list) {
    if (!r) continue;
    const k = kids(r).find((x) => COLORFILL.includes(x.localName));
    if (k) return k;
  }
  return null;
};

function faceName(ctx, f, slot) {
  if (!f) return '';
  const n = str(f, 'typeface') || '';
  if (!n) return '';
  const t = ctx.theme;
  if (n === '+mj-lt') return t.major.latin || '';
  if (n === '+mn-lt') return t.minor.latin || '';
  if (n === '+mj-ea') return t.major.ea || '';
  if (n === '+mn-ea') return t.minor.ea || '';
  void slot;
  return n;
}
const SERIF =
  /Times|Georgia|Garamond|Cambria|Palatino|Book Antiqua|Century|Baskerville|宋|SimSun|STSong|明朝|Mincho|Song/i;
function fontStack(ctx, latin, ea) {
  const names = [latin, ea].filter(Boolean);
  names.forEach((n) => ctx.deck.fonts.add(n));
  const fallback = SERIF.test(latin || ea || '')
    ? '"Times New Roman","SimSun","Noto Serif SC",serif'
    : '"Segoe UI","Microsoft YaHei","Noto Sans SC",sans-serif';
  return names
    .map((n) => `"${n.replace(/"/g, '')}"`)
    .concat(fallback)
    .join(',');
}

function drawRun(ctx, runEl, pPr, chain, scale, phColor) {
  const rPr = kid(runEl, 'rPr'),
    list = [rPr, ...defRs(chain)],
    s = document.createElement('span');
  const sz = lookRun(list, 'sz'),
    size = (sz != null ? +sz / 100 : 18) * scale;
  s.style.fontSize = +size.toFixed(2) + 'px';
  const latin = faceName(ctx, lookRunKid(list, 'latin')),
    ea = faceName(ctx, lookRunKid(list, 'ea'));
  s.style.fontFamily = fontStack(ctx, latin, ea);
  const b = lookRun(list, 'b'),
    i = lookRun(list, 'i'),
    u = lookRun(list, 'u'),
    st = lookRun(list, 'strike'),
    base = lookRun(list, 'baseline'),
    cap = lookRun(list, 'cap'),
    spc = lookRun(list, 'spc');
  if (b === '1' || b === 'true') s.style.fontWeight = 700;
  if (i === '1' || i === 'true') s.style.fontStyle = 'italic';
  const deco = [];
  if (u && u !== 'none') deco.push('underline');
  if (st && st !== 'noStrike') deco.push('line-through');
  if (deco.length) s.style.textDecoration = deco.join(' ');
  if (base && +base !== 0) {
    s.style.verticalAlign = +base > 0 ? 'super' : 'sub';
    s.style.fontSize = +(size * 0.66).toFixed(2) + 'px';
    s.style.lineHeight = 0;
  }
  if (cap === 'all') s.style.textTransform = 'uppercase';
  else if (cap === 'small') s.style.fontVariant = 'small-caps';
  if (spc) s.style.letterSpacing = +spc / 100 + 'px';
  const nearList = [rPr, ...defRs(chain.slice(0, chain.near || 0))],
    fill = lookFill(phColor ? nearList : list),
    col = fill
      ? fill.localName === 'solidFill'
        ? colorOf(fill, ctx)
        : fill.localName === 'gradFill'
          ? colorOf(kid(kid(fill, 'gsLst'), 'gs'), ctx)
          : null
      : null;
  const link = rPr && kid(rPr, 'hlinkClick');
  if (col) s.style.color = css(col);
  else if (fill && fill.localName === 'noFill') s.style.color = 'transparent';
  else if (phColor) s.style.color = css(phColor);
  if (link) {
    const r = ctx.rels.get(rid(link, 'id'));
    if (r && r.external) {
      s.dataset.link = r.target;
      s.style.textDecoration = 'underline';
      s.style.color = css({ rgb: hex2(ctx.theme.colors.hlink || '0563C1'), a: 1 });
      s.classList.add('x-link');
    }
  }
  return { el: s, size };
}
function fieldText(ctx, f) {
  const t = str(f, 'type') || '';
  if (/^slidenum/.test(t)) return String(ctx.deck.slides.indexOf(ctx.slide) + 1);
  const given = kid(f, 't');
  return given ? given.textContent : '';
}

function drawBody(host, shape, body, ctx, phInfo, phColor, box) {
  const props = [
    kid(body, 'bodyPr'),
    ...(phInfo ? phInfo.chain.map((p) => kid(kid(p, 'txBody'), 'bodyPr')) : []),
  ].filter(Boolean);
  if (!ctx.inTable && phInfo && ctx.slide.master) {
    const mp = ctx.slide.master.placeholders.find(
      (p) => p.type === (phInfo.type === 'ctrTitle' ? 'title' : phInfo.type === 'subTitle' ? 'body' : phInfo.type)
    );
    if (mp) {
      const b = kid(kid(mp.node, 'txBody'), 'bodyPr');
      if (b && !props.includes(b)) props.push(b);
    }
  }
  const pa = (a) => {
    for (const p of props) if (p.hasAttribute(a)) return p.getAttribute(a);
    return null;
  };
  const pk = (n) => {
    for (const p of props) {
      const k = kid(p, n);
      if (k) return k;
    }
    return null;
  };
  const ins = (a, d) => {
    const v = pa(a);
    return (v == null ? d : +v) / EMU;
  };
  const t = div('x-t');
  t._body = body;
  t._shape = shape;
  const l = ins('lIns', 91440),
    r = ins('rIns', 91440),
    top = ins('tIns', 45720),
    bot = ins('bIns', 45720);
  t.style.padding = `${top}px ${r}px ${bot}px ${l}px`;
  const anchor = pa('anchor') || 't';
  t.style.justifyContent = anchor === 'ctr' ? 'center' : anchor === 'b' ? 'flex-end' : 'flex-start';
  const wrap = pa('wrap');
  if (wrap === 'none') {
    t.style.whiteSpace = 'nowrap';
  }
  const vert = pa('vert');
  if (vert && vert !== 'horz') {
    t.style.writingMode = 'vertical-rl';
    t.style.textOrientation = /eaVert|mongolianVert|wordArtVert/.test(vert) ? 'mixed' : 'sideways';
    if (vert === 'vert270') t.style.transform = 'rotate(180deg)';
    t.style.justifyContent = anchor === 'ctr' ? 'center' : anchor === 'b' ? 'flex-start' : 'flex-end';
  }
  const auto = pk('normAutofit'),
    scale = auto ? num(auto, 'fontScale', 100000) / 100000 : 1,
    reduce = auto ? num(auto, 'lnSpcReduction', 0) / 100000 : 0;
  const counters = [];
  let index = 0;
  for (const p of kids(body, 'p')) {
    const pPr = kid(p, 'pPr'),
      lvl = num(pPr, 'lvl', 0),
      chain = styleChain(ctx, shape, body, phInfo, lvl),
      para = document.createElement('p');
    para.dataset.p = index++;
    const algn = lookPara(pPr, chain, 'algn');
    para.style.textAlign =
      algn === 'ctr' ? 'center' : algn === 'r' ? 'right' : algn === 'just' || algn === 'dist' ? 'justify' : 'left';
    const marL = +(lookPara(pPr, chain, 'marL') || 0) / EMU,
      indent = +(lookPara(pPr, chain, 'indent') || 0) / EMU;
    para.style.paddingLeft = Math.max(0, marL) + 'px';
    const marR = +(lookPara(pPr, chain, 'marR') || 0) / EMU;
    if (marR) para.style.paddingRight = marR + 'px';
    const ls = lookChild(pPr, chain, ['lnSpc']),
      sb = lookChild(pPr, chain, ['spcBef']),
      sa = lookChild(pPr, chain, ['spcAft']);
    let factor = 1,
      exact = null;
    if (ls) {
      const pct = kid(ls, 'spcPct'),
        pts = kid(ls, 'spcPts');
      if (pct) factor = num(pct, 'val', 100000) / 100000;
      else if (pts) exact = num(pts, 'val', 0) / 100;
    }
    factor = Math.max(0.5, factor - reduce);
    const first = kids(p).find((c) => c.localName === 'r' || c.localName === 'fld'),
      endR = kid(p, 'endParaRPr'),
      firstSize =
        (((first && num(kid(first, 'rPr'), 'sz')) || (endR && num(endR, 'sz')) || lookRun(defRs(chain), 'sz') || 1800)
          / 100)
        * scale;
    const spacing = (n) => {
      if (!n) return 0;
      const pct = kid(n, 'spcPct'),
        pts = kid(n, 'spcPts');
      return pct ? (num(pct, 'val', 0) / 100000) * firstSize * 1.2 : pts ? num(pts, 'val', 0) / 100 : 0;
    };
    para.style.fontSize = +firstSize.toFixed(2) + 'px';
    para.style.marginTop = (index > 1 ? spacing(sb) : 0) + 'px';
    para.style.marginBottom = spacing(sa) + 'px';
    para.style.lineHeight = exact != null ? exact + 'px' : String(+(1.2 * factor).toFixed(3));
    if (exact == null && factor < 1) para.style.top = (-firstSize * 1.2 * (1 - factor)) / 2 + 'px';
    // the bullet
    const none = lookChild(pPr, chain, ['buNone', 'buChar', 'buAutoNum', 'buBlip']),
      hasText = kids(p).some((c) => c.localName === 'r' || c.localName === 'fld');
    if (none && none.localName === 'buBlip' && hasText) {
      const b = kid(none, 'blip'),
        u = urlOf(ctx, rid(b, 'embed'), b);
      if (u) {
        const szp = lookChild(pPr, chain, ['buSzPct']),
          z = firstSize * (szp ? num(szp, 'val', 100000) / 100000 : 1),
          im = document.createElement('img');
        im.className = 'x-bu';
        im.src = u;
        im.alt = '';
        im.style.width = z * 0.8 + 'px';
        im.style.height = z * 0.8 + 'px';
        im.style.left = Math.max(0, marL + indent) + 'px';
        im.style.top = firstSize * 0.25 + 'px';
        para.appendChild(im);
      }
    }
    if (none && none.localName !== 'buNone' && none.localName !== 'buBlip' && hasText) {
      const bu = document.createElement('span');
      bu.className = 'x-bu';
      bu.setAttribute('aria-hidden', 'true');
      let ch = '•';
      if (none.localName === 'buChar') {
        ch = str(none, 'char') || '•';
        const fnt = lookChild(pPr, chain, ['buFont']),
          fname = fnt ? str(fnt, 'typeface') || '' : '';
        const code = ch.charCodeAt(0);
        if (code >= 0xf000 && code <= 0xf0ff) ch = String.fromCharCode(code - 0xf000);
        if (/Wingdings|Sorts|Webdings/i.test(fname)) ch = WING[ch] || '▪';
        else if (/Symbol/i.test(fname)) ch = ch === '·' ? '•' : ch;
        else if (fname && !fname.startsWith('+')) bu.style.fontFamily = `"${fname}",sans-serif`;
      } else {
        const start = num(none, 'startAt', 1);
        while (counters.length <= lvl) counters.push(null);
        counters.length = lvl + 1;
        const key = str(none, 'type') + start;
        const was = counters[lvl];
        const n = was && was.key === key ? was.n + 1 : start;
        counters[lvl] = { key, n };
        ch = autoNum(str(none, 'type') || 'arabicPeriod', n);
      }
      const szp = lookChild(pPr, chain, ['buSzPct']),
        szpts = lookChild(pPr, chain, ['buSzPts']);
      const sizeB = szpts ? num(szpts, 'val', 0) / 100 : firstSize * (szp ? num(szp, 'val', 100000) / 100000 : 1);
      bu.style.fontSize = sizeB + 'px';
      const clr = lookChild(pPr, chain, ['buClr']),
        firstR = first && kid(first, 'rPr'),
        runFill = firstR && kid(firstR, 'solidFill');
      const colr = clr ? colorOf(clr, ctx) : runFill ? colorOf(runFill, ctx) : null;
      if (colr) bu.style.color = css(colr);
      else if (phColor) bu.style.color = css(phColor);
      if (!bu.style.fontFamily && first) {
        const rl = [kid(first, 'rPr'), ...defRs(chain)];
        bu.style.fontFamily = fontStack(
          ctx,
          faceName(ctx, lookRunKid(rl, 'latin')),
          faceName(ctx, lookRunKid(rl, 'ea'))
        );
      }
      bu.style.left = Math.max(0, marL + indent) + 'px';
      bu.textContent = ch;
      para.appendChild(bu);
      if (indent >= 0) para.style.textIndent = indent + sizeB + 'px';
    } else {
      if (none && none.localName !== 'buNone') {
        /* no text */
      }
      if (indent && hasText) para.style.textIndent = indent + 'px';
    }
    if (!none || none.localName === 'buNone') {
      if (!hasText) counters.length = 0;
    }
    let any = false;
    for (const c of kids(p)) {
      if (c.localName === 'r' || c.localName === 'fld') {
        const { el } = drawRun(ctx, c, pPr, chain, scale, phColor);
        el.textContent = c.localName === 'fld' ? fieldText(ctx, c) : kid(c, 't') ? kid(c, 't').textContent : '';
        para.appendChild(el);
        any = true;
      } else if (c.localName === 'br') {
        const br = document.createElement('br'),
          rp = kid(c, 'rPr'),
          z = rp && num(rp, 'sz');
        if (z) br.style.fontSize = (z / 100) * scale + 'px';
        para.appendChild(br);
        any = true;
      }
    }
    if (!any) {
      const e = document.createElement('br');
      para.style.fontSize = firstSize + 'px';
      para.appendChild(e);
    }
    t.appendChild(para);
  }
  if (pk('spAutoFit')) t.classList.add('x-grow');
  host.appendChild(t);
}

// ---------- one element ----------
function findPh(ctx, node) {
  const ph = phOf(node);
  if (!ph) return null;
  const type = str(ph, 'type') || 'body',
    idx = str(ph, 'idx'),
    s = ctx.slide,
    chain = [];
  const near = (list) =>
    (idx != null ? list.find((p) => p.idx === idx) : null)
    || list.find((p) => p.type === type && (idx == null || p.idx == null))
    || list.find((p) => p.type === type);
  if (ctx.level === 'slide' && s.layout) {
    const a = near(s.layout.placeholders);
    if (a) chain.push(a.node);
  }
  if (ctx.level !== 'master' && s.master) {
    const t2 = type === 'ctrTitle' ? 'title' : type === 'subTitle' ? 'body' : type;
    const fromLayout = chain[0] && phOf(chain[0]),
      lt = fromLayout ? str(fromLayout, 'type') || 'body' : t2,
      t3 = lt === 'ctrTitle' ? 'title' : lt === 'subTitle' ? 'body' : lt,
      b = s.master.placeholders.find((p) => p.type === t3) || s.master.placeholders.find((p) => p.type === t2);
    if (b) chain.push(b.node);
  }
  return { type, chain };
}
function xfrmOf(node, phInfo) {
  const pr = kid(node, 'spPr') || kid(node, 'grpSpPr');
  let x = pr && kid(pr, 'xfrm');
  if (!x && node.localName === 'graphicFrame') x = kid(node, 'xfrm');
  if (!x && phInfo)
    for (const p of phInfo.chain) {
      const q = kid(kid(p, 'spPr'), 'xfrm');
      if (q) {
        x = q;
        break;
      }
    }
  return x;
}
function boxOf(x, map) {
  const off = kid(x, 'off'),
    ext = kid(x, 'ext');
  return {
    x: ((num(off, 'x', 0) - map.ox) * map.sx) / EMU,
    y: ((num(off, 'y', 0) - map.oy) * map.sy) / EMU,
    w: (num(ext, 'cx', 0) * map.sx) / EMU,
    h: (num(ext, 'cy', 0) * map.sy) / EMU,
    rot: num(x, 'rot', 0) / 60000,
    flipH: str(x, 'flipH') === '1',
    flipV: str(x, 'flipV') === '1',
  };
}
function placeBox(e, b) {
  e.style.left = b.x + 'px';
  e.style.top = b.y + 'px';
  e.style.width = b.w + 'px';
  e.style.height = b.h + 'px';
  const tr = [];
  if (b.rot) tr.push(`rotate(${b.rot}deg)`);
  if (b.flipH) tr.push('scaleX(-1)');
  if (b.flipV) tr.push('scaleY(-1)');
  if (tr.length) e.style.transform = tr.join(' ');
}
function shadowOf(pr, ctx, style) {
  const eff = kid(pr, 'effectLst');
  const s = eff && kid(eff, 'outerShdw');
  void style;
  if (!s) return '';
  const dist = num(s, 'dist', 0) / EMU,
    dir = ((num(s, 'dir', 0) / 60000) * Math.PI) / 180,
    blur = num(s, 'blurRad', 0) / EMU,
    c = colorOf(s, ctx);
  return `drop-shadow(${(Math.cos(dir) * dist).toFixed(2)}px ${(Math.sin(dir) * dist).toFixed(2)}px ${(blur / 2).toFixed(2)}px ${css(c)})`;
}
const DASH = {
  dash: [4, 3],
  sysDash: [3, 1],
  dot: [1, 3],
  sysDot: [1, 1],
  lgDash: [8, 3],
  dashDot: [4, 3, 1, 3],
  lgDashDot: [8, 3, 1, 3],
  lgDashDotDot: [8, 3, 1, 3, 1, 3],
  sysDashDot: [3, 1, 1, 1],
  sysDashDotDot: [3, 1, 1, 1, 1, 1],
};

function lineOf(pr, ctx, style) {
  let ln = pr && kid(pr, 'ln'),
    refC = null,
    base = null;
  const ref = style && kid(style, 'lnRef');
  if (ref && num(ref, 'idx', 0) > 0) {
    refC = colorOf(ref, ctx);
    base = (ctx.theme.lines || [])[num(ref, 'idx', 0) - 1] || null;
  }
  if (!ln && !base) return null;
  const pick = (name) => (ln && kid(ln, name)) || (base && kid(base, name));
  if (ln && kid(ln, 'noFill')) return null;
  const solid = ln && kid(ln, 'solidFill'),
    gr = ln && kid(ln, 'gradFill');
  let color = null;
  if (solid) color = colorOf(solid, ctx);
  else if (gr) color = colorOf(kid(kid(gr, 'gsLst'), 'gs'), ctx);
  else if (base) {
    const bs = kid(base, 'solidFill');
    color = bs ? colorOf(bs, ctx, refC) : refC;
    if (kid(base, 'noFill')) return null;
  }
  if (!color) return null;
  const w = (num(ln, 'w') != null ? num(ln, 'w') : num(base, 'w', 9525)) / EMU,
    dash = pick('prstDash'),
    head = ln && kid(ln, 'headEnd'),
    tail = ln && kid(ln, 'tailEnd');
  return {
    color,
    w,
    dash: dash ? DASH[str(dash, 'val')] : null,
    cap: (ln && str(ln, 'cap')) === 'rnd' ? 'round' : (ln && str(ln, 'cap')) === 'sq' ? 'square' : 'butt',
    head: head && str(head, 'type') !== 'none' ? str(head, 'type') : null,
    tail: tail && str(tail, 'type') !== 'none' ? str(tail, 'type') : null,
  };
}
function drawGeometry(host, pathD, fill, line, box) {
  if ((!fill || fill.none) && !line) return;
  const s = svg('svg', { class: 'x-g', width: Math.max(box.w, 1), height: Math.max(box.h, 1) }),
    defs = svg('defs');
  s.appendChild(defs);
  const p = svg('path', { d: pathD, fill: svgPaint(fill, defs, box) });
  if (fill && fill.color === null) p.setAttribute('fill', 'none');
  if (line) {
    p.setAttribute('stroke', css(line.color));
    p.setAttribute('stroke-width', line.w);
    p.setAttribute('stroke-linecap', line.cap);
    p.setAttribute('stroke-linejoin', 'round');
    if (line.dash) p.setAttribute('stroke-dasharray', line.dash.map((x) => x * line.w).join(' '));
    const marker = (kind, start) => {
      const id = 'm' + ++uid,
        z = Math.max(3, 2.5),
        m = svg('marker', {
          id,
          viewBox: '0 0 10 10',
          refX: kind === 'oval' ? 5 : 9,
          refY: 5,
          markerWidth: z * 1.2,
          markerHeight: z * 1.2,
          orient: start ? 'auto-start-reverse' : 'auto',
          markerUnits: 'strokeWidth',
        });
      m.appendChild(
        kind === 'oval'
          ? svg('circle', { cx: 5, cy: 5, r: 4, fill: css(line.color) })
          : kind === 'arrow'
            ? svg('path', { d: 'M1 1L9 5L1 9', fill: 'none', stroke: css(line.color), 'stroke-width': 1.5 })
            : svg('path', {
                d:
                  kind === 'stealth'
                    ? 'M0 0L10 5L0 10L3 5Z'
                    : kind === 'diamond'
                      ? 'M5 0L10 5L5 10L0 5Z'
                      : 'M0 0L10 5L0 10Z',
                fill: css(line.color),
              })
      );
      defs.appendChild(m);
      return `url(#${id})`;
    };
    if (line.head) p.setAttribute('marker-start', marker(line.head, true));
    if (line.tail) p.setAttribute('marker-end', marker(line.tail, false));
  }
  s.appendChild(p);
  host.appendChild(s);
}
function pathOf(pr, phInfo, box) {
  let g = pr && (kid(pr, 'prstGeom') || kid(pr, 'custGeom'));
  if (!g && phInfo)
    for (const p of phInfo.chain) {
      const q = kid(p, 'spPr'),
        h = q && (kid(q, 'prstGeom') || kid(q, 'custGeom'));
      if (h) {
        g = h;
        break;
      }
    }
  if (!g) return `M0 0H${box.w}V${box.h}H0Z`;
  return g.localName === 'custGeom' ? customPath(g, box.w, box.h) : prstPath(str(g, 'prst'), box.w, box.h, g);
}

function drawSp(node, parent, ctx, map) {
  const phInfo = findPh(ctx, node),
    x = xfrmOf(node, phInfo);
  if (!x) return;
  const box = boxOf(x, map),
    pr = kid(node, 'spPr');
  let style = kid(node, 'style');
  if (!style && phInfo)
    for (const q of phInfo.chain) {
      style = kid(q, 'style');
      if (style) break;
    }
  const e = div('x-s');
  placeBox(e, box);
  const nvId = str(
    kid(
      kids(node).find((c) => /^nv/.test(c.localName)),
      'cNvPr'
    ),
    'id'
  );
  if (nvId) e.dataset.sp = nvId;
  e._node = node;
  e._box = box;
  let c = ctx;
  let phFill = null;
  if (phInfo)
    for (const p of phInfo.chain) {
      phFill = fillFrom(kid(p, 'spPr'), ctx);
      if (phFill) break;
    }
  let fill = pr ? fillFrom(pr, c) : null;
  if (!fill) fill = phFill;
  if (!fill && style) {
    const ref = kid(style, 'fillRef'),
      i = num(ref, 'idx', 0),
      col = colorOf(ref, c),
      list = c.theme.fills || [];
    if (i > 0 && list[i - 1]) {
      const f = readFill(list[i - 1], c, col);
      fill = f && f.stops ? { ...f } : f || { color: col };
    }
  }
  let line = lineOf(pr, c, style);
  if (!line && phInfo && !(pr && kid(pr, 'ln')))
    for (const p of phInfo.chain) {
      line = lineOf(kid(p, 'spPr'), c, kid(p, 'style'));
      if (line) break;
    }
  drawGeometry(e, pathOf(pr, phInfo, box), fill, line, box);
  const sh = pr ? shadowOf(pr, c, style) : '';
  if (sh) e.style.filter = sh;
  const body = kid(node, 'txBody');
  if (
    body
    && (!phInfo
      || kids(body, 'p').some((p) =>
        kids(p).some((r) => r.localName === 'r' || r.localName === 'fld' || r.localName === 'br')
      ))
  ) {
    const fr = style && kid(style, 'fontRef'),
      phColor = fr ? colorOf(fr, c) : null;
    const host = div('x-tb');
    if (box.flipH || box.flipV)
      host.style.transform = `${box.flipH ? 'scaleX(-1)' : ''} ${box.flipV ? 'rotate(180deg) scaleX(-1)' : ''}`;
    const tx = kid(node, 'txXfrm');
    if (tx) {
      const b2 = boxOf(tx, map),
        a = (-box.rot * Math.PI) / 180,
        dx = b2.x + b2.w / 2 - (box.x + box.w / 2),
        dy = b2.y + b2.h / 2 - (box.y + box.h / 2),
        lx = dx * Math.cos(a) - dy * Math.sin(a),
        ly = dx * Math.sin(a) + dy * Math.cos(a);
      host.style.inset = 'auto';
      host.style.width = b2.w + 'px';
      host.style.height = b2.h + 'px';
      host.style.left = box.w / 2 + lx - b2.w / 2 + 'px';
      host.style.top = box.h / 2 + ly - b2.h / 2 + 'px';
      if (b2.rot !== box.rot) host.style.transform = `rotate(${b2.rot - box.rot}deg)`;
    }
    drawBody(host, node, body, c, phInfo, phColor, box);
    e.appendChild(host);
  } else if (phInfo && ctx.level === 'slide' && !/^(pic|media|clipArt|chart|tbl|dgm)$/.test(phInfo.type)) {
    const lay = phInfo.chain.map((q) => kid(q, 'txBody')).find((b) => b && kids(b, 'p').length);
    if (lay) {
      const host = div('x-tb x-hint');
      drawBody(host, node, lay, ctx, phInfo, null, box);
      const t = host.firstChild;
      t._body = body;
      t._shape = node;
      t._hint = true;
      e.appendChild(host);
    }
  }
  parent.appendChild(e);
}
function drawPic(node, parent, ctx, map) {
  const phInfo = findPh(ctx, node),
    x = xfrmOf(node, phInfo);
  if (!x) return;
  const box = boxOf(x, map),
    pr = kid(node, 'spPr'),
    fill = kid(node, 'blipFill'),
    blip = kid(fill, 'blip'),
    e = div('x-s x-pic');
  placeBox(e, box);
  const id = str(kid(kid(node, 'nvPicPr'), 'cNvPr'), 'id');
  if (id) e.dataset.sp = id;
  e._node = node;
  e._box = box;
  const url = urlOf(ctx, rid(blip, 'embed'), blip),
    nv = kid(node, 'nvPicPr', 'nvPr');
  const vf = nv && kid(nv, 'videoFile'),
    af = nv && kid(nv, 'audioFile'),
    em = nv && nv.getElementsByTagNameNS('*', 'media')[0],
    mediaRid = (em && rid(em, 'embed')) || (vf && rid(vf, 'link')) || (af && rid(af, 'link'));
  if (mediaRid) {
    const r = ctx.deck.relsOfNode(node).get(mediaRid);
    if (r && !r.external && ctx.deck.files[r.target]) {
      const ext = r.target.split('.').pop().toLowerCase(),
        kind = af || /^(mp3|wav|m4a|wma|aac|ogg|flac)$/.test(ext) ? 'audio' : 'video';
      let u = ctx.deck.urls.get(r.target);
      if (!u) {
        u = URL.createObjectURL(new Blob([ctx.deck.files[r.target]], { type: MEDIA_MIME[ext] || '' }));
        ctx.deck.urls.set(r.target, u);
      }
      e.classList.add('x-media');
      e._media = { url: u, kind };
      const play = div('x-play');
      play.style.setProperty('--z', Math.min(box.w, box.h) * 0.3 + 'px');
      e._play = play;
    }
  }
  const g = pr && (kid(pr, 'prstGeom') || kid(pr, 'custGeom')),
    crop = kid(fill, 'srcRect'),
    L = num(crop, 'l', 0) / 100000,
    R = num(crop, 'r', 0) / 100000,
    T = num(crop, 't', 0) / 100000,
    B = num(crop, 'b', 0) / 100000;
  const hold = div('x-pin');
  e.appendChild(hold);
  if (g && str(g, 'prst') !== 'rect') hold.style.clipPath = `path('${pathOf(pr, phInfo, box)}')`;
  if (url) {
    const im = document.createElement('img');
    im.src = url;
    im.alt = '';
    im.draggable = false;
    im.style.width = box.w / Math.max(0.01, 1 - L - R) + 'px';
    im.style.height = box.h / Math.max(0.01, 1 - T - B) + 'px';
    im.style.left = (-L / Math.max(0.01, 1 - L - R)) * box.w + 'px';
    im.style.top = (-T / Math.max(0.01, 1 - T - B)) * box.h + 'px';
    const al = blip && kid(blip, 'alphaModFix');
    if (al) im.style.opacity = num(al, 'amt', 100000) / 100000;
    const duo = blip && kid(blip, 'duotone');
    if (duo && kids(duo).length >= 2) {
      const [c0, c1] = kids(duo)
          .slice(0, 2)
          .map((n) => colorOf({ children: [n] }, ctx)),
        id2 = 'd' + ++uid,
        sv = svg('svg', { width: 0, height: 0, style: 'position:absolute' }),
        fl = svg('filter', { id: id2, 'color-interpolation-filters': 'sRGB' }),
        ct = svg('feComponentTransfer'),
        mx = svg('feColorMatrix', { type: 'matrix', values: '.3 .59 .11 0 0 .3 .59 .11 0 0 .3 .59 .11 0 0 0 0 0 1 0' });
      ['R', 'G', 'B'].forEach((ch, i) =>
        ct.appendChild(svg('feFunc' + ch, { type: 'table', tableValues: `${c0.rgb[i] / 255} ${c1.rgb[i] / 255}` }))
      );
      fl.append(mx, ct);
      sv.appendChild(fl);
      e.appendChild(sv);
      im.style.filter = `url(#${id2})`;
    }
    hold.appendChild(im);
  } else hold.classList.add('x-none');
  const line = lineOf(pr, ctx, kid(node, 'style'));
  if (line) drawGeometry(e, pathOf(pr, phInfo, box), null, line, box);
  const sh = pr ? shadowOf(pr, ctx) : '';
  if (sh) e.style.filter = sh;
  if (e._play) e.appendChild(e._play);
  parent.appendChild(e);
}
function drawGroup(node, parent, ctx, map) {
  const pr = kid(node, 'grpSpPr'),
    x = pr && kid(pr, 'xfrm');
  if (!x) {
    drawTree(kids(node), parent, ctx, map);
    return;
  }
  const box = boxOf(x, map),
    off = kid(x, 'chOff'),
    ext = kid(x, 'chExt'),
    cx = num(ext, 'cx', 1) || 1,
    cy = num(ext, 'cy', 1) || 1,
    own = kid(x, 'ext');
  const e = div('x-s x-grp');
  placeBox(e, box);
  const id = str(kid(kid(node, 'nvGrpSpPr'), 'cNvPr'), 'id');
  if (id) e.dataset.sp = id;
  e._node = node;
  e._box = box;
  const inner = {
    ox: num(off, 'x', 0),
    oy: num(off, 'y', 0),
    sx: (num(own, 'cx', cx) / cx) * map.sx,
    sy: (num(own, 'cy', cy) / cy) * map.sy,
  };
  const fill = fillFrom(pr, ctx);
  drawTree(kids(node), e, fill ? { ...ctx, groupFill: fill } : ctx, inner);
  parent.appendChild(e);
}
const TBL_DEFAULT = { '{5C22544A-7EE6-4342-B048-85BDC9FD1C3A}': true };
function tableStylePart(ctx, id, part) {
  const s = ctx.deck.tableStyles.get(id);
  return s ? kid(s, part) : null;
}
function drawFrame(node, parent, ctx, map) {
  const x = kid(node, 'xfrm') || xfrmOf(node);
  if (!x) return;
  const box = boxOf(x, map),
    data = kid(node, 'graphic', 'graphicData');
  if (!data) return;
  const tbl = kid(data, 'tbl'),
    chartNode = kid(data, 'chart'),
    dgm = kid(data, 'relIds'),
    ole = kid(data, 'oleObj') || (data.querySelector && [...data.getElementsByTagNameNS('*', 'pic')][0]);
  const e = div('x-s');
  placeBox(e, box);
  const id = str(kid(kid(node, 'nvGraphicFramePr'), 'cNvPr'), 'id');
  if (id) e.dataset.sp = id;
  e._node = node;
  e._box = box;
  if (tbl) drawTable(tbl, e, ctx, box);
  else if (chartNode) {
    const r = ctx.rels.get(rid(chartNode, 'id')),
      d = r && ctx.deck.xml(r.target);
    if (d) {
      const c = { ...ctx, rels: ctx.deck.relsOf(r.target) };
      drawChart(e, d, c, box);
      ctx.deck.charts = ctx.deck.charts || 0;
    }
  } else if (dgm) {
    // SmartArt keeps a drawing of itself; that is what PowerPoint shows too
    const rel = [...ctx.rels.values()].find((v) => v.type === 'diagramDrawing');
    const all = [...ctx.rels.values()].filter((v) => v.type === 'diagramDrawing');
    void rel;
    const want = rid(dgm, 'dm'),
      dm = ctx.rels.get(want),
      dmDoc = dm && ctx.deck.xml(dm.target);
    let drawPath = null;
    if (dmDoc) {
      const ext = [...dmDoc.getElementsByTagNameNS('*', 'dataModelExt')][0],
        rel2 = ext && ext.getAttribute('relId');
      if (rel2 && ctx.rels.get(rel2)) drawPath = ctx.rels.get(rel2).target;
    }
    if (!drawPath && all[0]) drawPath = all[0].target;
    const dd = drawPath && ctx.deck.xml(drawPath),
      tree = dd && kid(dd, 'spTree');
    if (tree) {
      const c = { ...ctx, rels: ctx.deck.relsOf(drawPath) };
      drawTree(kids(tree), e, c, { ox: 0, oy: 0, sx: 1, sy: 1 });
    } else if (dmDoc) {
      const words = kids(kid(dmDoc, 'ptLst'), 'pt')
        .filter((q) => !str(q, 'type') || str(q, 'type') === 'node')
        .map((q) =>
          [...q.getElementsByTagNameNS('*', 'r')].map((r) => (kid(r, 't') ? kid(r, 't').textContent : '')).join('')
        )
        .filter(Boolean);
      const n = words.length,
        cols = Math.max(1, Math.round(Math.sqrt((n * box.w) / Math.max(box.h, 1) / 2))),
        rowsN = Math.ceil(n / cols),
        g = 6,
        tw = (box.w - g * (cols - 1)) / cols,
        th = (box.h - g * (rowsN - 1)) / rowsN,
        col = { rgb: hex2(ctx.theme.colors.accent1 || '4472C4'), a: 1 };
      words.forEach((w, i) => {
        const c = div('x-word');
        c.style.cssText = `position:absolute;display:flex;align-items:center;justify-content:center;text-align:center;overflow:hidden;box-sizing:border-box;padding:4px;left:${(i % cols) * (tw + g)}px;top:${Math.floor(i / cols) * (th + g)}px;width:${tw}px;height:${th}px;border-radius:4px;color:#fff;font-size:${Math.max(9, Math.min(18, th * 0.3))}px;background:${css(col)}`;
        const pw = document.createElement('p');
        pw.textContent = w;
        pw.style.margin = 0;
        c.appendChild(pw);
        e.appendChild(c);
      });
    }
  } else if (ole)
    drawPic(ole, e, ctx, { ox: num(kid(x, 'off'), 'x', 0), oy: num(kid(x, 'off'), 'y', 0), sx: 1, sy: 1 });
  parent.appendChild(e);
}
function drawTable(tbl, host, ctx, box) {
  const pr = kid(tbl, 'tblPr'),
    styleId =
      str(kid(pr, 'tableStyleId'), 'val') || (pr && kid(pr, 'tableStyleId') ? kid(pr, 'tableStyleId').textContent : ''),
    table = document.createElement('table');
  const firstRow = str(pr, 'firstRow') === '1',
    firstCol = str(pr, 'firstCol') === '1',
    lastRow = str(pr, 'lastRow') === '1',
    lastCol = str(pr, 'lastCol') === '1',
    bandRow = str(pr, 'bandRow') === '1',
    bandCol = str(pr, 'bandCol') === '1';
  const cols = kids(kid(tbl, 'tblGrid'), 'gridCol').map((g) => num(g, 'w', 0) / EMU),
    rows = kids(tbl, 'tr');
  const cg = document.createElement('colgroup');
  cols.forEach((w) => {
    const c = document.createElement('col');
    c.style.width = w + 'px';
    cg.appendChild(c);
  });
  table.appendChild(cg);
  table.style.width = cols.reduce((a, b) => a + b, 0) + 'px';
  const known = ctx.deck.tableStyles.has(styleId),
    fallback = !known && TBL_DEFAULT[styleId];
  rows.forEach((row, ri) => {
    const tr = document.createElement('tr');
    tr.style.height = num(row, 'h', 0) / EMU + 'px';
    let cx = 0;
    kids(row, 'tc').forEach((tc) => {
      const ci = cx;
      cx++;
      if (str(tc, 'hMerge') === '1' || str(tc, 'vMerge') === '1') return;
      const td = document.createElement('td'),
        tcPr = kid(tc, 'tcPr'),
        gs = num(tc, 'gridSpan', 1),
        rs = num(tc, 'rowSpan', 1);
      if (gs > 1) td.colSpan = gs;
      if (rs > 1) td.rowSpan = rs;
      const ins = (a, d) => num(tcPr, a, d) / EMU;
      td.style.padding = `${ins('marT', 45720)}px ${ins('marR', 91440)}px ${ins('marB', 45720)}px ${ins('marL', 91440)}px`;
      const parts = [];
      if (known) {
        parts.push('wholeTbl');
        if (bandRow) parts.push((ri - (firstRow ? 1 : 0)) % 2 === 0 ? 'band1H' : 'band2H');
        if (bandCol) parts.push((ci - (firstCol ? 1 : 0)) % 2 === 0 ? 'band1V' : 'band2V');
        if (firstCol && ci === 0) parts.push('firstCol');
        if (lastCol && ci === cols.length - 1) parts.push('lastCol');
        if (lastRow && ri === rows.length - 1) parts.push('lastRow');
        if (firstRow && ri === 0) parts.push('firstRow');
      }
      let bg = null,
        txt = null,
        bold = false;
      const edges = {};
      for (const name of parts) {
        const part = tableStylePart(ctx, styleId, name);
        if (!part) continue;
        const st = kid(part, 'tcStyle'),
          tx = kid(part, 'tcTxStyle');
        if (st) {
          const f = kid(st, 'fill');
          if (f) {
            const ff = fillFrom(f, ctx);
            if (ff) bg = ff;
          } else {
            const fr = kid(st, 'fillRef');
            if (fr) bg = { color: colorOf(fr, ctx) };
          }
          const bd = kid(st, 'tcBdr');
          if (bd) for (const side of kids(bd)) edges[side.localName] = side;
        }
        if (tx) {
          const c = colorOf(tx, ctx) || (kid(tx, 'fontRef') && colorOf(kid(tx, 'fontRef'), ctx));
          if (c) txt = c;
          if (str(tx, 'b') === 'on') bold = true;
          else if (str(tx, 'b') === 'off') bold = false;
        }
      }
      if (fallback) {
        const a = { rgb: hex2(ctx.theme.colors.accent1 || '4472C4'), a: 1 },
          tint = (k) => ({ rgb: a.rgb.map((v) => fromLin(toLin(v) * k + (1 - k))), a: 1 });
        if (firstRow && ri === 0) {
          bg = { color: a };
          txt = { rgb: hex2(ctx.theme.colors.lt1 || 'FFFFFF'), a: 1 };
          bold = true;
        } else bg = { color: tint(bandRow && (ri - (firstRow ? 1 : 0)) % 2 === 0 ? 0.4 : 0.2) };
        td.style.border = `1px solid ${css({ rgb: hex2(ctx.theme.colors.lt1 || 'FFFFFF'), a: 1 })}`;
      }
      const own = tcPr ? fillFrom(tcPr, ctx) : null;
      if (own) bg = own;
      if (bg) td.style.background = cssFill(bg);
      const side = {
        lnL: ['Left', 'left', 'insideV'],
        lnR: ['Right', 'right', 'insideV'],
        lnT: ['Top', 'top', 'insideH'],
        lnB: ['Bottom', 'bottom', 'insideH'],
      };
      for (const k in side) {
        const [Css, outer, inside] = side[k];
        let ln = tcPr && kid(tcPr, k),
          line = null;
        if (ln) line = lineOf({ localName: 'x', children: [ln] }, ctx);
        else {
          const atEdge =
              (k === 'lnL' && ci === 0)
              || (k === 'lnR' && ci + gs >= cols.length)
              || (k === 'lnT' && ri === 0)
              || (k === 'lnB' && ri + rs >= rows.length),
            holder = edges[atEdge ? outer : inside];
          if (holder) {
            const l2 = kid(holder, 'ln'),
              rf = kid(holder, 'lnRef');
            if (l2) line = lineOf({ localName: 'x', children: [l2] }, ctx);
            else if (rf) line = lineOf(null, ctx, { localName: 'x', children: [rf] });
          }
        }
        if (line)
          td.style['border' + Css] = `${Math.max(line.w, 0.5)}px ${line.dash ? 'dashed' : 'solid'} ${css(line.color)}`;
      }
      const a = str(tcPr, 'anchor');
      td.style.verticalAlign = a === 'ctr' ? 'middle' : a === 'b' ? 'bottom' : 'top';
      const body = kid(tc, 'txBody');
      if (body) {
        const c = { ...ctx, inTable: true, fontRefStyle: null },
          cell = div('x-cell');
        drawBody(
          cell,
          tc,
          body,
          c,
          null,
          txt || { rgb: hex2(ctx.theme.colors[ctx.clrMap.tx1 || 'dk1'] || '000000'), a: 1 },
          box
        );
        const inner = cell.firstChild;
        inner.style.padding = 0;
        inner.style.position = 'static';
        inner.style.display = 'block';
        if (bold)
          cell.querySelectorAll('span').forEach((sp) => {
            if (!sp.style.fontWeight) sp.style.fontWeight = 700;
          });
        td.appendChild(cell);
      }
      tr.appendChild(td);
    });
    table.appendChild(tr);
  });
  host.appendChild(table);
}
function drawTree(list, parent, ctx, map) {
  for (const n of unwrap(list)) {
    const k = n.localName;
    if (k === 'sp' || k === 'cxnSp') drawSp(n, parent, ctx, map);
    else if (k === 'pic') drawPic(n, parent, ctx, map);
    else if (k === 'grpSp') drawGroup(n, parent, ctx, map);
    else if (k === 'graphicFrame') drawFrame(n, parent, ctx, map);
  }
}
const notPh = (list) => unwrap(list).filter((n) => !phOf(n));

// one slide, whole, at its natural size
export function paintSlide(deck, i) {
  const s = deck.slides[i],
    root = div('slide');
  root.style.width = deck.w + 'px';
  root.style.height = deck.h + 'px';
  if (!s.master) return root;
  const ctx = contextFor(deck, s, 'slide'),
    bg = background(deck, s, ctx);
  if (bg && bg.url) {
    root.style.backgroundImage = `url("${bg.url}")`;
    root.style.backgroundSize = bg.tile ? 'auto' : '100% 100%';
  } else root.style.background = cssFill(bg);
  const hideMaster = str(s.xml, 'showMasterSp') === '0',
    hideLayoutMaster = s.layout && str(s.layout.xml, 'showMasterSp') === '0';
  if (!hideMaster && !hideLayoutMaster) {
    const t = kid(s.master.xml, 'cSld', 'spTree');
    if (t)
      drawTree(
        notPh(kids(t)),
        root,
        { ...withRels(ctx, s.master.rels), level: 'master' },
        { ox: 0, oy: 0, sx: 1, sy: 1 }
      );
  }
  if (!hideMaster && s.layout) {
    const t = kid(s.layout.xml, 'cSld', 'spTree');
    if (t)
      drawTree(
        notPh(kids(t)),
        root,
        { ...withRels(ctx, s.layout.rels), level: 'layout' },
        { ox: 0, oy: 0, sx: 1, sy: 1 }
      );
  }
  const before = root.children.length,
    tree = kid(s.xml, 'cSld', 'spTree');
  if (tree) drawTree(kids(tree), root, ctx, { ox: 0, oy: 0, sx: 1, sy: 1 });
  [...root.children].slice(before).forEach((c) => c.classList.add('x-top'));
  return root;
}
