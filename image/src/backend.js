// Everything the page asks of the Rust side: the family's commands (shared/web/backend.js, from dotany-base), then Image's own.
import { invoke, fileSrc } from '@dotany/web/backend.js';

export * from '@dotany/web/backend.js';

export const openPath = (path) => invoke('open_path', { path });
export const openPasted = (name, bytes) => {
  const head = new TextEncoder().encode(name + '\n'),
    body = new Uint8Array(head.length + bytes.length);
  body.set(head);
  body.set(bytes, head.length);
  return invoke('open_pasted', body);
};
export const framesState = (slot) => invoke('frames_state', { slot });
export const reloadDocument = () => invoke('reload_document');
export const estimateSave = (seq, request) => invoke('estimate_save', { seq, request });
export const saveNotes = (request) => invoke('save_notes', { request });
export const clearTiles = () => invoke('clear_tiles');
export const saveAs = (path, overwrite, request) => invoke('save_as', { path, overwrite, request });
export const openInText = () => invoke('open_in_text');

// a piece of the drawing layer: three numbers, then a PNG. Large raw bodies crawl through WebView2 and hold up the whole
// window, and a drawing layer is mostly empty, so it packs to almost nothing
export async function putTile(slot, x, y, png) {
  const body = new Uint8Array(12 + png.length);
  new Uint32Array(body.buffer, 0, 3).set([slot, x, y]);
  body.set(png, 12);
  return invoke('put_tile', body);
}

// pixels come from their own address, off the main thread on both sides
const pix = (path) => fileSrc(path, 'dotpix');
async function fetchPixels(path) {
  const r = await fetch(pix(path));
  if (!r.ok) return null;
  return { w: +r.headers.get('X-Width'), h: +r.headers.get('X-Height'), data: new Uint8Array(await r.arrayBuffer()) };
}
export const viewPixels = (gen, slot, frame, max) => fetchPixels(`view/${gen}/${slot}/${frame}/${max}`);
export const lutTable = (gen) => fetchPixels(`lut/${gen}`);
export const regionPixels = (gen, slot, frame, r, ow, oh, geo, a) =>
  fetchPixels(
    `region/${gen}/${slot}/${frame}/${r.x}/${r.y}/${r.w}/${r.h}/${ow}/${oh}/${geo.join('/')}/${a.exposure}/${a.contrast}/${a.saturation}/${a.temperature}/${a.highlights}/${a.shadows}`
  );
