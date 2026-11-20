// the common charts, drawn from the numbers the file keeps with them
import { kids, kid, num, str } from './deck.js';
import { colorOf, css, fillFrom } from './paint.js';

const clamp = (v, a, b) => Math.min(b, Math.max(a, v));
const SVGNS = 'http://www.w3.org/2000/svg';
const svg = (t, a = {}, text) => {
  const e = document.createElementNS(SVGNS, t);
  for (const k in a) e.setAttribute(k, a[k]);
  if (text != null) e.textContent = text;
  return e;
};
const cacheOf = (n) => {
  if (!n) return [];
  const c = n.getElementsByTagNameNS('*', 'pt'),
    out = [];
  for (const p of c) out[num(p, 'idx', 0)] = kid(p, 'v') ? kid(p, 'v').textContent : '';
  return out;
};
function nice(max) {
  if (max <= 0) return 1;
  const e = Math.pow(10, Math.floor(Math.log10(max))),
    f = max / e;
  return (f <= 1 ? 1 : f <= 2 ? 2 : f <= 5 ? 5 : 10) * e;
}
const textW = (t, size) => {
  let w = 0;
  for (const ch of String(t || '')) w += ch.charCodeAt(0) > 255 ? 1 : 0.58;
  return w * size;
};
const short = (v) =>
  Math.abs(v) >= 1000 ? v.toLocaleString('en-US', { maximumFractionDigits: 1 }) : String(+(+v).toFixed(2));
const richText = (n) => (n ? [...n.getElementsByTagNameNS('*', 't')].map((t) => t.textContent).join('') : '');

export function drawChart(host, root, ctx0, box) {
  let ctx = ctx0;
  const chart = kid(root, 'chart'),
    plot = kid(chart, 'plotArea');
  if (!plot) return;
  const sizeIn = (n) => {
    const d = n && kid(n, 'txPr'),
      r = d && d.getElementsByTagNameNS('*', 'defRPr')[0];
    return r && num(r, 'sz') ? num(r, 'sz') / 100 : 0;
  };
  const baseSize = sizeIn(root) || 10,
    defSize = baseSize;
  const textC = (() => {
    const d = kid(root, 'txPr'),
      r = d && d.getElementsByTagNameNS('*', 'defRPr')[0],
      f = r && kid(r, 'solidFill'),
      c = f ? colorOf(f, ctx) : { rgb: [89, 89, 89], a: 1 };
    return css(c);
  })();
  const face =
    ctx.theme.minor && (ctx.theme.minor.ea || ctx.theme.minor.latin)
      ? `"${ctx.theme.minor.latin}","${ctx.theme.minor.ea}","Segoe UI","Microsoft YaHei",sans-serif`
      : '"Segoe UI","Microsoft YaHei",sans-serif';
  const s = svg('svg', {
    class: 'x-g',
    width: box.w,
    height: box.h,
    'font-size': defSize,
    'font-family': face,
    fill: textC,
  });
  host.appendChild(s);
  const spPr = kid(root, 'spPr'),
    bg = spPr && fillFrom(spPr, ctx);
  if (bg && bg.color) s.appendChild(svg('rect', { width: box.w, height: box.h, fill: css(bg.color) }));
  const over = [...ctx.rels.values()].find((r) => r.type === 'themeOverride'),
    oc = over && ctx.deck.xml(over.target),
    scheme = oc && kid(oc, 'clrScheme');
  if (scheme) {
    const colors = { ...ctx.theme.colors };
    for (const x of kids(scheme)) {
      const v = kids(x)[0];
      if (v) colors[x.localName] = str(v, 'lastClr') || str(v, 'val');
    }
    ctx = { ...ctx, theme: { ...ctx.theme, colors } };
  }
  const plotPr = kid(plot, 'spPr'),
    plotBg = plotPr && fillFrom(plotPr, ctx);
  const styleNode = [...root.getElementsByTagNameNS('*', 'style')].find(
      (x) =>
        x.parentElement === root || x.parentElement.localName === 'Fallback' || x.parentElement.localName === 'Choice'
    ),
    styleNo = styleNode ? (num(styleNode, 'val', 2) - 1) % 8 : 1;
  const allSer = kids(plot)
    .filter((c) => /Chart$/.test(c.localName))
    .reduce((a, t) => a + kids(t, 'ser').length, 0);
  const styled = (i) => {
    if (styleNo === 1) return hexOf(ctx, 'accent' + ((i % 6) + 1));
    const base = styleNo === 0 ? [128, 128, 128] : hexOf(ctx, 'accent' + (styleNo - 1)),
      t = allSer > 1 ? i / (allSer - 1) : 0.3,
      k = t < 0.4 ? 0.6 + t : t;
    return t < 0.4 ? base.map((v) => v * k) : base.map((v) => v + (255 - v) * (t - 0.4) * 0.95);
  };
  const types = kids(plot).filter((c) => /Chart$/.test(c.localName)),
    series = [];
  let colorAt = 0;
  for (const t of types)
    for (const ser of kids(t, 'ser')) {
      const name = cacheOf(kid(ser, 'tx'))[0] || '',
        cats = cacheOf(kid(ser, 'cat')),
        vals = cacheOf(kid(ser, 'val')).map((v) => +v || 0),
        pr = kid(ser, 'spPr'),
        f = pr && fillFrom(pr, ctx),
        ln = pr && kid(pr, 'ln');
      const accent = { rgb: styled(colorAt), a: 1 };
      colorAt++;
      const lineC = ln && kid(ln, 'solidFill') ? colorOf(kid(ln, 'solidFill'), ctx) : null;
      const points = {};
      for (const d of kids(ser, 'dPt')) {
        const q = kid(d, 'spPr'),
          ff = q && fillFrom(q, ctx);
        if (ff && ff.color) points[num(kid(d, 'idx'), 'val', 0)] = ff.color;
      }
      series.push({
        type: t.localName.replace('3D', ''),
        name,
        cats,
        vals,
        color: f && f.color ? f.color : lineC || accent,
        points,
      });
    }
  if (!series.length) return;
  const styled2 = (i, n) => {
    if (styleNo === 1) return hexOf(ctx, 'accent' + ((i % 6) + 1));
    const base = styleNo === 0 ? [128, 128, 128] : hexOf(ctx, 'accent' + (styleNo - 1)),
      t = n > 1 ? i / (n - 1) : 0.3;
    return base.map((v) => (t < 0.4 ? v * (0.6 + t) : v + (255 - v) * (t - 0.4) * 0.95));
  };
  const titleN = kid(chart, 'title'),
    auto = kid(chart, 'autoTitleDeleted'),
    titleText = titleN
      ? richText(titleN) || (series.length === 1 ? series[0].name : '')
      : series.length === 1 && types[0].localName.match(/pie|doughnut/i) && num(auto, 'val', 0) !== 1
        ? series[0].name
        : '';
  let top = 8,
    bottom = box.h - 8,
    left = 8,
    right = box.w - 8;
  if (titleText) {
    const size = defSize * 1.4;
    s.appendChild(svg('text', { x: box.w / 2, y: top + size, 'text-anchor': 'middle', 'font-size': size }, titleText));
    top += size * 1.7;
  }
  const first = series[0],
    round = first.type === 'pieChart' || first.type === 'doughnutChart',
    cats = first.cats.length ? first.cats : first.vals.map((_, i) => String(i + 1));
  const legendN = kid(chart, 'legend');
  if (legendN) {
    const defSize = sizeIn(legendN) || baseSize,
      legendG = svg('g', { 'font-size': defSize });
    s.appendChild(legendG);
    const pos = str(kid(legendN, 'legendPos'), 'val') || 'r',
      items = round
        ? cats.map((c, i) => [c, first.points[i] || { rgb: styled2(i, cats.length), a: 1 }])
        : series.map((x) => [x.name, x.color]);
    if (pos === 'r' || pos === 'l') {
      const w = Math.min(box.w * 0.3, defSize * 1.6 + Math.max(...items.map(([t]) => textW(t, defSize))));
      let y = (top + bottom) / 2 - items.length * defSize * 0.75;
      const x = pos === 'r' ? right - w : left;
      for (const [t, c] of items) {
        legendG.appendChild(svg('rect', { x, y, width: defSize * 0.7, height: defSize * 0.7, fill: css(c) }));
        legendG.appendChild(svg('text', { x: x + defSize, y: y + defSize * 0.68 }, t));
        y += defSize * 1.5;
      }
      if (pos === 'r') right -= w + 8;
      else left += w + 8;
    } else {
      const widths = items.map(([t]) => defSize * 2.2 + textW(t, defSize));
      let x = box.w / 2 - widths.reduce((a, b) => a + b, 0) / 2;
      const y = pos === 'b' ? bottom - defSize : top;
      items.forEach(([t, c], i) => {
        legendG.appendChild(
          svg('rect', { x, y: y + defSize * 0.1, width: defSize * 0.7, height: defSize * 0.7, fill: css(c) })
        );
        legendG.appendChild(svg('text', { x: x + defSize, y: y + defSize * 0.78 }, t));
        x += widths[i];
      });
      if (pos === 'b') bottom -= defSize * 1.8;
      else top += defSize * 1.8;
    }
  }
  if (round) {
    const total = first.vals.reduce((a, b) => a + b, 0) || 1,
      cx = (left + right) / 2,
      cy = (top + bottom) / 2,
      R = (Math.min(right - left, bottom - top) / 2 - 4) * 0.9,
      hole =
        first.type === 'doughnutChart' ? (R * num(kid(kid(plot, 'doughnutChart'), 'holeSize'), 'val', 50)) / 100 : 0;
    const showPct =
      !!first
      && [
        ...kids(plot)
          .filter((x) => /(pie|doughnut)/i.test(x.localName))[0]
          .getElementsByTagNameNS('*', 'showPercent'),
      ].some((x) => num(x, 'val', 0) === 1);
    let a = -Math.PI / 2;
    first.vals.forEach((v, i) => {
      const sw = (v / total) * Math.PI * 2,
        b = a + sw,
        big = sw > Math.PI ? 1 : 0,
        P = (t, r) => `${cx + Math.cos(t) * r} ${cy + Math.sin(t) * r}`,
        col = first.points[i] || { rgb: styled2(i, first.vals.length), a: 1 };
      const d = hole
        ? `M${P(a, R)}A${R} ${R} 0 ${big} 1 ${P(b, R)}L${P(b, hole)}A${hole} ${hole} 0 ${big} 0 ${P(a, hole)}Z`
        : `M${cx} ${cy}L${P(a, R)}A${R} ${R} 0 ${big} 1 ${P(b, R)}Z`;
      s.appendChild(svg('path', { d, fill: css(col), stroke: '#fff', 'stroke-width': 1 }));
      if (showPct && v / total > 0.03) {
        const m = a + sw / 2,
          r = hole ? (R + hole) / 2 : R * 0.68;
        s.appendChild(
          svg(
            'text',
            { x: cx + Math.cos(m) * r, y: cy + Math.sin(m) * r + defSize * 0.35, 'text-anchor': 'middle' },
            Math.round((v / total) * 100) + '%'
          )
        );
      }
      a = b;
    });
    return;
  }
  const barN = kid(plot, 'barChart') || kid(plot, 'bar3DChart'),
    horizontal = barN && str(kid(barN, 'barDir'), 'val') === 'bar',
    grouping = barN ? str(kid(barN, 'grouping'), 'val') : '',
    stacked = grouping === 'stacked' || grouping === 'percentStacked',
    percent = grouping === 'percentStacked';
  const bars = series.filter((x) => x.type === 'barChart'),
    n = cats.length;
  const totals = cats.map((_, i) => bars.reduce((a, x) => a + (x.vals[i] || 0), 0));
  let hi = percent ? 100 : Math.max(0, ...series.map((x) => Math.max(0, ...x.vals)), ...(stacked ? totals : [0])),
    lo = Math.min(0, ...series.map((x) => Math.min(...x.vals)));
  const span = Math.max(1, (horizontal ? box.w : box.h) - 90),
    fits = clamp(span / (defSize * (horizontal ? 5 : 3)), 3, 10),
    head = percent ? 0 : (hi - lo) * 0.05,
    step = nice((hi + head - lo || 1) / fits);
  hi = percent ? 100 : Math.ceil((hi + head) / step) * step;
  lo = Math.floor(lo / step) * step;
  const catAx = kid(plot, 'catAx') || kid(plot, 'dateAx'),
    valAx = kid(plot, 'valAx'),
    catSize = sizeIn(catAx) || baseSize,
    valSize = sizeIn(valAx) || baseSize,
    off = (n) => n && num(kid(n, 'delete'), 'val', 0) === 1;
  const labelW = off(valAx) ? 0 : Math.max(String(short(hi)).length, String(short(lo)).length) * valSize * 0.62 + 6;
  const axisColor = 'rgba(128,128,128,.35)';
  if (horizontal) {
    left += Math.max(...cats.map((c) => textW(c, catSize))) + 6;
    bottom -= valSize * 1.5;
  } else {
    left += labelW;
    bottom -= off(catAx) ? 0 : catSize * 1.5;
  }
  if (plotBg && plotBg.color)
    s.appendChild(svg('rect', { x: left, y: top, width: right - left, height: bottom - top, fill: css(plotBg.color) }));
  const W = right - left,
    H = bottom - top,
    Y = (v) => bottom - ((v - lo) / (hi - lo)) * H,
    X = (v) => left + ((v - lo) / (hi - lo)) * W;
  for (let v = lo; v <= hi + step / 2; v += step) {
    if (horizontal) {
      s.appendChild(svg('line', { x1: X(v), x2: X(v), y1: top, y2: bottom, stroke: axisColor, 'stroke-width': 0.75 }));
      if (!off(valAx))
        s.appendChild(
          svg('text', { x: X(v), y: bottom + valSize * 1.2, 'text-anchor': 'middle', 'font-size': valSize }, short(v))
        );
    } else {
      s.appendChild(svg('line', { x1: left, x2: right, y1: Y(v), y2: Y(v), stroke: axisColor, 'stroke-width': 0.75 }));
      if (!off(valAx))
        s.appendChild(
          svg('text', { x: left - 5, y: Y(v) + valSize * 0.35, 'text-anchor': 'end', 'font-size': valSize }, short(v))
        );
    }
  }
  const band = (horizontal ? H : W) / Math.max(1, n);
  const gapPct = barN ? num(kid(barN, 'gapWidth'), 'val', 150) / 100 : 1;
  if (!off(catAx))
    cats.forEach((c, i) => {
      if (horizontal)
        s.appendChild(
          svg(
            'text',
            { x: left - 5, y: top + band * (i + 0.5) + catSize * 0.35, 'text-anchor': 'end', 'font-size': catSize },
            c
          )
        );
      else if (n <= 40)
        s.appendChild(
          svg(
            'text',
            { x: left + band * (i + 0.5), y: bottom + catSize * 1.2, 'text-anchor': 'middle', 'font-size': catSize },
            c
          )
        );
    });
  if (bars.length) {
    const showVals = [...barN.getElementsByTagNameNS('*', 'showVal')].some((x) => num(x, 'val', 0) === 1);
    const groups = stacked ? 1 : bars.length,
      thick = band / (groups + gapPct),
      base = cats.map(() => 0);
    bars.forEach((x, si) =>
      x.vals.forEach((v0, i) => {
        const v = percent ? (totals[i] ? (v0 / totals[i]) * 100 : 0) : v0,
          from = stacked ? base[i] : 0,
          to = from + v;
        if (stacked) base[i] = to;
        const shift = (band - thick * groups) / 2 + (stacked ? 0 : si * thick),
          col = x.points[i] || x.color;
        if (horizontal) {
          const j = n - 1 - i;
          void j;
          s.appendChild(
            svg('rect', {
              x: X(Math.min(from, to)),
              y: top + band * i + shift,
              width: Math.abs(X(to) - X(from)),
              height: thick,
              fill: css(col),
            })
          );
        } else {
          s.appendChild(
            svg('rect', {
              x: left + band * i + shift,
              y: Y(Math.max(from, to)),
              width: thick,
              height: Math.abs(Y(to) - Y(from)),
              fill: css(col),
            })
          );
          if (showVals)
            s.appendChild(
              svg(
                'text',
                {
                  x: left + band * i + shift + thick / 2,
                  y: Y(Math.max(from, to)) - 3,
                  'text-anchor': 'middle',
                  'font-size': baseSize * 0.9,
                },
                short(v0)
              )
            );
        }
      })
    );
  }
  for (const x of series) {
    if (x.type !== 'lineChart' && x.type !== 'areaChart') continue;
    const pts = x.vals.map((v, i) => [left + band * (i + 0.5), Y(v)]);
    if (x.type === 'areaChart')
      s.appendChild(
        svg('path', {
          d: `M${pts[0][0]} ${bottom}L${pts.map((p) => p.join(' ')).join('L')}L${pts[pts.length - 1][0]} ${bottom}Z`,
          fill: css(x.color),
          'fill-opacity': 0.75,
        })
      );
    else {
      s.appendChild(
        svg('path', {
          d: 'M' + pts.map((p) => p.join(' ')).join('L'),
          fill: 'none',
          stroke: css(x.color),
          'stroke-width': 2.25,
          'stroke-linejoin': 'round',
        })
      );
      pts.forEach((p) => s.appendChild(svg('circle', { cx: p[0], cy: p[1], r: 2.5, fill: css(x.color) })));
    }
  }
}
function hexOf(ctx, name) {
  const h = ctx.theme.colors[name] || '808080';
  return [parseInt(h.slice(0, 2), 16), parseInt(h.slice(2, 4), 16), parseInt(h.slice(4, 6), 16)];
}
