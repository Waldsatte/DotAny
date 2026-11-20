import { renderKeys } from './keys.js';
import FAMILY_EN from './lang/en.json';

// ---------- language: Chinese is the source, every other language is a table looked up from it ----------
// The Chinese written in the code is the key and is what Chinese shows: T('另存为'). Each other language is a table of its
// own, Chinese to that language: the family's words in shared/web/lang/<code>.json, a program's own in its
// src/lang/<code>.json. A word missing from a table falls back to English, then to the Chinese.
//
// In a key:
// - {Mod+S}: a shortcut, written as the keyboard says it (keys.js).
// - {n}, {name}: a value the code passes, T('删除全部 {n} 段录音？', { n }). A number is written with the language's
//   decimal sign and no grouping, as String(n) writes it in English; pass num(n) for 12,345.
// - {n, plural, one {# page} other {# pages}}: a count, in a translation (Chinese never needs it); the forms are
//   Intl.PluralRules' (zero, one, two, few, many, other), # is the number. =0 {…} matches the number itself.
// - 关闭::混响: the same Chinese meaning something else (reverb's 关闭 is Off, the window's 关闭 is Close). Chinese shows
//   the part before ::, the part after tells the translator where it is.
//
// To add a language: a line in LANGS, shared/web/lang/<code>.json and src/lang/<code>.json beside each program's en.json,
// each loaded when chosen (`() => import('./lang/ja.json')`), so a language nobody picks costs nothing.

/** The languages the family speaks, in the order Settings › Language lists them. name: in its own language, never
 *  translated; html: <html lang>; locale: what Intl formats numbers, dates and plurals with; words: the family's table
 *  (zh has none). */
export const LANGS = [
  { code: 'zh', name: '简体中文', html: 'zh-CN', locale: 'zh-CN' },
  {
    code: 'zh-Hant',
    name: '繁體中文',
    html: 'zh-Hant-TW',
    locale: 'zh-TW',
    words: () => import('./lang/zh-Hant.json'),
  },
  { code: 'en', name: 'English', html: 'en', locale: 'en-US', words: FAMILY_EN },
  { code: 'ja', name: '日本語', html: 'ja', locale: 'ja-JP', words: () => import('./lang/ja.json') },
  { code: 'ko', name: '한국어', html: 'ko', locale: 'ko-KR', words: () => import('./lang/ko.json') },
  // one Spanish and one Portuguese for everyone who speaks them: Latin American Spanish, Brazilian Portuguese
  { code: 'es-419', name: 'Español', html: 'es-419', locale: 'es-419', words: () => import('./lang/es-419.json') },
  { code: 'pt-BR', name: 'Português', html: 'pt-BR', locale: 'pt-BR', words: () => import('./lang/pt-BR.json') },
  { code: 'fr', name: 'Français', html: 'fr', locale: 'fr-FR', words: () => import('./lang/fr.json') },
  { code: 'de', name: 'Deutsch', html: 'de', locale: 'de-DE', words: () => import('./lang/de.json') },
];

// code -> the words loaded so far (the family's, then each program's); a loader waits in `later` until the language is chosen
const TABLES = { en: {} };
const later = {};
const add = (code, words) => {
  const t = TABLES[code] || (TABLES[code] = {});
  // a word the family already has keeps the family's (the check script keeps program tables apart from it)
  for (const [z, w] of Object.entries(words)) if (!(z in t)) t[z] = w;
};
/** A program's tables (and the family's, below): { en: {…}, ja: () => import('./lang/ja.json') }. */
export function addTables(tables) {
  for (const [code, words] of Object.entries(tables || {})) {
    if (typeof words === 'function') (later[code] || (later[code] = [])).push(words);
    else add(code, words);
  }
}
for (const l of LANGS) if (l.words) addTables({ [l.code]: l.words });
/** Kept for the older call: a program's own English. */
export const addWords = (words) => addTables({ en: words });

const info = (code) => LANGS.find((l) => l.code === code) || LANGS[0];
export let lang = 'zh';
export const setLang = (v) => {
  lang = info(v).code;
};
/** The language on screen, as LANGS has it. */
export const langInfo = () => info(lang);
/** Whether the language's words are all here (a table chosen for the first time is still coming). */
export const langReady = (code = lang) => !(later[code] && later[code].length) && !loading[code];
const loading = {};
/** Loads what the language still waits for; resolves once T can say it (one load, however many ask). */
export function loadLang(code = lang) {
  if (loading[code]) return loading[code];
  const wait = later[code] || [];
  if (!wait.length) return Promise.resolve();
  later[code] = [];
  return (loading[code] = Promise.all(wait.map((f) => f())).then((ms) => {
    for (const m of ms) add(code, m.default || m);
    delete loading[code];
  }));
}

/**
 * The language System means: the first of the system's languages (navigator.languages, then navigator.language) the
 * family has a table for, English when none. Chinese written in Traditional characters (zh-TW, zh-HK, zh-MO, zh-Hant) takes
 * zh-Hant, other Chinese (zh-CN, zh-SG, zh-Hans) Simplified; every other language goes by its first part, so es-ES and
 * es-MX take es-419 and pt-PT takes pt-BR. A stored choice the family no longer has counts as System.
 */
export function pickLang(
  stored,
  prefs = (typeof navigator !== 'undefined'
    && (navigator.languages && navigator.languages.length ? navigator.languages : [navigator.language]))
    || []
) {
  if (forced) return forced;
  if (stored && LANGS.some((l) => l.code === stored)) return stored;
  const has = (c) => LANGS.some((l) => l.code === c);
  for (const p of prefs) {
    const t = String(p || '').toLowerCase();
    if (!t) continue;
    const exact = LANGS.find((l) => l.code.toLowerCase() === t || l.locale.toLowerCase() === t);
    if (exact) return exact.code;
    const base = t.split('-')[0];
    if (base === 'zh') return /-(tw|hk|mo|hant)\b/.test(t) && !/-hans\b/.test(t) && has('zh-Hant') ? 'zh-Hant' : 'zh';
    const same = LANGS.find((l) => l.code.toLowerCase().split('-')[0] === base);
    if (same) return same.code;
  }
  return 'en';
}

// ---------- looking a word up ----------
const shown = (z) => {
  const i = z.indexOf('::');
  return i < 0 ? z : z.slice(0, i);
};
const lookup = (z, code) => (code === 'zh' ? undefined : ((TABLES[code] && TABLES[code][z]) ?? TABLES.en[z]));
/** The words for a key in the language on screen, as the table has them (no values filled in): English, then Chinese. */
export const word = (z, code = lang) => lookup(z, code) ?? shown(z);

const plurals = {};
const pluralOf = (n) => (plurals[lang] || (plurals[lang] = new Intl.PluralRules(langInfo().locale))).select(n);
const numbers = {};
/** A number the language's way: 12,345 · 12 345 · 12.345. */
export const num = (n, opts) => {
  const k = lang + JSON.stringify(opts || {});
  return (numbers[k] || (numbers[k] = new Intl.NumberFormat(langInfo().locale, opts))).format(n);
};
const PLAIN = { useGrouping: false, maximumFractionDigits: 20 };
const valueOf = (v) => (typeof v === 'number' ? num(v, PLAIN) : String(v));
// {n, plural, one {…} other {…}}, with nested braces; # inside is the number
function choose(s, vars) {
  let out = '',
    i = 0;
  for (;;) {
    const at = s.indexOf(', plural,', i);
    if (at < 0) return out + s.slice(i);
    const open = s.lastIndexOf('{', at),
      name = s.slice(open + 1, at).trim();
    let j = at + 9,
      depth = 1;
    const forms = {};
    while (depth > 0 && j < s.length) {
      while (s[j] === ' ') j++;
      if (s[j] === '}') {
        depth--;
        j++;
        break;
      }
      const sel = /^(=?\w+)\s*\{/.exec(s.slice(j));
      if (!sel) break;
      j += sel[0].length;
      let d = 1,
        k = j;
      for (; k < s.length && d; k++) {
        if (s[k] === '{') d++;
        else if (s[k] === '}') d--;
      }
      forms[sel[1]] = s.slice(j, k - 1);
      j = k;
    }
    // a count the program already wrote out with num() ("1,234", German "1.234") is read by its digits and shown as given
    const raw = vars[name],
      n = typeof raw === 'number' ? raw : Number(String(raw).replace(/\D/g, ''));
    const form = forms['=' + n] ?? forms[pluralOf(n)] ?? forms.other ?? '';
    out += s.slice(i, open) + form.replace(/#/g, typeof raw === 'number' ? num(n, PLAIN) : String(raw));
    i = j;
  }
}
/** The words for a Chinese key in the language on screen, with its values filled in. */
export function T(z, vars) {
  let s = renderKeys(word(z));
  if (!vars) return s;
  if (s.includes(', plural,')) s = choose(s, vars);
  return s.replace(/\{(\w+)\}/g, (m, k) => (k in vars ? valueOf(vars[k]) : m));
}
/** Every way the loaded languages say a key (未命名, Untitled, …): for a name the program gave in another language. */
export const allWords = (z) => [
  ...new Set([
    shown(z),
    ...Object.values(TABLES)
      .map((t) => t[z])
      .filter(Boolean),
  ]),
];

// ---------- sizes and dates the language's way ----------
/**
 * A file size: 840 KB, 1.5 MB, 2.1 GB (1,5 MB in German). The family's programs differ in small ways, kept as they were:
 * zero: '0 KB' for nothing; gb: GB from 1 GB; wholeFrom: whole MB from this size up (Image: 12 MB from 10 MB).
 */
export function size(b, { zero = false, gb = false, wholeFrom = Infinity } = {}) {
  if (zero && !b) return num(0) + ' KB';
  const fixed = (x, d) => num(x, { minimumFractionDigits: d, maximumFractionDigits: d, useGrouping: false });
  if (gb && b >= 1073741824) return fixed(b / 1073741824, 1) + ' GB';
  if (b >= 1048576) return fixed(b / 1048576, b >= wholeFrom ? 0 : 1) + ' MB';
  return fixed(Math.max(1, Math.round(b / 1024)), 0) + ' KB';
}
/** A date the language's way (Intl options). Chinese keeps a space between its numbers and characters: 10 月 30 日. */
export function date(d, opts) {
  const f = new Intl.DateTimeFormat(langInfo().locale, opts);
  if (!/^(zh|ja)/.test(lang)) return f.format(d);
  return f
    .formatToParts(d)
    .map((p, i, all) => {
      if (p.type !== 'literal') return p.value;
      const before = all[i - 1],
        after = all[i + 1];
      return (
        (before && /\d$/.test(before.value) ? ' ' : '') + p.value.trim() + (after && /^\d/.test(after.value) ? ' ' : '')
      );
    })
    .join('');
}

// ---------- with DOTANY_TEST set: made-up languages that find where the layout doesn't adapt yet ----------
// The layout adapts to the sentence; translations are never cut to fit. A test can switch a running window to these
// and list every word that is cut, spills out of its box, leaves the window or runs into its neighbour.
// - x-long: the English with every word about 40% longer and accented (Säävé Äss), so no line gets more places to
//   break: as long as German or French get, and longer.
// - x-tall: the Japanese between tall letters above and below (ẶǺ…ģỵ), for words cut off at the top or bottom.
// Each word starts with an invisible word joiner (U+2060), so the check tells the program's words from a file's own.
// They are never listed in Settings › Language.
let forced = null;
const ACCENT = { a: 'ä', e: 'é', i: 'ï', o: 'ö', u: 'ü', A: 'Ä', E: 'É', I: 'Ï', O: 'Ö', U: 'Ü' };
/** The English made about 40% longer, its {values}, {shortcuts} and plural syntax kept (plural forms lengthened too). */
export function longer(s) {
  let out = '',
    i = 0,
    n = 0;
  while (i < s.length) {
    if (s[i] === '{') {
      let d = 1,
        k = i + 1;
      for (; k < s.length && d; k++) {
        if (s[k] === '{') d++;
        else if (s[k] === '}') d--;
      }
      const inner = s.slice(i + 1, k - 1),
        head = /^\s*\w+\s*,\s*plural\s*,/.exec(inner);
      if (!head) {
        out += s.slice(i, k);
        i = k;
        continue;
      }
      let rest = inner.slice(head[0].length),
        body = '';
      for (;;) {
        const sel = /^(\s*=?\w+\s*)\{/.exec(rest);
        if (!sel) {
          body += rest;
          break;
        }
        let e = 1,
          j = sel[0].length;
        for (; j < rest.length && e; j++) {
          if (rest[j] === '{') e++;
          else if (rest[j] === '}') e--;
        }
        body += sel[0] + longer(rest.slice(sel[0].length, j - 1)) + '}';
        rest = rest.slice(j);
      }
      out += '{' + head[0] + body + '}';
      i = k;
      continue;
    }
    const c = s[i++];
    if (/[A-Za-z]/.test(c)) {
      n++;
      out += ACCENT[c] || c;
      if (n % 2 === 0) out += c.toLowerCase();
    } else {
      out += c;
    }
  }
  return out;
}
function addTestLanguages() {
  if (LANGS.some((l) => l.code === 'x-long')) return;
  LANGS.push(
    { code: 'x-long', name: 'x-long', html: 'de', locale: 'en-US', test: true },
    { code: 'x-tall', name: 'x-tall', html: 'ja', locale: 'ja-JP', test: true }
  );
  addTables({
    'x-long': async () => Object.fromEntries(Object.entries(TABLES.en).map(([k, v]) => [k, '\u2060' + longer(v)])),
    'x-tall': async () => {
      await loadLang('ja');
      return Object.fromEntries(
        Object.entries({ ...TABLES.en, ...TABLES.ja }).map(([k, v]) => [k, '\u2060ẶǺ' + v + 'ģỵ'])
      );
    },
  });
}
/** Shows the window in x-long or x-tall (or any language) whatever is stored; null goes back. The window's own
 *  applyLang() draws it. */
export async function testLanguage(code) {
  addTestLanguages();
  forced = code || null;
  if (forced) await loadLang(forced);
}

// The checks need every key; tools read these where they did
export const hasEnglish = (z) => z in TABLES.en;
export const allChinese = () => Object.keys(TABLES.en);
/** Whether the language's words run long, as English's do (a row of chips fits fewer): every language not written in
 *  Chinese characters, kana or Hangul. */
export const longWords = () => !/^(zh|ja|ko)/.test(lang);
