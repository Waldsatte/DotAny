// Everything the page asks of the Rust side: the family's commands (shared/web/backend.js, from dotany-base), then Audio's own.
import { invoke } from '@dotany/web/backend.js';

export * from '@dotany/web/backend.js';

export const openPath = (path) => invoke('open_path', { path });
export const pickCover = (label) => invoke('pick_cover', { label });
export const setCoverPath = (path) => invoke('set_cover_path', { path });
export const estimateSave = (seq, request) => invoke('estimate_save', { seq, request });
export const saveNotes = (request) => invoke('save_notes', { request });
export const saveAs = (path, request) => invoke('save_as', { path, request });

export const waveBars = (spans, from, span, bars) => invoke('wave_bars', { spans, from, span, bars });
export const levelOf = (spans) => invoke('level_of', { spans });
export const playerSet = (recipe, k) => invoke('player_set', { recipe, k });
export const playerPlay = (from, until) => invoke('player_play', { from, until });
export const playerPause = () => invoke('player_pause');
export const playerSeek = (at) => invoke('player_seek', { at });
export const playerStats = () => invoke('player_stats');
