// This tool's own commands. The family's are in @dotany/web/backend.js.
import { invoke } from '@dotany/web/backend.js';

/** The presentation's bytes, whole, as an ArrayBuffer. Rejects with 'missing', 'refused' or 'damaged'. */
export const readDeck = (path) => invoke('read_deck', { path });
/** The page has read it: now it's the open file, watched and on the recent list. */
export const adoptDeck = (path) => invoke('adopt_deck', { path });
/** Bytes the page made, written whole. A PDF or a picture (`exported`) doesn't go on the recent list. */
export const saveBytes = (path, bytes, exported) =>
  invoke('save_bytes', bytes, { headers: { path: encodeURIComponent(path), export: exported ? '1' : '0' } });
/** The page's print layout, printed by WebView2 to a PDF of sheets `width` × `height` inches, written to `path`. */
export const printPdf = (path, width, height) => invoke('print_pdf', { path, width, height });
/** The Mac: the print layout's slides (`width` × `height` inches) as PNGs `pixels` wide, one to each of `paths`. Answers
 *  the first one's save, or the first failure; rejects when the pictures couldn't be made. */
export const printPng = (paths, width, height, pixels) => invoke('print_png', { paths, width, height, pixels });
/** The window full screen for the show, and back. */
export const fullScreen = (on) => invoke('set_full', { on });
/** A web address a slide links to, in the system's browser. */
export const openLink = (url) => invoke('open_link', { url });
/** The system's open dialog for a picture or a film or sound to put on a slide; then that file's bytes. */
export const pickInsert = (kind, label) => invoke('pick_insert', { kind, label });
export const readInsert = (path) => invoke('read_insert', { path });
/** Windows plays a film or sound (WMV, AVI, WMA). */
export const mediaOpen = (gen, ext, bytes) => invoke('media_open', bytes, { headers: { gen: String(gen), ext } });
export const mediaControl = (gen, action, value, id) =>
  invoke('media_control', { gen, action, value: value == null ? null : value, id: id == null ? null : id });
export const mediaClose = (gen) => invoke('media_close', { gen });
export const mediaCount = () => invoke('media_count');
/** The Mac: a film WKWebView won't play (AVI) repackaged for it, as bytes. Rejects 'windows' (WMV, WMA) or 'cannot'. */
export const mediaRepack = (ext, bytes) =>
  invoke('media_repack', bytes, { headers: { ext } }).then((b) => new Uint8Array(b));
/** The Mac with more than one screen: the presenter's view in a window of its own on the presenter's screen; false with
 *  one screen. Then what it shows (`presenterSend`) and its going (`presenterClose`); presenter.js is that window's
 *  page, which asks the show's page through `presenterDo`. */
export const presenterOpen = () => invoke('presenter_open');
export const presenterSend = (state) => invoke('presenter_send', { state });
export const presenterDo = (action) => invoke('presenter_do', { action });
export const presenterClose = () => invoke('presenter_close');
