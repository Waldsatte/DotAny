// reading a .pptx: the parts, the inheritance between slide, layout and master, the theme, notes, click steps and transitions
import { unzipSync, strFromU8 } from 'fflate';

export const kids = (n, name) => (n ? [...n.children].filter((c) => !name || c.localName === name) : []);
export const kid = (n, ...path) => {
  let c = n;
  for (const name of path) {
    if (!c) return null;
    c = [...c.children].find((x) => x.localName === name) || null;
  }
  return c;
};
export const num = (n, a, d = null) => {
  if (!n) return d;
  const v = n.getAttribute(a);
  return v == null || v === '' ? d : +v;
};
export const str = (n, a) => (n ? n.getAttribute(a) : null);
export const parse = (text) => {
  const d = new DOMParser().parseFromString(text, 'application/xml');
  if (d.querySelector('parsererror')) throw new Error('bad xml');
  return d.documentElement;
};

// PowerPoint wraps newer things in a choice with an older fallback. The fallback is read unless it says nothing
export function unwrap(list) {
  const out = [];
  for (const c of list) {
    if (c.localName === 'AlternateContent') {
      const pick = kid(c, 'Fallback');
      const use = pick && pick.children.length ? pick : kid(c, 'Choice');
      if (use) out.push(...kids(use));
    } else out.push(c);
  }
  return out;
}

const dirOf = (p) => p.slice(0, p.lastIndexOf('/') + 1);
function joinPath(base, rel) {
  if (rel.startsWith('/')) return rel.slice(1);
  const out = dirOf(base).split('/').filter(Boolean);
  for (const s of rel.split('/')) {
    if (s === '..') out.pop();
    else if (s !== '.') out.push(s);
  }
  return out.join('/');
}

export class Encrypted extends Error {}

export function readDeck(bytes) {
  if (bytes[0] === 0xd0 && bytes[1] === 0xcf) throw new Encrypted();
  const files = unzipSync(bytes, { filter: (f) => f.originalSize < 512 * 1048576 });
  const xmlCache = new Map(),
    docPath = new WeakMap(),
    relCache = new Map();
  const xml = (p) => {
    if (xmlCache.has(p)) return xmlCache.get(p);
    const f = files[p];
    if (!f) return null;
    const d = parse(strFromU8(f));
    xmlCache.set(p, d);
    docPath.set(d.ownerDocument, p);
    return d;
  };
  const relsOf = (p) => {
    if (relCache.has(p)) return relCache.get(p);
    const r = new Map();
    relCache.set(p, r);
    const d = xml(dirOf(p) + '_rels/' + p.slice(dirOf(p).length) + '.rels');
    if (d)
      for (const x of kids(d))
        r.set(x.getAttribute('Id'), {
          type: (x.getAttribute('Type') || '').split('/').pop(),
          target:
            x.getAttribute('TargetMode') === 'External'
              ? x.getAttribute('Target')
              : joinPath(p, x.getAttribute('Target')),
          external: x.getAttribute('TargetMode') === 'External',
        });
    return r;
  };
  const relOfType = (rels, t) => {
    for (const r of rels.values()) if (r.type === t) return r;
    return null;
  };

  const rootRels = relsOf('');
  const presPath = (relOfType(rootRels, 'officeDocument') || {}).target || 'ppt/presentation.xml';
  const pres = xml(presPath);
  if (!pres || pres.localName !== 'presentation') throw new Error('not a presentation');
  const presRels = relsOf(presPath),
    size = kid(pres, 'sldSz');
  const deck = {
    files,
    xml,
    relsOf,
    relOfType,
    touched: new Set(),
    presPath,
    pres,
    dropRelCache: () => relCache.clear(),
    relsPath: (p) => dirOf(p) + '_rels/' + p.slice(dirOf(p).length) + '.rels',
    setPart(p, text) {
      const e = parse(text);
      xmlCache.set(p, e);
      docPath.set(e.ownerDocument, p);
      files[p] = files[p] || new Uint8Array(0);
      deck.touched.add(p);
      relCache.clear();
    },
    partText(p) {
      const e = xml(p);
      return e
        ? '<?xml version="1.0" encoding="UTF-8" standalone="yes"?>\n' + new XMLSerializer().serializeToString(e)
        : null;
    },
    hasPart: (p) => !!files[p],
    removePart(p) {
      delete files[p];
      xmlCache.delete(p);
      deck.touched.delete(p);
      relCache.clear();
    },
    relist() {
      deck.pres = xml(presPath);
      const rels = relsOf(presPath),
        list = kids(kid(deck.pres, 'sldIdLst'), 'sldId'),
        old = new Map(deck.slides.map((s) => [s.part, s]));
      deck.slides = list
        .map((s) => {
          const id =
              s.getAttributeNS('http://schemas.openxmlformats.org/officeDocument/2006/relationships', 'id')
              || s.getAttribute('r:id'),
            r = rels.get(id);
          return r ? readSlide(deck, r.target) : null;
        })
        .filter(Boolean);
      void old;
    },
    reread(s) {
      const i = deck.slides.indexOf(s);
      if (i >= 0) {
        deck.slides[i] = readSlide(deck, s.part);
        deck.touched.add(s.part);
        return deck.slides[i];
      }
      return s;
    },
    relsOfNode: (n) => relsOf(docPath.get(n.ownerDocument) || ''),
    w: num(size, 'cx', 12192000) / 12700,
    h: num(size, 'cy', 6858000) / 12700,
    slides: [],
    defaultText: kid(pres, 'defaultTextStyle'),
    mediaBytes: 0,
    fonts: new Set(),
    urls: new Map(),
  };

  const themes = new Map();
  deck.themeAt = (p) => {
    if (themes.has(p)) return themes.get(p);
    const t = readTheme(xml(p));
    themes.set(p, t);
    return t;
  };
  const masters = new Map(),
    layouts = new Map();
  deck.masterAt = (p) => {
    if (masters.has(p)) return masters.get(p);
    const d = xml(p),
      rels = relsOf(p),
      tr = relOfType(rels, 'theme'),
      m = { path: p, xml: d, rels, theme: tr ? deck.themeAt(tr.target) : null, clrMap: {}, styles: kid(d, 'txStyles') };
    const map = kid(d, 'clrMap');
    if (map) for (const a of map.attributes) m.clrMap[a.name] = a.value;
    m.placeholders = phList(d);
    masters.set(p, m);
    return m;
  };
  deck.layoutAt = (p) => {
    if (layouts.has(p)) return layouts.get(p);
    const d = xml(p),
      rels = relsOf(p),
      mr = relOfType(rels, 'slideMaster'),
      l = { path: p, xml: d, rels, master: deck.masterAt(mr.target), placeholders: phList(d) };
    layouts.set(p, l);
    return l;
  };

  const ts = relOfType(presRels, 'tableStyles');
  deck.tableStyles = new Map();
  if (ts && xml(ts.target))
    for (const s of kids(xml(ts.target), 'tblStyle')) deck.tableStyles.set(str(s, 'styleId'), s);

  const list = kids(kid(pres, 'sldIdLst'), 'sldId');
  deck.slideParts = list
    .map((s) => {
      const id =
          s.getAttributeNS('http://schemas.openxmlformats.org/officeDocument/2006/relationships', 'id')
          || s.getAttribute('r:id'),
        r = presRels.get(id);
      return r ? r.target : null;
    })
    .filter(Boolean);
  for (const part of deck.slideParts) deck.slides.push(readSlide(deck, part));
  for (const name in files) if (/^ppt\/media\//.test(name)) deck.mediaBytes += files[name].length;

  const coreR = relOfType(rootRels, 'core-properties'),
    core = coreR ? xml(coreR.target) : null,
    field = (n) => {
      const e = core && kid(core, n);
      return e && e.textContent ? e.textContent : '';
    };
  deck.title = field('title');
  deck.author = field('creator');
  return deck;
}

const phList = (d) => {
  const tree = kid(d, 'cSld', 'spTree'),
    out = [];
  if (tree)
    for (const s of unwrap(kids(tree))) {
      const ph = phOf(s);
      if (ph) out.push({ type: str(ph, 'type') || 'body', idx: str(ph, 'idx'), node: s, given: !!str(ph, 'type') });
    }
  return out;
};
export const phOf = (s) => {
  const nv = kids(s).find((c) => /^nv/.test(c.localName)),
    pr = nv && kid(nv, 'nvPr');
  return pr ? kid(pr, 'ph') : null;
};

function readTheme(d) {
  const t = { colors: {}, major: {}, minor: {}, fills: [], lines: [], bgFills: [] };
  if (!d) return t;
  const el = kid(d, 'themeElements'),
    scheme = kid(el, 'clrScheme');
  if (scheme)
    for (const c of kids(scheme)) {
      const v = kids(c)[0];
      if (v) t.colors[c.localName] = str(v, 'lastClr') || str(v, 'val');
    }
  const fonts = kid(el, 'fontScheme');
  const read = (n) => {
    const out = { latin: str(kid(n, 'latin'), 'typeface') || '', ea: str(kid(n, 'ea'), 'typeface') || '' };
    if (n) for (const f of kids(n, 'font')) if (str(f, 'script') === 'Hans' && !out.ea) out.ea = str(f, 'typeface');
    return out;
  };
  if (fonts) {
    t.major = read(kid(fonts, 'majorFont'));
    t.minor = read(kid(fonts, 'minorFont'));
  }
  const fmt = kid(el, 'fmtScheme');
  if (fmt) {
    t.fills = kids(kid(fmt, 'fillStyleLst'));
    t.lines = kids(kid(fmt, 'lnStyleLst'));
    t.bgFills = kids(kid(fmt, 'bgFillStyleLst'));
  }
  return t;
}

function readSlide(deck, part) {
  const d = deck.xml(part),
    rels = deck.relsOf(part),
    lr = deck.relOfType(rels, 'slideLayout'),
    layout = lr ? deck.layoutAt(lr.target) : null;
  const s = {
    part,
    xml: d,
    rels,
    layout,
    master: layout ? layout.master : null,
    hidden: str(d, 'show') === '0',
    texts: [],
    title: '',
    notes: '',
    trans: readTransition(d),
    steps: [],
    stepCount: 0,
  };
  const walk = (n) => {
    for (const c of unwrap(kids(n))) {
      if (c.localName === 'grpSp') {
        walk(c);
        continue;
      }
      const ph = phOf(c);
      const t = textOfShape(c);
      if (t) {
        s.texts.push(t);
        if (ph && /title/i.test(str(ph, 'type') || '') && !s.title) s.title = t;
      }
    }
  };
  const tree = kid(d, 'cSld', 'spTree');
  if (tree) walk(tree);
  const nr = deck.relOfType(rels, 'notesSlide');
  if (nr) {
    const nd = deck.xml(nr.target),
      nt = nd && kid(nd, 'cSld', 'spTree');
    if (nt)
      for (const c of kids(nt)) {
        const ph = phOf(c);
        if (ph && str(ph, 'type') === 'body') {
          const body = kid(c, 'txBody');
          s.notes = kids(body, 'p')
            .map((p) => paraText(p))
            .join('\n')
            .trim();
        }
      }
  }
  readSteps(s, d);
  return s;
}
const paraText = (p) =>
  kids(p)
    .map((r) =>
      r.localName === 'br'
        ? '\n'
        : r.localName === 'r' || r.localName === 'fld'
          ? kid(r, 't')
            ? kid(r, 't').textContent
            : ''
          : ''
    )
    .join('');
function textOfShape(c) {
  const body = kid(c, 'txBody');
  if (body) return kids(body, 'p').map(paraText).join('\n').trim();
  const frame = kid(c, 'graphic', 'graphicData', 'tbl');
  if (frame) return [...frame.getElementsByTagNameNS('*', 't')].map((t) => t.textContent).join(' ');
  return '';
}

function readTransition(d) {
  let t = null;
  const look = (n) => {
    for (const c of kids(n)) {
      if (c.localName === 'transition') {
        if (!t) t = c;
      } else if (c.localName === 'AlternateContent') for (const b of kids(c)) look(b);
    }
  };
  look(d);
  if (!t) return null;
  const kind = kids(t)[0],
    name = kind ? kind.localName : 'fade';
  const spd = str(t, 'spd'),
    own =
      t.getAttributeNS('http://schemas.microsoft.com/office/powerpoint/2010/main', 'dur') || t.getAttribute('p14:dur');
  const out = {
    kind:
      name === 'pull'
        ? 'cover'
        : ['push', 'cover', 'wipe', 'zoom', 'cut'].includes(name)
          ? name
          : name === 'newsflash'
            ? 'zoom'
            : 'fade',
    dir: str(kind, 'dir') || 'l',
    dur: own ? +own : spd === 'fast' ? 300 : spd === 'slow' ? 1000 : 500,
    advance: num(t, 'advTm'),
    onClick: str(t, 'advClick') !== '0',
  };
  return out;
}

// what happens on each click: which shapes or paragraphs come in, and which go
function readSteps(s, d) {
  const timing = kid(d, 'timing');
  if (!timing) return;
  let step = 0;
  const delayOf = (n) => {
    const cond = kid(kid(n, 'stCondLst'), 'cond'),
      v = cond ? str(cond, 'delay') : '0';
    return v && v !== 'indefinite' ? +v : 0;
  };
  const walk = (n, wait) => {
    for (const c of kids(n)) {
      if (c.localName === 'cTn' && str(c, 'nodeType') === 'interactiveSeq') continue;
      if (c.localName === 'cTn' && str(c, 'presetClass')) {
        const nt = str(c, 'nodeType');
        if (nt === 'clickEffect') step++;
        const tgt = c.getElementsByTagNameNS('*', 'spTgt')[0];
        if (tgt) {
          const rg = tgt.getElementsByTagNameNS('*', 'pRg')[0];
          let dur = 0;
          for (const q of c.getElementsByTagNameNS('*', 'cTn')) {
            const v = num(q, 'dur');
            if (v && v > dur) dur = v;
          }
          const fx = c.getElementsByTagNameNS('*', 'animEffect')[0];
          s.steps.push({
            step,
            when: nt === 'withEffect' ? 'with' : nt === 'afterEffect' ? 'after' : 'click',
            sp: str(tgt, 'spid'),
            para: rg ? [num(rg, 'st'), num(rg, 'end')] : null,
            cls: str(c, 'presetClass'),
            preset: num(c, 'presetID'),
            sub: num(c, 'presetSubtype', 0),
            dur: dur > 1 ? dur : 0,
            wait: wait + delayOf(c),
            filter: fx ? str(fx, 'filter') : '',
          });
        }
      } else walk(c, c.localName === 'cTn' ? wait + delayOf(c) : wait);
    }
  };
  walk(timing, 0);
  s.stepCount = step;
  // a shape set to appear with the slide's own arrival belongs to no click
}

// what a file's words will really be drawn in: a name that can't be measured as different from the fallback isn't installed
const meter = typeof document !== 'undefined' ? document.createElement('canvas').getContext('2d') : null;
export function fontInstalled(name) {
  if (!meter) return true;
  const probe = 'mmwwMMlliiW字体永';
  for (const base of ['monospace', 'serif']) {
    meter.font = `72px ${base}`;
    const a = meter.measureText(probe).width;
    meter.font = `72px "${name}", ${base}`;
    if (Math.abs(meter.measureText(probe).width - a) > 0.5) return true;
  }
  return false;
}
