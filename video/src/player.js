// The player, on its own: nothing here knows about Video's window, so another tool can use it.
// new RealFilm(url); await film.ready() rejects if the system can't draw this film's first picture.
// Then film.el goes on the page; play, pause, time, duration, width, height, playing, volume, rate, thumbs, frame, soundBytes, onended, dispose.
export class RealFilm {
  constructor(url) {
    this.el = document.createElement('video');
    // from the program's own address, so a frame can become a cover
    this.el.crossOrigin = 'anonymous';
    this.el.playsInline = true;
    this.el.preload = 'auto';
    this.el.src = url;
    this.onended = null;
    this.el.addEventListener('ended', () => {
      if (this.onended) this.onended();
    });
  }
  // a film counts as open once its first picture has been drawn; sound alone, or a stream the system can't read, doesn't
  async ready() {
    const wait = (ms, event) =>
      new Promise((ok, no) => {
        const t = setTimeout(() => no(new Error('slow')), ms);
        this.el.addEventListener(
          event,
          () => {
            clearTimeout(t);
            ok();
          },
          { once: true }
        );
        this.el.addEventListener(
          'error',
          () => {
            clearTimeout(t);
            no(new Error('decode'));
          },
          { once: true }
        );
      });
    await wait(30000, 'loadedmetadata');
    if (!this.el.videoWidth) throw new Error('no picture');
    // some files don't state their length until you ask for the end
    if (!Number.isFinite(this.el.duration)) {
      await new Promise((ok) => {
        this.el.addEventListener('durationchange', () => {
          if (Number.isFinite(this.el.duration)) ok();
        });
        this.el.currentTime = 1e7;
      });
      this.el.currentTime = 0;
    }
    if (this.el.readyState < 2) await wait(30000, 'loadeddata');
  }
  // a second, silent copy walks through the film so playing is never disturbed
  async thumbs(times, paint, alive) {
    const v = document.createElement('video');
    v.crossOrigin = 'anonymous';
    v.muted = true;
    v.preload = 'auto';
    v.src = this.el.src;
    await new Promise((ok, no) => {
      v.addEventListener('loadeddata', ok, { once: true });
      v.addEventListener('error', no, { once: true });
    });
    for (let i = 0; i < times.length && alive(); i++) {
      await new Promise((ok) => {
        const t = setTimeout(ok, 4000);
        v.addEventListener(
          'seeked',
          () => {
            clearTimeout(t);
            ok();
          },
          { once: true }
        );
        v.currentTime = Math.min(times[i], this.duration - 0.05);
      });
      if (alive()) paint(i, v);
    }
    v.removeAttribute('src');
    v.load();
  }
  get width() {
    return this.el.videoWidth;
  }
  get height() {
    return this.el.videoHeight;
  }
  get duration() {
    return this.el.duration;
  }
  get time() {
    return this.el.currentTime;
  }
  set time(t) {
    this.el.currentTime = t;
  }
  get playing() {
    return !this.el.paused;
  }
  // the Mac, after a seek while paused: WKWebView sometimes shows no picture for the new place and keeps the old one
  // (seen with MPEG-4 Part 2 pictures, now and then; not with H.264). It then presents no frame at all, and seeking to
  // the very same time does nothing; a second seek always brings the picture, so one that presents none is followed by
  // one a millisecond later (the same picture)
  settle(tries = 3) {
    const v = this.el;
    if (!v.paused || !v.requestVideoFrameCallback) return;
    const mine = (this.settling = (this.settling || 0) + 1),
      t = v.currentTime;
    let shown = false;
    v.requestVideoFrameCallback(() => (shown = true));
    const check = () => {
      if (mine !== this.settling || shown || !v.paused) return;
      if (v.seeking) return v.addEventListener('seeked', () => setTimeout(check, 80), { once: true });
      if (!tries) return;
      v.currentTime = t + 0.001 <= v.duration ? t + 0.001 : Math.max(0, t - 0.001);
      this.settle(tries - 1);
    };
    v.addEventListener('seeked', () => setTimeout(check, 80), { once: true });
  }
  // how much sound has really come out of the decoder. WKWebView doesn't count it: there, a sound track the system
  // can play is all it says (a sound it can't play leaves no track)
  get soundBytes() {
    return this.el.webkitAudioDecodedByteCount ?? (this.el.audioTracks?.length ? 1 : 0);
  }
  play() {
    return this.el.play();
  }
  pause() {
    this.el.pause();
  }
  set volume(v) {
    this.el.volume = v;
  }
  set rate(v) {
    this.el.playbackRate = v;
  }
  frame() {
    return this.el;
  }
  dispose() {
    this.el.pause();
    this.el.removeAttribute('src');
    this.el.load();
  }
}
