// The page's side of playing. The sound itself is in Rust; this keeps the line moving smoothly between what Rust reports.
import * as be from './backend.js';
import { app } from '@dotany/web/shared.js';

// a button's sound never lands on top of the music (the family's sound() asks app.quiet)
const quiet = { on: false };
app.quiet = () => quiet.on;

export const player = {
  playing: false,
  offset: 0,
  until: null,
  total: () => 1,
  onend: null,
  onnodevice: null,
  anchor: 0,
  anchorAt: 0,
  time() {
    if (!this.playing) return this.offset;
    return Math.min(this.anchor + (performance.now() - this.anchorAt) / 1000, this.until ?? this.total());
  },
  play(from = this.offset, until = null) {
    const total = this.total();
    const at = from >= total - 0.01 ? 0 : Math.max(0, from);
    this.until = until;
    this.playing = true;
    quiet.on = true;
    this.anchor = at;
    this.anchorAt = performance.now();
    this.offset = at;
    be.playerPlay(at, until);
  },
  pause() {
    this.offset = this.time();
    this.playing = false;
    quiet.on = false;
    be.playerPause();
  },
  seek(t) {
    if (this.playing) this.play(t, null);
    else {
      this.offset = t;
      be.playerSeek(t);
    }
  },
  // the song changed under it: stopped, and at the start
  reset() {
    this.playing = false;
    quiet.on = false;
    this.offset = 0;
    this.until = null;
  },
};

be.on('player', (p) => {
  if (p.noDevice) {
    player.playing = false;
    quiet.on = false;
    if (player.onnodevice) player.onnodevice();
    return;
  }
  if (p.ended) {
    if (!player.playing) return;
    player.playing = false;
    quiet.on = false;
    player.offset = p.at;
    player.until = null;
    if (player.onend) player.onend();
    return;
  }
  if (!player.playing) return;
  // follow Rust's clock when it has really moved apart; a little jitter isn't worth a jump
  const gap = p.at - player.time();
  if (Math.abs(gap) > 0.04 && Math.abs(gap) < 1.5) {
    player.anchor = p.at;
    player.anchorAt = performance.now();
  }
});
