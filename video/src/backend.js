// Everything the page asks of the Rust side: the family's commands (shared/web/backend.js, from dotany-base), then Video's own.
import { invoke, fileSrc } from '@dotany/web/backend.js';

export * from '@dotany/web/backend.js';

// a film is read first and takes over the window only once it has really played
export const openPath = (path) => invoke('open_path', { path });
export const commitOpen = (gen) => invoke('commit_open', { gen });
export const filmUrl = (gen) => fileSrc(String(gen), 'dotfilm');
// a film Windows plays (AVI, WMV): see system-film.js
export const systemFilm = {
  open: (gen) => invoke('system_open', { gen }),
  control: (gen, action, value = null, id = null) => invoke('system_control', { gen, action, value, id }),
  close: (gen) => invoke('system_close', { gen }),
};
export const pickSubtitle = (label) => invoke('pick_subtitle', { label });
export const readSubtitle = (path) => invoke('read_subtitle', { path }).then((b) => new Uint8Array(b));
export const pickCover = (label) => invoke('pick_cover', { label });
export const loadCover = (path) => invoke('load_cover', { path }).then((b) => new Uint8Array(b));
export const setCover = (bytes) => invoke('set_cover', bytes);
export const readCover = () => invoke('read_cover').then((b) => (b.byteLength ? new Uint8Array(b) : null));
export const estimateSave = (request) => invoke('estimate_save', { request });
export const saveAs = (path, request) => invoke('save_as', { path, request });
