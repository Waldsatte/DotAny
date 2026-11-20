// Shortcuts: code binds "Mod" and every label comes from here, so Ctrl+S can become ⌘S in one place.
// In copy a shortcut is written as a token, {Mod+S}; the translator turns tokens into labels.
export const platform = /mac/i.test(navigator.platform)
  ? 'mac'
  : /linux/i.test(navigator.platform)
    ? 'linux'
    : 'windows';
const mac = platform === 'mac';

// The Mac writes keys as its menus do: symbols, no "+", modifiers always in the order ⌃ ⌥ ⇧ ⌘.
const NAMES = mac
  ? {
      Mod: '⌘',
      Ctrl: '⌃',
      Shift: '⇧',
      Alt: '⌥',
      Enter: '↩',
      Esc: '⎋',
      Escape: '⎋',
      Backspace: '⌫',
      Delete: '⌦',
      Tab: '⇥',
      Left: '←',
      Right: '→',
      Up: '↑',
      Down: '↓',
    }
  : { Mod: 'Ctrl', Shift: 'Shift', Alt: 'Alt' };
const ORDER = ['Ctrl', 'Alt', 'Shift', 'Mod'];

// Shortcuts that are a different key on the Mac, not only a different name: copy keeps writing Windows' key and the Mac
// shows its own. Redo is ⇧⌘Z there (⌘Y is not redo); presenting from the current slide is ⇧⌘↩, as in the Mac's own
// presentation programs.
const MAC_KEY = { 'Mod+Y': 'Mod+Shift+Z', 'Shift+F5': 'Mod+Shift+Enter' };

/** On Windows and Linux the keys as written; on the Mac in the menus' order, its own key where it differs. */
function macCombo(combo) {
  const keys = (MAC_KEY[combo] || combo).split('+');
  const mods = ORDER.filter((m) => keys.slice(0, -1).includes(m));
  const rest = keys.filter((k, i) => i === keys.length - 1 || !ORDER.includes(k));
  return [...mods, ...rest];
}

export const keyLabel = (combo) =>
  (mac ? macCombo(combo) : combo.split('+')).map((k) => NAMES[k] || k).join(mac ? '' : '+');
export const renderKeys = (s) =>
  s.includes('{') ? s.replace(/\{((?:Mod|Ctrl|Shift|Alt)\+[^}]+)\}/g, (_, combo) => keyLabel(combo)) : s;

/** The shortcut key is down: Ctrl or ⌘ on Windows and Linux; ⌘ alone on the Mac. There Control is something
 *  else (⌃A to the line's start in a text field, Control-click is the right click), so it never counts as the shortcut
 *  key. With ⌘, Control makes the system's own keys (⌃⌘F full screen, ⌃⌘Q lock the screen, ⌃⌘Space the characters,
 *  ⌃⌘D look up), so ⌃⌘ is never a tool's shortcut either (⌃⌘F would open a tool's find as well as the system's full screen). */
export const modKey = (e) => (mac ? e.metaKey && !e.ctrlKey : e.ctrlKey || e.metaKey);

/** {Mod+Y} redoes on Windows and Linux. On the Mac redo is ⇧⌘Z alone: ⌘Y is not redo there, and is left to the system. */
export const yRedoes = !mac;

/** The Mac's ⌘↑ and ⌘↓, "to the start" and "to the end" of what is being read (a Mac laptop has no Home and End; fn ← →
 *  makes them, and those still work as on Windows): 'start', 'end', or '' for any other key and always on Windows. */
export const macEdge = (e) =>
  mac && e.metaKey && !e.ctrlKey && !e.altKey && !e.shiftKey
    ? e.key === 'ArrowUp'
      ? 'start'
      : e.key === 'ArrowDown'
        ? 'end'
        : ''
    : '';

/** The Mac: an ⎋ the page has used (it put away a panel, a menu, a card, a find) is used up there. The web view hands
 *  an ⎋ nobody marked as used on to the window, where the system's ⎋ (`cancelOperation:`) leaves full screen, as in
 *  QuickTime Player and Preview; without the mark both happen at once. Nothing on Windows. */
export const used = (e) => {
  if (mac) e.preventDefault();
};

/**
 * Words that are said differently on each system, because they name the system's own things:
 * T(os({ windows: '在资源管理器中显示', mac: '在访达中显示' })). Both are words in the tables like any other; Linux says
 * what Windows says unless given its own.
 */
export const os = (words) => words[platform] ?? words.windows;
