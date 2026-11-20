// changing a file: every edit goes into the part's own XML, everything the edit doesn't touch stays exactly as it came
import { zipSync, strToU8 } from 'fflate';
import { kids, kid, num, str, unwrap } from './deck.js';

// what this program names the things it adds, in the window's language: set by main.js (DotAny Slides)
export const NAMES = {};
export const NS = {
  a: 'http://schemas.openxmlformats.org/drawingml/2006/main',
  r: 'http://schemas.openxmlformats.org/officeDocument/2006/relationships',
  p: 'http://schemas.openxmlformats.org/presentationml/2006/main',
  p14: 'http://schemas.microsoft.com/office/powerpoint/2010/main',
  mc: 'http://schemas.openxmlformats.org/markup-compatibility/2006',
};
const ROOTNS = `xmlns:a="${NS.a}" xmlns:r="${NS.r}" xmlns:p="${NS.p}" xmlns:p14="${NS.p14}"`;
const HEAD = '<?xml version="1.0" encoding="UTF-8" standalone="yes"?>\n';
const E = (pt) => Math.round(pt * 12700);
const REL = 'http://schemas.openxmlformats.org/officeDocument/2006/relationships';
const esc = (s) => String(s).replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;').replace(/"/g, '&quot;');
export const toXml = (node) => HEAD + new XMLSerializer().serializeToString(node);
function frag(owner, xml) {
  const d = new DOMParser().parseFromString(`<x ${ROOTNS}>${xml}</x>`, 'application/xml');
  return [...d.documentElement.children].map((c) => owner.ownerDocument.importNode(c, true));
}
const one = (owner, xml) => frag(owner, xml)[0];
const treeOf = (s) => kid(s.xml, 'cSld', 'spTree');
const nvOf = (n) => kids(n).find((c) => /^nv/.test(c.localName));
export const idOf = (n) => str(kid(nvOf(n), 'cNvPr'), 'id');
export const nameOf = (n) => str(kid(nvOf(n), 'cNvPr'), 'name') || '';
export function nextId(s) {
  let m = 1;
  for (const e of s.xml.getElementsByTagNameNS('*', 'cNvPr')) m = Math.max(m, +e.getAttribute('id') || 0);
  return m + 1;
}

// where a thing sits, in points. A placeholder that only inherited its place gets one of its own the first time it moves
export function setBox(node, b) {
  let x;
  if (node.localName === 'graphicFrame') x = kid(node, 'xfrm');
  else {
    const pr = kid(node, 'spPr') || kid(node, 'grpSpPr');
    x = kid(pr, 'xfrm');
    if (!x) {
      x = one(node, '<a:xfrm><a:off x="0" y="0"/><a:ext cx="0" cy="0"/></a:xfrm>');
      pr.insertBefore(x, pr.firstChild);
    }
  }
  const off = kid(x, 'off'),
    ext = kid(x, 'ext');
  if (node.localName === 'grpSp' && !kid(x, 'chOff')) return;
  off.setAttribute('x', E(b.x));
  off.setAttribute('y', E(b.y));
  ext.setAttribute('cx', E(Math.max(b.w, 0)));
  ext.setAttribute('cy', E(Math.max(b.h, 0)));
  if (b.rot) x.setAttribute('rot', Math.round((((b.rot % 360) + 360) % 360) * 60000));
  else x.removeAttribute('rot');
}

// ---------- words: back from what's on screen into runs ----------
const FILL = (c) =>
  /^[0-9A-Fa-f]{6}$/.test(c)
    ? `<a:solidFill><a:srgbClr val="${c}"/></a:solidFill>`
    : `<a:solidFill><a:schemeClr val="${c}"/></a:solidFill>`;
export function writeBody(shape, tEl, runs) {
  let body = tEl._body;
  if (!body) {
    body = one(shape, '<p:txBody><a:bodyPr/><a:lstStyle/></p:txBody>');
    shape.appendChild(body);
  }
  const oldP = kids(body, 'p'),
    firstRun = runs[0] || null,
    outP = [];
  for (const pe of tEl.querySelectorAll(':scope > p')) {
    const src = oldP[+pe.dataset.p] || oldP[0],
      p = one(body, '<a:p/>');
    const pPr = src && kid(src, 'pPr');
    if (pPr) p.appendChild(pPr.cloneNode(true));
    let last = null,
      any = false;
    const add = (text, rk, over) => {
      const orig = runs[rk] || last || firstRun,
        r = one(body, '<a:r><a:t/></a:r>'),
        endP = src && kid(src, 'endParaRPr');
      let rPr =
        orig && kid(orig, 'rPr') ? kid(orig, 'rPr').cloneNode(true) : one(body, '<a:rPr lang="${NAMES.lang}"/>');
      if (!orig && endP) {
        for (const at of endP.attributes) rPr.setAttribute(at.name, at.value);
        for (const k of kids(endP)) rPr.appendChild(k.cloneNode(true));
      }
      rPr.removeAttribute('err');
      rPr.removeAttribute('dirty');
      if (over.b != null) rPr.setAttribute('b', over.b);
      if (over.i != null) rPr.setAttribute('i', over.i);
      if (over.u != null) rPr.setAttribute('u', over.u ? 'sng' : 'none');
      if (over.sz != null) rPr.setAttribute('sz', Math.round(over.sz * 100));
      if (over.c) {
        for (const f of kids(rPr)) if (/Fill$/.test(f.localName)) f.remove();
        rPr.insertBefore(one(body, FILL(over.c)), kids(rPr).find((k) => k.localName !== 'ln') || null);
      }
      r.insertBefore(rPr, r.firstChild);
      kid(r, 't').textContent = text;
      p.appendChild(r);
      last = orig;
      any = true;
    };
    const walk = (n, rk, over) => {
      for (const c of n.childNodes) {
        if (c.nodeType === 3) {
          if (c.nodeValue) add(c.nodeValue.replace(/​/g, ''), rk, over);
        } else if (c.nodeType === 1) {
          if (c.classList.contains('x-bu')) continue;
          if (c.tagName === 'BR') {
            if (c.nextSibling || n !== pe) {
              const br = one(body, '<a:br/>');
              if (last && kid(last, 'rPr')) {
                const bp = kid(last, 'rPr').cloneNode(true);
                br.appendChild(bp);
              }
              p.appendChild(br);
            }
            continue;
          }
          const d = c.dataset || {},
            o2 = { ...over };
          for (const k of ['b', 'i', 'u', 'sz', 'c'])
            if (d[k] != null) o2[k] = k === 'sz' ? +d[k] : k === 'u' ? d[k] === '1' : d[k];
          walk(c, d.rk != null ? +d.rk : rk, o2);
        }
      }
    };
    walk(pe, null, {});
    if (!any) {
      const end = src && kid(src, 'endParaRPr');
      if (end) p.appendChild(end.cloneNode(true));
      else if (firstRun && kid(firstRun, 'rPr') && kid(firstRun, 'rPr').hasAttribute('sz'))
        p.appendChild(
          one(body, `<a:endParaRPr lang="${NAMES.lang}" sz="${kid(firstRun, 'rPr').getAttribute('sz')}"/>`)
        );
    }
    outP.push(p);
  }
  oldP.forEach((p) => p.remove());
  (outP.length ? outP : [one(body, '<a:p/>')]).forEach((p) => body.appendChild(p));
}
export function setAlign(tEl, algn) {
  const body = tEl._body;
  if (!body) return;
  for (const p of kids(body, 'p')) {
    let pr = kid(p, 'pPr');
    if (!pr) {
      pr = one(body, '<a:pPr/>');
      p.insertBefore(pr, p.firstChild);
    }
    pr.setAttribute('algn', algn);
  }
}
export function setBullets(tEl, on) {
  const body = tEl._body;
  if (!body) return;
  for (const p of kids(body, 'p')) {
    let pr = kid(p, 'pPr');
    if (!pr) {
      pr = one(body, '<a:pPr/>');
      p.insertBefore(pr, p.firstChild);
    }
    for (const k of kids(pr)) if (/^bu/.test(k.localName)) k.remove();
    if (on) {
      pr.setAttribute('marL', E(18));
      pr.setAttribute('indent', E(-18));
      pr.appendChild(one(body, '<a:buFont typeface="Arial"/>'));
      pr.appendChild(one(body, '<a:buChar char="•"/>'));
    } else {
      pr.setAttribute('marL', 0);
      pr.setAttribute('indent', 0);
      pr.appendChild(one(body, '<a:buNone/>'));
    }
  }
}
export function setFill(node, hex) {
  const pr = kid(node, 'spPr');
  if (!pr) return;
  for (const k of kids(pr)) if (/Fill$/.test(k.localName)) k.remove();
  const f = one(pr, hex ? FILL(hex) : '<a:noFill/>'),
    geom = kids(pr).find((k) => /Geom$/.test(k.localName));
  pr.insertBefore(f, geom ? geom.nextSibling : kid(pr, 'xfrm') ? kid(pr, 'xfrm').nextSibling : pr.firstChild);
}
export function setLine(node, hex) {
  const pr = kid(node, 'spPr');
  if (!pr) return;
  let ln = kid(pr, 'ln');
  if (!ln) {
    ln = one(pr, '<a:ln w="12700"/>');
    const after = kids(pr).find((k) => /effect|scene3d|sp3d|extLst/i.test(k.localName));
    pr.insertBefore(ln, after || null);
  }
  for (const k of kids(ln)) if (/Fill$/.test(k.localName)) k.remove();
  ln.insertBefore(one(pr, hex ? FILL(hex) : '<a:noFill/>'), ln.firstChild);
}

// ---------- new things ----------
const THEME_FILL = (c) => `<a:solidFill><a:schemeClr val="${c}"/></a:solidFill>`;
export function addTextBox(s, x, y, w, h, words = '') {
  const id = nextId(s),
    n = one(
      treeOf(s),
      `<p:sp><p:nvSpPr><p:cNvPr id="${id}" name="${NAMES.text} ${id}"/><p:cNvSpPr txBox="1"/><p:nvPr/></p:nvSpPr><p:spPr><a:xfrm><a:off x="${E(x)}" y="${E(y)}"/><a:ext cx="${E(w)}" cy="${E(h)}"/></a:xfrm><a:prstGeom prst="rect"><a:avLst/></a:prstGeom><a:noFill/></p:spPr><p:txBody><a:bodyPr wrap="square" rtlCol="0"><a:spAutoFit/></a:bodyPr><a:lstStyle/><a:p>${words ? `<a:r><a:rPr lang="${NAMES.lang}" sz="2000"/><a:t>${esc(words)}</a:t></a:r>` : '<a:endParaRPr lang="${NAMES.lang}" sz="2000"/>'}</a:p></p:txBody></p:sp>`
    );
  treeOf(s).appendChild(n);
  return n;
}
export function addShape(s, prst, x, y, w, h) {
  const id = nextId(s),
    isLine = prst === 'line',
    n = isLine
      ? one(
          treeOf(s),
          `<p:cxnSp><p:nvCxnSpPr><p:cNvPr id="${id}" name="${NAMES.line} ${id}"/><p:cNvCxnSpPr/><p:nvPr/></p:nvCxnSpPr><p:spPr><a:xfrm><a:off x="${E(x)}" y="${E(y)}"/><a:ext cx="${E(w)}" cy="0"/></a:xfrm><a:prstGeom prst="line"><a:avLst/></a:prstGeom><a:ln w="19050">${THEME_FILL('tx1')}</a:ln></p:spPr></p:cxnSp>`
        )
      : one(
          treeOf(s),
          `<p:sp><p:nvSpPr><p:cNvPr id="${id}" name="${NAMES.shape} ${id}"/><p:cNvSpPr/><p:nvPr/></p:nvSpPr><p:spPr><a:xfrm><a:off x="${E(x)}" y="${E(y)}"/><a:ext cx="${E(w)}" cy="${E(h)}"/></a:xfrm><a:prstGeom prst="${prst}"><a:avLst/></a:prstGeom>${THEME_FILL('accent1')}<a:ln><a:noFill/></a:ln></p:spPr><p:txBody><a:bodyPr rtlCol="0" anchor="ctr"/><a:lstStyle/><a:p><a:pPr algn="ctr"/><a:endParaRPr lang="${NAMES.lang}" sz="1800">${THEME_FILL('bg1')}</a:endParaRPr></a:p></p:txBody></p:sp>`
        );
  treeOf(s).appendChild(n);
  return n;
}
function mediaName(deck, stem, ext) {
  let i = 1;
  while (deck.files[`ppt/media/${stem}${i}.${ext}`]) i++;
  return `${stem}${i}.${ext}`;
}
const CT = {
  png: 'image/png',
  jpg: 'image/jpeg',
  jpeg: 'image/jpeg',
  gif: 'image/gif',
  bmp: 'image/bmp',
  svg: 'image/svg+xml',
  mp4: 'video/mp4',
  m4v: 'video/mp4',
  mov: 'video/quicktime',
  wmv: 'video/x-ms-wmv',
  avi: 'video/x-msvideo',
  mp3: 'audio/mpeg',
  wav: 'audio/wav',
  m4a: 'audio/mp4',
  wma: 'audio/x-ms-wma',
  ogg: 'audio/ogg',
  flac: 'audio/flac',
};
function ensureDefault(deck, ext) {
  const ct = deck.xml('[Content_Types].xml');
  if (!kids(ct, 'Default').some((d) => str(d, 'Extension').toLowerCase() === ext)) {
    const d = ct.ownerDocument.createElementNS(ct.namespaceURI, 'Default');
    d.setAttribute('Extension', ext);
    d.setAttribute('ContentType', CT[ext] || 'application/octet-stream');
    ct.insertBefore(d, ct.firstChild);
  }
}
function ensureOverride(deck, part, type) {
  const ct = deck.xml('[Content_Types].xml'),
    name = '/' + part;
  if (kids(ct, 'Override').some((o) => str(o, 'PartName') === name)) return;
  const d = ct.ownerDocument.createElementNS(ct.namespaceURI, 'Override');
  d.setAttribute('PartName', name);
  d.setAttribute('ContentType', type);
  ct.appendChild(d);
}
export function dropOverride(deck, part) {
  const ct = deck.xml('[Content_Types].xml');
  for (const o of kids(ct, 'Override')) if (str(o, 'PartName') === '/' + part) o.remove();
}
function relsDoc(deck, part) {
  const p = part.slice(0, part.lastIndexOf('/') + 1) + '_rels/' + part.slice(part.lastIndexOf('/') + 1) + '.rels';
  let d = deck.xml(p);
  if (!d) {
    deck.setPart(p, HEAD + '<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"/>');
    d = deck.xml(p);
  }
  return { d, p };
}
const relTarget = (from, to) => {
  const a = from.split('/').slice(0, -1),
    b = to.split('/');
  while (a.length && b.length && a[0] === b[0]) {
    a.shift();
    b.shift();
  }
  return '../'.repeat(a.length) + b.join('/');
};
export function addRel(deck, part, type, target, external) {
  const { d, p } = relsDoc(deck, part);
  let n = 1;
  const used = new Set(kids(d).map((x) => str(x, 'Id')));
  while (used.has('rId' + n)) n++;
  const r = d.ownerDocument.createElementNS(d.namespaceURI, 'Relationship');
  r.setAttribute('Id', 'rId' + n);
  r.setAttribute('Type', type.startsWith('http') ? type : REL + '/' + type);
  r.setAttribute('Target', external ? target : relTarget(part, target));
  if (external) r.setAttribute('TargetMode', 'External');
  d.appendChild(r);
  deck.touched.add(p);
  deck.dropRelCache();
  return 'rId' + n;
}
export function addPicture(deck, s, bytes, ext, x, y, w, h, name = NAMES.picture) {
  ext = ext.toLowerCase();
  const m = mediaName(deck, 'image', ext),
    path = 'ppt/media/' + m;
  deck.files[path] = bytes;
  ensureDefault(deck, ext);
  const rid = addRel(deck, s.part, 'image', path),
    id = nextId(s),
    n = one(
      treeOf(s),
      `<p:pic><p:nvPicPr><p:cNvPr id="${id}" name="${esc(name)} ${id}"/><p:cNvPicPr><a:picLocks noChangeAspect="1"/></p:cNvPicPr><p:nvPr/></p:nvPicPr><p:blipFill><a:blip r:embed="${rid}"/><a:stretch><a:fillRect/></a:stretch></p:blipFill><p:spPr><a:xfrm><a:off x="${E(x)}" y="${E(y)}"/><a:ext cx="${E(w)}" cy="${E(h)}"/></a:xfrm><a:prstGeom prst="rect"><a:avLst/></a:prstGeom></p:spPr></p:pic>`
    );
  treeOf(s).appendChild(n);
  deck.touched.add('[Content_Types].xml');
  return n;
}
// a film or a sound is a picture with something playable behind it, the way PowerPoint keeps them
export function addMedia(deck, s, bytes, ext, kind, poster, x, y, w, h) {
  ext = ext.toLowerCase();
  const m = mediaName(deck, 'media', ext),
    path = 'ppt/media/' + m;
  deck.files[path] = bytes;
  ensureDefault(deck, ext);
  const linkRid = addRel(deck, s.part, kind, path),
    embedRid = addRel(deck, s.part, 'http://schemas.microsoft.com/office/2007/relationships/media', path);
  ensureDefault(deck, 'png');
  const pm = mediaName(deck, 'image', 'png'),
    pp = 'ppt/media/' + pm;
  deck.files[pp] = poster;
  const imgRid = addRel(deck, s.part, 'image', pp),
    id = nextId(s);
  const n = one(
    treeOf(s),
    `<p:pic><p:nvPicPr><p:cNvPr id="${id}" name="${kind === 'video' ? NAMES.video : NAMES.audio} ${id}"/><p:cNvPicPr><a:picLocks noChangeAspect="1"/></p:cNvPicPr><p:nvPr><a:${kind}File r:link="${linkRid}"/><p:extLst><p:ext uri="{DAA4B4D4-6D71-4841-9C94-3DE7FCFB9230}"><p14:media r:embed="${embedRid}"/></p:ext></p:extLst></p:nvPr></p:nvPicPr><p:blipFill><a:blip r:embed="${imgRid}"/><a:stretch><a:fillRect/></a:stretch></p:blipFill><p:spPr><a:xfrm><a:off x="${E(x)}" y="${E(y)}"/><a:ext cx="${E(w)}" cy="${E(h)}"/></a:xfrm><a:prstGeom prst="rect"><a:avLst/></a:prstGeom></p:spPr></p:pic>`
  );
  treeOf(s).appendChild(n);
  deck.touched.add('[Content_Types].xml');
  return n;
}
export function cloneShape(s, node, dx = 14, dy = 14) {
  const c = node.cloneNode(true);
  let id = nextId(s);
  for (const e of c.getElementsByTagNameNS('*', 'cNvPr')) e.setAttribute('id', id++);
  const nv = kid(nvOf(c), 'cNvPr');
  if (nv) nv.setAttribute('id', str(nv, 'id'));
  const pr = kid(c, 'spPr') || kid(c, 'grpSpPr'),
    x = (pr && kid(pr, 'xfrm')) || kid(c, 'xfrm'),
    off = x && kid(x, 'off');
  if (off) {
    off.setAttribute('x', num(off, 'x', 0) + E(dx));
    off.setAttribute('y', num(off, 'y', 0) + E(dy));
  }
  treeOf(s).appendChild(c);
  return c;
}
export function zMove(s, node, how) {
  const tree = treeOf(s),
    items = kids(tree).filter((c) => !/^(nvGrpSpPr|grpSpPr|extLst)$/.test(c.localName)),
    i = items.indexOf(node);
  if (i < 0) return false;
  const end = kids(tree).find((c) => c.localName === 'extLst') || null;
  if (how === 'up' && i < items.length - 1) tree.insertBefore(node, items[i + 1].nextSibling || end);
  else if (how === 'down' && i > 0) tree.insertBefore(node, items[i - 1]);
  else return false;
  return true;
}

// ---------- transitions ----------
export function setTransition(s, kind, dir, durMs) {
  const d = s.xml;
  for (const c of kids(d)) {
    if (c.localName === 'transition') c.remove();
    else if (c.localName === 'AlternateContent' && c.getElementsByTagNameNS('*', 'transition').length) c.remove();
  }
  if (!kind) return;
  const inner =
    kind === 'fade'
      ? '<p:fade/>'
      : kind === 'push'
        ? `<p:push dir="${dir}"/>`
        : kind === 'cover'
          ? `<p:cover dir="${dir}"/>`
          : kind === 'wipe'
            ? `<p:wipe dir="${dir}"/>`
            : '<p:zoom dir="in"/>';
  const spd = durMs <= 400 ? 'fast' : durMs >= 900 ? 'slow' : 'med';
  const n = one(
    d,
    `<mc:AlternateContent xmlns:mc="${NS.mc}"><mc:Choice xmlns:p14="${NS.p14}" Requires="p14"><p:transition spd="${spd}" p14:dur="${durMs}">${inner}</p:transition></mc:Choice><mc:Fallback><p:transition spd="${spd}">${inner}</p:transition></mc:Fallback></mc:AlternateContent>`
  );
  const after = kids(d).find((c) => c.localName === 'timing' || c.localName === 'extLst') || null;
  d.insertBefore(n, after);
}

// ---------- click steps ----------
const SET = (t) =>
  `<p:set><p:cBhvr><p:cTn id="0" dur="1" fill="hold"><p:stCondLst><p:cond delay="0"/></p:stCondLst></p:cTn>${t}<p:attrNameLst><p:attrName>style.visibility</p:attrName></p:attrNameLst></p:cBhvr><p:to><p:strVal val="visible"/></p:to></p:set>`;
const ANIM = (t, attr, from, to, dur) =>
  `<p:anim calcmode="lin" valueType="num"><p:cBhvr additive="base"><p:cTn id="0" dur="${dur}" fill="hold"/>${t}<p:attrNameLst><p:attrName>${attr}</p:attrName></p:attrNameLst></p:cBhvr><p:tavLst><p:tav tm="0"><p:val><p:strVal val="${from}"/></p:val></p:tav><p:tav tm="100000"><p:val><p:strVal val="${to}"/></p:val></p:tav></p:tavLst></p:anim>`;
export const EFFECTS = {
  appear: { id: 1, sub: 0, name: '出现', body: (t) => SET(t) },
  fade: {
    id: 10,
    sub: 0,
    name: '淡入',
    body: (t, d) =>
      SET(t)
      + `<p:animEffect transition="in" filter="fade"><p:cBhvr><p:cTn id="0" dur="${d}"/>${t}</p:cBhvr></p:animEffect>`,
  },
  rise: {
    id: 2,
    sub: 4,
    name: '浮现',
    body: (t, d) => SET(t) + ANIM(t, 'ppt_x', '#ppt_x', '#ppt_x', d) + ANIM(t, 'ppt_y', '1+#ppt_h/2', '#ppt_y', d),
  },
  left: {
    id: 2,
    sub: 8,
    name: '自左侧',
    body: (t, d) => SET(t) + ANIM(t, 'ppt_x', '0-#ppt_w/2', '#ppt_x', d) + ANIM(t, 'ppt_y', '#ppt_y', '#ppt_y', d),
  },
  right: {
    id: 2,
    sub: 2,
    name: '自右侧',
    body: (t, d) => SET(t) + ANIM(t, 'ppt_x', '1+#ppt_w/2', '#ppt_x', d) + ANIM(t, 'ppt_y', '#ppt_y', '#ppt_y', d),
  },
  wipe: {
    id: 22,
    sub: 8,
    name: '擦除',
    body: (t, d) =>
      SET(t)
      + `<p:animEffect transition="in" filter="wipe(left)"><p:cBhvr><p:cTn id="0" dur="${d}"/>${t}</p:cBhvr></p:animEffect>`,
  },
  zoom: {
    id: 23,
    sub: 16,
    name: '缩放',
    body: (t, d) => SET(t) + ANIM(t, 'ppt_w', '0', '#ppt_w', d) + ANIM(t, 'ppt_h', '0', '#ppt_h', d),
  },
};
export const effectKey = (t) => {
  for (const k in EFFECTS)
    if (EFFECTS[k].id === t.preset && (EFFECTS[k].sub === t.sub || EFFECTS[k].id !== 2)) return k;
  return t.cls === 'entr' ? 'fade' : null;
};
function mainSeqList(s, make) {
  let timing = kid(s.xml, 'timing');
  const find = () => {
    if (!timing) return null;
    for (const c of timing.getElementsByTagNameNS('*', 'cTn'))
      if (str(c, 'nodeType') === 'mainSeq') return kid(c, 'childTnLst');
    return null;
  };
  let list = find();
  if (list || !make) return list;
  if (timing) timing.remove();
  timing = one(
    s.xml,
    `<p:timing><p:tnLst><p:par><p:cTn id="1" dur="indefinite" restart="never" nodeType="tmRoot"><p:childTnLst><p:seq concurrent="1" nextAc="seek"><p:cTn id="2" dur="indefinite" nodeType="mainSeq"><p:childTnLst/></p:cTn><p:prevCondLst><p:cond evt="onPrev" delay="0"><p:tgtEl><p:sldTgt/></p:tgtEl></p:cond></p:prevCondLst><p:nextCondLst><p:cond evt="onNext" delay="0"><p:tgtEl><p:sldTgt/></p:tgtEl></p:cond></p:nextCondLst></p:seq></p:childTnLst></p:cTn></p:par></p:tnLst></p:timing>`
  );
  s.xml.insertBefore(timing, kids(s.xml).find((c) => c.localName === 'extLst') || null);
  return find();
}
export function renumber(s) {
  const t = kid(s.xml, 'timing');
  if (!t) return;
  let n = 1;
  for (const c of t.getElementsByTagNameNS('*', 'cTn')) c.setAttribute('id', n++);
}
// remove every effect on one shape, and whatever click holds nothing afterwards
export function clearEffects(s, spid) {
  const list = mainSeqList(s);
  if (!list) return;
  for (const click of kids(list, 'par')) {
    for (const c of [...click.getElementsByTagNameNS('*', 'cTn')]) {
      if (!str(c, 'presetClass')) continue;
      const tgt = c.getElementsByTagNameNS('*', 'spTgt')[0];
      if (tgt && str(tgt, 'spid') === String(spid)) {
        const par = c.parentElement;
        const holder = par.parentElement;
        par.remove();
        if (holder && !kids(holder, 'par').length) {
          const up = holder.parentElement.parentElement;
          up.remove();
        }
      }
    }
    if (!click.getElementsByTagNameNS('*', 'spTgt').length) click.remove();
  }
  // a click whose first effect has gone: the next one takes the click
  for (const click of kids(list, 'par')) {
    const first = [...click.getElementsByTagNameNS('*', 'cTn')].find((c) => str(c, 'presetClass'));
    if (first && str(first, 'nodeType') !== 'clickEffect') first.setAttribute('nodeType', 'clickEffect');
  }
  const bl = kid(kid(s.xml, 'timing'), 'bldLst');
  if (bl) {
    for (const b of kids(bl)) if (str(b, 'spid') === String(spid)) b.remove();
    if (!kids(bl).length) bl.remove();
  }
  if (!list.children.length) kid(s.xml, 'timing').remove();
}
export function addEffect(s, spid, key, when, dur) {
  clearEffects(s, spid);
  const list = mainSeqList(s, true),
    fx = EFFECTS[key],
    tgt = `<p:tgtEl><p:spTgt spid="${spid}"/></p:tgtEl>`,
    clicks = kids(list, 'par'),
    lastClick = clicks[clicks.length - 1];
  const node = (type) =>
    `<p:par><p:cTn id="0" presetID="${fx.id}" presetClass="entr" presetSubtype="${fx.sub}" fill="hold" nodeType="${type}"><p:stCondLst><p:cond delay="0"/></p:stCondLst><p:childTnLst>${fx.body(tgt, dur)}</p:childTnLst></p:cTn></p:par>`;
  if (when === 'with' && lastClick) {
    const inners = kids(kid(kid(lastClick, 'cTn'), 'childTnLst'), 'par'),
      holder = kid(kid(inners[inners.length - 1], 'cTn'), 'childTnLst');
    frag(holder, node('withEffect')).forEach((n) => holder.appendChild(n));
  } else if (when === 'after' && lastClick) {
    const outer = kid(kid(lastClick, 'cTn'), 'childTnLst'),
      total = kids(outer, 'par').reduce((a, p) => a + effectLen(p), 0);
    frag(
      outer,
      `<p:par><p:cTn id="0" fill="hold"><p:stCondLst><p:cond delay="${total}"/></p:stCondLst><p:childTnLst>${node('afterEffect')}</p:childTnLst></p:cTn></p:par>`
    ).forEach((n) => outer.appendChild(n));
  } else
    frag(
      list,
      `<p:par><p:cTn id="0" fill="hold"><p:stCondLst><p:cond delay="indefinite"/></p:stCondLst><p:childTnLst><p:par><p:cTn id="0" fill="hold"><p:stCondLst><p:cond delay="0"/></p:stCondLst><p:childTnLst>${node('clickEffect')}</p:childTnLst></p:cTn></p:par></p:childTnLst></p:cTn></p:par>`
    ).forEach((n) => list.appendChild(n));
  renumber(s);
}
function effectLen(par) {
  let m = 0;
  for (const c of par.getElementsByTagNameNS('*', 'cTn')) {
    const d = num(c, 'dur');
    if (d && d > m) m = d;
  }
  return m;
}
export function moveClick(s, spid, d) {
  const list = mainSeqList(s);
  if (!list) return false;
  const clicks = kids(list, 'par'),
    i = clicks.findIndex((c) =>
      [...c.getElementsByTagNameNS('*', 'spTgt')].some((t) => str(t, 'spid') === String(spid))
    );
  const j = i + d;
  if (i < 0 || j < 0 || j >= clicks.length) return false;
  if (d < 0) list.insertBefore(clicks[i], clicks[j]);
  else list.insertBefore(clicks[j], clicks[i]);
  renumber(s);
  return true;
}

// ---------- pages ----------
const presList = (deck) => kid(deck.xml(deck.presPath), 'sldIdLst');
function newSlidePart(deck) {
  let n = 1;
  while (deck.files[`ppt/slides/slide${n}.xml`] || deck.xml(`ppt/slides/slide${n}.xml`)) n++;
  return `ppt/slides/slide${n}.xml`;
}
function insertId(deck, part, at) {
  const list = presList(deck),
    rid = addRel(deck, deck.presPath, 'slide', part);
  let max = 255;
  for (const s of kids(list)) max = Math.max(max, num(s, 'id', 255));
  const e = list.ownerDocument.createElementNS(NS.p, 'p:sldId');
  e.setAttribute('id', max + 1);
  e.setAttributeNS(NS.r, 'r:id', rid);
  const ref = kids(list)[at] || null;
  list.insertBefore(e, ref);
}
const SLIDE_TYPE = 'application/vnd.openxmlformats-officedocument.presentationml.slide+xml';
// a new page takes a layout's placeholders, empty
export function addSlide(deck, layoutPath, at) {
  const part = newSlidePart(deck),
    layout = deck.layoutAt(layoutPath);
  let id = 2;
  const hold = layout.placeholders
    .filter((p) => !/^(dt|ftr|sldNum)$/.test(p.type))
    .map((p) => {
      const ph = `<p:ph${p.given ? ` type="${p.type}"` : ''}${p.idx != null ? ` idx="${p.idx}"` : ''}/>`;
      const n = id++;
      return `<p:sp><p:nvSpPr><p:cNvPr id="${n}" name="${NAMES.placeholder} ${n}"/><p:cNvSpPr><a:spLocks noGrp="1"/></p:cNvSpPr><p:nvPr>${ph}</p:nvPr></p:nvSpPr><p:spPr/><p:txBody><a:bodyPr/><a:lstStyle/><a:p><a:endParaRPr lang="${NAMES.lang}"/></a:p></p:txBody></p:sp>`;
    })
    .join('');
  deck.setPart(
    part,
    `${HEAD}<p:sld ${ROOTNS}><p:cSld><p:spTree><p:nvGrpSpPr><p:cNvPr id="1" name=""/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr><p:grpSpPr/>${hold}</p:spTree></p:cSld><p:clrMapOvr><a:masterClrMapping/></p:clrMapOvr></p:sld>`
  );
  ensureOverride(deck, part, SLIDE_TYPE);
  addRel(deck, part, 'slideLayout', layoutPath);
  insertId(deck, part, at);
  deck.touched.add('[Content_Types].xml');
  deck.touched.add(deck.presPath);
  return part;
}
export function duplicateSlide(deck, s, at) {
  const part = newSlidePart(deck);
  deck.setPart(part, toXml(s.xml));
  ensureOverride(deck, part, SLIDE_TYPE);
  // same relationships with the same ids, so the copy's words and pictures stay wired up. Notes are its own copy
  const { d: src } = relsDoc(deck, s.part);
  const { d: dst, p } = relsDoc(deck, part);
  for (const r of kids(src)) {
    const c = dst.ownerDocument.importNode(r, true);
    if (/notesSlide$/.test(str(r, 'Type'))) {
      const old = deck.relsOf(s.part).get(str(r, 'Id')).target;
      let n = 1;
      while (deck.xml(`ppt/notesSlides/notesSlide${n}.xml`)) n++;
      const np = `ppt/notesSlides/notesSlide${n}.xml`;
      deck.setPart(np, toXml(deck.xml(old)));
      ensureOverride(deck, np, 'application/vnd.openxmlformats-officedocument.presentationml.notesSlide+xml');
      const oldRels = deck.relsOf(old);
      for (const [id, v] of oldRels) {
        const rn = relsDoc(deck, np).d,
          x = rn.ownerDocument.createElementNS(rn.namespaceURI, 'Relationship');
        x.setAttribute('Id', id);
        x.setAttribute('Type', REL + '/' + v.type);
        x.setAttribute('Target', v.type === 'slide' ? relTarget(np, part) : relTarget(np, v.target));
        rn.appendChild(x);
      }
      c.setAttribute('Target', relTarget(part, np));
    }
    dst.appendChild(c);
  }
  deck.touched.add(p);
  deck.dropRelCache();
  insertId(deck, part, at);
  deck.touched.add('[Content_Types].xml');
  deck.touched.add(deck.presPath);
  return part;
}
export function removeSlide(deck, s) {
  const list = presList(deck),
    rels0 = deck.relsOf(deck.presPath),
    entry = kids(list).find((e) => {
      const r = rels0.get(e.getAttributeNS(NS.r, 'id') || e.getAttribute('r:id'));
      return r && r.target === s.part;
    });
  if (!entry) return;
  const id = entry.getAttributeNS(NS.r, 'id') || entry.getAttribute('r:id'),
    pr = deck.xml(
      deck.presPath.slice(0, deck.presPath.lastIndexOf('/') + 1) + '_rels/' + deck.presPath.split('/').pop() + '.rels'
    );
  for (const r of kids(pr)) if (str(r, 'Id') === id) r.remove();
  entry.remove();
  deck.touched.add(deck.presPath);
  deck.touched.add(pr.ownerDocument && deck.relsPath(deck.presPath));
  deck.dropRelCache();
  const notes = deck.relOfType(s.rels, 'notesSlide');
  if (notes) {
    deck.removePart(notes.target);
    dropOverride(deck, notes.target);
    deck.removePart(deck.relsPath(notes.target));
  }
  deck.removePart(s.part);
  deck.removePart(deck.relsPath(s.part));
  dropOverride(deck, s.part);
  deck.touched.add('[Content_Types].xml');
}
export function moveSlides(deck, order) {
  const list = presList(deck),
    entries = kids(list);
  order.forEach((i) => list.appendChild(entries[i]));
  deck.touched.add(deck.presPath);
}
export function setHidden(deck, s, on) {
  if (on) s.xml.setAttribute('show', '0');
  else s.xml.removeAttribute('show');
  deck.touched.add(s.part);
}

// ---------- notes ----------
const NOTES_TYPE = 'application/vnd.openxmlformats-officedocument.presentationml.notesSlide+xml',
  NM_TYPE = 'application/vnd.openxmlformats-officedocument.presentationml.notesMaster+xml',
  THEME_TYPE = 'application/vnd.openxmlformats-officedocument.theme+xml';
function notesMaster(deck) {
  const have = deck.relOfType(deck.relsOf(deck.presPath), 'notesMaster');
  if (have) return have.target;
  const path = 'ppt/notesMasters/notesMaster1.xml',
    master = deck.slides[0].master,
    themeSrc = deck.relOfType(master.rels, 'theme');
  let n = 1;
  while (deck.hasPart(`ppt/theme/theme${n}.xml`)) n++;
  const themePath = `ppt/theme/theme${n}.xml`;
  deck.setPart(themePath, toXml(deck.xml(themeSrc.target)));
  ensureOverride(deck, themePath, THEME_TYPE);
  deck.setPart(
    path,
    `${HEAD}<p:notesMaster ${ROOTNS}><p:cSld><p:bg><p:bgRef idx="1001"><a:schemeClr val="bg1"/></p:bgRef></p:bg><p:spTree><p:nvGrpSpPr><p:cNvPr id="1" name=""/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr><p:grpSpPr/><p:sp><p:nvSpPr><p:cNvPr id="2" name="${NAMES.notes} 1"/><p:cNvSpPr><a:spLocks noGrp="1"/></p:cNvSpPr><p:nvPr><p:ph type="body" sz="quarter" idx="1"/></p:nvPr></p:nvSpPr><p:spPr><a:xfrm><a:off x="685800" y="4400550"/><a:ext cx="5486400" cy="3600450"/></a:xfrm><a:prstGeom prst="rect"><a:avLst/></a:prstGeom></p:spPr><p:txBody><a:bodyPr/><a:lstStyle/><a:p><a:r><a:rPr lang="${NAMES.lang}"/><a:t>${NAMES.notesWord}</a:t></a:r></a:p></p:txBody></p:sp></p:spTree></p:cSld><p:clrMap bg1="lt1" tx1="dk1" bg2="lt2" tx2="dk2" accent1="accent1" accent2="accent2" accent3="accent3" accent4="accent4" accent5="accent5" accent6="accent6" hlink="hlink" folHlink="folHlink"/></p:notesMaster>`
  );
  ensureOverride(deck, path, NM_TYPE);
  addRel(deck, path, 'theme', themePath);
  const rid = addRel(deck, deck.presPath, 'notesMaster', path),
    pres = deck.xml(deck.presPath),
    list = pres.ownerDocument.createElementNS(NS.p, 'p:notesMasterIdLst'),
    id = pres.ownerDocument.createElementNS(NS.p, 'p:notesMasterId');
  id.setAttributeNS(NS.r, 'r:id', rid);
  list.appendChild(id);
  const after = kid(pres, 'sldMasterIdLst');
  pres.insertBefore(list, after ? after.nextSibling : pres.firstChild);
  deck.touched.add(deck.presPath);
  deck.touched.add('[Content_Types].xml');
  return path;
}
export function setNotes(deck, s, text) {
  const rel = deck.relOfType(deck.relsOf(s.part), 'notesSlide');
  if (!rel && !text.trim()) return false;
  const paras = (text ? text.split('\n') : [''])
    .map((l) =>
      l
        ? `<a:p><a:r><a:rPr lang="${NAMES.lang}"/><a:t>${esc(l)}</a:t></a:r></a:p>`
        : '<a:p><a:endParaRPr lang="${NAMES.lang}"/></a:p>'
    )
    .join('');
  if (rel) {
    const nd = deck.xml(rel.target),
      tree = kid(nd, 'cSld', 'spTree');
    let body = null;
    for (const c of kids(tree)) {
      const nv = kid(nvOf(c), 'nvPr'),
        ph = nv && kid(nv, 'ph');
      if (ph && str(ph, 'type') === 'body') body = c;
    }
    if (!body) {
      body = one(
        tree,
        `<p:sp><p:nvSpPr><p:cNvPr id="3" name="${NAMES.notes} 2"/><p:cNvSpPr><a:spLocks noGrp="1"/></p:cNvSpPr><p:nvPr><p:ph type="body" idx="1"/></p:nvPr></p:nvSpPr><p:spPr/><p:txBody><a:bodyPr/><a:lstStyle/></p:txBody></p:sp>`
      );
      tree.appendChild(body);
    }
    const tx = kid(body, 'txBody');
    kids(tx, 'p').forEach((p) => p.remove());
    frag(tx, paras).forEach((p) => tx.appendChild(p));
    deck.touched.add(rel.target);
    return true;
  }
  const master = notesMaster(deck);
  let n = 1;
  while (deck.hasPart(`ppt/notesSlides/notesSlide${n}.xml`)) n++;
  const np = `ppt/notesSlides/notesSlide${n}.xml`;
  deck.setPart(
    np,
    `${HEAD}<p:notes ${ROOTNS}><p:cSld><p:spTree><p:nvGrpSpPr><p:cNvPr id="1" name=""/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr><p:grpSpPr/><p:sp><p:nvSpPr><p:cNvPr id="2" name="${NAMES.notes} 1"/><p:cNvSpPr><a:spLocks noGrp="1"/></p:cNvSpPr><p:nvPr><p:ph type="body" idx="1"/></p:nvPr></p:nvSpPr><p:spPr/><p:txBody><a:bodyPr/><a:lstStyle/>${paras}</p:txBody></p:sp></p:spTree></p:cSld><p:clrMapOvr><a:masterClrMapping/></p:clrMapOvr></p:notes>`
  );
  ensureOverride(deck, np, NOTES_TYPE);
  addRel(deck, np, 'notesMaster', master);
  addRel(deck, np, 'slide', s.part);
  addRel(deck, s.part, 'notesSlide', np);
  deck.touched.add('[Content_Types].xml');
  deck.touched.add(s.part);
  return true;
}

// ---------- saving ----------
export function saveDeck(deck) {
  const out = {};
  const live = new Set();
  // only what's still referred to goes out; a picture deleted in this session doesn't ride along
  const parts = Object.keys(deck.files);
  for (const p of parts) out[p] = deck.touched.has(p) && deck.xml(p) ? strToU8(toXml(deck.xml(p))) : deck.files[p];
  for (const p of deck.touched) if (p && deck.xml(p)) out[p] = strToU8(toXml(deck.xml(p)));
  void live;
  void unwrap;
  return zipSync(
    Object.fromEntries(
      Object.entries(out).map(([k, v]) => [
        k,
        [v, { level: /\.(png|jpe?g|gif|mp4|m4v|mov|mp3|m4a|wma|wmv|avi)$/i.test(k) ? 0 : 6 }],
      ])
    )
  );
}

// ---------- what kind of file it says it is (DotAny Slides) ----------
// The package's main part names the kind: a presentation, a slide show, a template, or one of those with macros. A pptm is
// only read here; what's saved from it is a presentation, so its main part says so and its macros don't go with it.
const MAIN = {
  pptx: 'application/vnd.openxmlformats-officedocument.presentationml.presentation.main+xml',
  ppsx: 'application/vnd.openxmlformats-officedocument.presentationml.slideshow.main+xml',
  potx: 'application/vnd.openxmlformats-officedocument.presentationml.template.main+xml',
  pptm: 'application/vnd.ms-powerpoint.presentation.macroEnabled.main+xml',
  ppsm: 'application/vnd.ms-powerpoint.slideshow.macroEnabled.main+xml',
  potm: 'application/vnd.ms-powerpoint.template.macroEnabled.main+xml',
};
const mainOverride = (deck) =>
  kids(deck.xml('[Content_Types].xml'), 'Override').find((o) => str(o, 'PartName') === '/' + deck.presPath) || null;
export function mainTypeOf(deck) {
  const o = mainOverride(deck),
    t = o && str(o, 'ContentType');
  for (const k in MAIN) if (MAIN[k] === t) return k;
  return null;
}
export function setMainType(deck, kind) {
  const o = mainOverride(deck);
  if (!o || str(o, 'ContentType') === MAIN[kind]) return;
  o.setAttribute('ContentType', MAIN[kind]);
  deck.touched.add('[Content_Types].xml');
}
// the macros and what only they use: the VBA project, its signatures, and the relationships and types that name them
export function dropMacros(deck) {
  const gone = new Set(),
    walk = (part) => {
      if (gone.has(part) || !deck.hasPart(part)) return;
      gone.add(part);
      for (const r of deck.relsOf(part).values())
        if (!r.external && /^vbaProject|vbaProjectSignature|vbaData/i.test(r.type)) walk(r.target);
    };
  for (const name of Object.keys(deck.files))
    if (/(^|\/)vbaProject[^/]*\.bin$|(^|\/)vbaData\.xml$/i.test(name)) walk(name);
  if (!gone.size) return false;
  for (const p of [deck.presPath, ...gone]) {
    const rp = deck.relsPath(p),
      d = deck.xml(rp);
    if (!d) continue;
    for (const r of kids(d))
      if (/vbaProject|vbaProjectSignature|vbaData|keyMapCustomizations|wordVbaData/i.test(str(r, 'Type') || '')) {
        r.remove();
        deck.touched.add(rp);
      }
  }
  for (const p of gone) {
    deck.removePart(p);
    deck.removePart(deck.relsPath(p));
    dropOverride(deck, p);
  }
  const ct = deck.xml('[Content_Types].xml');
  for (const d of kids(ct, 'Default'))
    if (
      /vbaProject/i.test(str(d, 'ContentType') || '')
      && !Object.keys(deck.files).some((n) => n.toLowerCase().endsWith('.' + (str(d, 'Extension') || '').toLowerCase()))
    )
      d.remove();
  deck.touched.add('[Content_Types].xml');
  deck.dropRelCache();
  return true;
}
