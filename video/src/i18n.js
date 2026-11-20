// DotAny Video's own words in other languages: src/lang/<code>.json, added to the family's (shared/web/lang) through the
// base. A word the family already has keeps the family's: a word that means something else here has a key of its own
// (the word, `::`, then a short tag).
import { addTables } from '@dotany/web/i18n.js';
import en from './lang/en.json';

export { T, lang, setLang } from '@dotany/web/i18n.js';

// English is always here; every other language is loaded when chosen (shared/web/i18n.js LANGS lists them)
export const WORDS = {
  en,
  'zh-Hant': () => import('./lang/zh-Hant.json'),
  ja: () => import('./lang/ja.json'),
  ko: () => import('./lang/ko.json'),
  'es-419': () => import('./lang/es-419.json'),
  'pt-BR': () => import('./lang/pt-BR.json'),
  fr: () => import('./lang/fr.json'),
  de: () => import('./lang/de.json'),
};
addTables(WORDS);
