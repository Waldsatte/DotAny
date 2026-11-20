// .ass and .ssa: the words of each Dialogue line, in the column its Format line names; times are H:MM:SS.cc.
// Only the words are kept, shown in the family's subtitle style: the file's fonts, colours, places and effects aren't.
function readAss(text) {
  const out = [];
  let inEvents = false,
    cols = null;
  const time = (s) => {
    const m = /(\d+):(\d{1,2}):(\d{1,2})[.:](\d{1,3})/.exec(s);
    return m ? +m[1] * 3600 + +m[2] * 60 + +m[3] + +('0.' + m[4]) : NaN;
  };
  for (const line of text.split('\n')) {
    const l = line.trim();
    if (/^\[.*\]$/.test(l)) {
      inEvents = /^\[events\]$/i.test(l);
      cols = null;
      continue;
    }
    if (!inEvents) continue;
    const at = l.indexOf(':');
    if (at < 0) continue;
    const kind = l.slice(0, at).trim().toLowerCase(),
      rest = l.slice(at + 1).trim();
    if (kind === 'format') {
      cols = rest.split(',').map((c) => c.trim().toLowerCase());
      continue;
    }
    if (kind !== 'dialogue' || !cols) continue;
    // the words are the last column and may hold commas of their own
    const parts = rest.split(','),
      field = (name) => parts[cols.indexOf(name)] || '';
    const raw = parts.slice(cols.length - 1).join(',');
    // a drawing ({\p1} … {\p0}) is a shape, not words; other {…} are styling; \N is a new line, \h a space that doesn't break
    const words = raw
      .replace(/\{[^}]*\\p[1-9][^}]*\}.*?(\{[^}]*\\p0[^}]*\}|$)/g, '')
      .replace(/\{[^}]*\}/g, '')
      .replace(/\\[Nn]/g, '\n')
      .replace(/\\h/g, ' ')
      .split('\n')
      .map((s) => s.trim())
      .filter(Boolean)
      .join('\n');
    const a = time(field('start')),
      b = time(field('end'));
    if (words && Number.isFinite(a) && Number.isFinite(b) && b > a) out.push({ a, b, words });
  }
  // lines on screen at the same time, often a sign above the speech, show together, one under the other
  const merged = [];
  for (const c of out.sort((x, y) => x.a - y.a || x.b - y.b)) {
    const same = merged.find((m) => m.a === c.a && m.b === c.b);
    if (!same) merged.push({ ...c });
    else if (!same.words.split('\n').includes(c.words)) same.words += '\n' + c.words;
  }
  return merged;
}

// subtitle files: .srt and .vtt are close enough to read with one routine; .ass and .ssa have their own
export function readSubtitles(text) {
  const plain = text.replace(/^﻿/, '').replace(/\r/g, '');
  if (/^\s*\[script info\]/i.test(plain) || /^\[events\]\s*$/im.test(plain)) return readAss(plain);
  const out = [],
    stamp = /(\d{1,2}:)?(\d{1,2}):(\d{2})[.,](\d{1,3})/g;
  const seconds = (s) => {
    const m = s.match(/(?:(\d{1,2}):)?(\d{1,2}):(\d{2})[.,](\d{1,3})/);
    return +(m[1] || 0) * 3600 + +m[2] * 60 + +m[3] + +m[4].padEnd(3, '0') / 1000;
  };
  for (const block of plain.split(/\n{2,}/)) {
    const lines = block.split('\n'),
      at = lines.findIndex((l) => l.includes('-->'));
    if (at < 0) continue;
    const times = lines[at].match(stamp);
    if (!times || times.length < 2) continue;
    const words = lines
      .slice(at + 1)
      .join('\n')
      .replace(/<[^>]+>/g, '')
      .trim();
    if (words) out.push({ a: seconds(times[0]), b: seconds(times[1]), words });
  }
  return out.sort((x, y) => x.a - y.a);
}

// a subtitle file names no encoding: a byte order mark decides, then UTF-8 if it holds, then the common Chinese one
export function subtitleText(bytes) {
  if (bytes[0] === 0xff && bytes[1] === 0xfe) return new TextDecoder('utf-16le').decode(bytes);
  if (bytes[0] === 0xfe && bytes[1] === 0xff) return new TextDecoder('utf-16be').decode(bytes);
  try {
    return new TextDecoder('utf-8', { fatal: true }).decode(bytes);
  } catch {
    return new TextDecoder('gbk').decode(bytes);
  }
}
