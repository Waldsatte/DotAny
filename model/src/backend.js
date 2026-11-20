// This tool's own commands. The family's are in @dotany/web/backend.js.
import { invoke } from '@dotany/web/backend.js';

/** The model's bytes, whole, as an ArrayBuffer. Rejects with 'missing', 'refused' or 'damaged'. */
export const readModel = (path) => invoke('read_model', { path });
/** The page has read it: now it's the open file, watched and on the recent list. */
export const adoptModel = (path) => invoke('adopt_model', { path });
/** Which of these names are there beside the model. */
export const haveBeside = (names) => invoke('have_beside', { names });
/** Bytes the page made, written whole. A picture doesn't go on the recent list. */
export const saveModel = (path, bytes, picture) =>
  invoke('save_model', bytes, { headers: { path: encodeURIComponent(path), picture: picture ? '1' : '0' } });
