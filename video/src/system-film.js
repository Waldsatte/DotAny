// A film WebView2 won't play (AVI, WMV), played by Windows: its media engine decodes and keeps the sound in step,
// and each finished picture arrives in memory shared with this page, drawn here on a canvas. The same face as RealFilm,
// so the window can't tell them apart. Nothing here knows about Video's window: `port` is open, control and close.
// The layout of the shared memory is in src-tauri/src/platform/media_foundation.rs.
const HEADER = 64;

export class SystemFilm {
  constructor(gen, port) {
    this.gen = gen;
    this.port = port;
    this.el = document.createElement('canvas');
    this.g = this.el.getContext('2d', { alpha: false });
    this.onended = null;
    this.paused = true;
    this.seekTo = null;
    this.seekId = 0;
    this.seen = 0;
    this.wasEnded = false;
    this.gone = false;
  }
  // as RealFilm: it counts as open once its first picture has been drawn
  async ready() {
    // only WebView2 shares memory with a page; the Mac never makes one of these (it plays every film in the page)
    if (!window.chrome?.webview) throw new Error('nodecoder');
    const view = chrome.webview;
    const shared = new Promise((ok, no) => {
      const t = setTimeout(() => {
        view.removeEventListener('sharedbufferreceived', take);
        no(new Error('slow'));
      }, 30000);
      const take = (e) => {
        if (!e.additionalData || e.additionalData.dotfilm !== this.gen) return;
        clearTimeout(t);
        view.removeEventListener('sharedbufferreceived', take);
        if (this.gone) {
          view.releaseBuffer(e.getBuffer());
          no(new Error('gone'));
          return;
        }
        ok(e.getBuffer());
      };
      view.addEventListener('sharedbufferreceived', take);
    });
    const r = await this.port.open(this.gen);
    if (r.status !== 'ok') {
      shared.catch(() => {});
      throw new Error(r.status);
    }
    // let go of while it was starting: it started anyway, so stop it now
    if (this.gone) {
      shared.catch(() => {});
      this.port.close(this.gen);
      throw new Error('gone');
    }
    this.facts = r.facts;
    this.buf = await shared;
    this.u32 = new Uint32Array(this.buf, 0, 16);
    this.f64 = new Float64Array(this.buf, 16, 2);
    this.el.width = this.facts.width;
    this.el.height = this.facts.height;
    await new Promise((ok, no) => {
      const t0 = performance.now();
      const look = () => {
        if (this.gone) return no(new Error('gone'));
        if (this.draw()) return ok();
        if (performance.now() - t0 > 15000) return no(new Error('no picture'));
        setTimeout(look, 10);
      };
      look();
    });
    this.tick();
  }
  // the newest picture, if there's one not yet drawn
  draw() {
    const seq = this.u32[0];
    if (!seq || seq === this.seen) return false;
    this.seen = seq;
    const w = this.u32[8],
      h = this.u32[9],
      size = w * h * 4,
      at = HEADER + this.u32[1] * size;
    const frame = new VideoFrame(new Uint8Array(this.buf, at, size), {
      format: 'BGRA',
      codedWidth: w,
      codedHeight: h,
      timestamp: 0,
    });
    this.g.drawImage(frame, 0, 0, this.el.width, this.el.height);
    frame.close();
    return true;
  }
  tick() {
    if (this.gone) return;
    this.draw();
    const ended = (this.u32[2] & 2) !== 0;
    if (ended && !this.wasEnded) {
      this.paused = true;
      if (this.onended) this.onended();
    }
    this.wasEnded = ended;
    // a timer rather than the display's beat: a window that isn't on screen keeps its film moving
    this.timer = setTimeout(() => this.tick(), 8);
  }
  async thumbs() {
    /* nothing to cut, so no strip: these films are only played */
  }
  get width() {
    return this.facts.width;
  }
  get height() {
    return this.facts.height;
  }
  get duration() {
    return this.facts.duration;
  }
  get time() {
    if (this.seekTo != null && this.u32[3] === this.seekId) this.seekTo = null;
    return this.seekTo != null ? this.seekTo : this.f64[0];
  }
  set time(t) {
    this.seekTo = t;
    this.seekId++;
    this.port.control(this.gen, 'seek', t, this.seekId);
  }
  get playing() {
    return !this.paused;
  }
  // the engine reports no byte count; it has sound if it found a track it can play
  get soundBytes() {
    return this.facts.audio ? 1 : 0;
  }
  play() {
    this.paused = false;
    this.wasEnded = false;
    this.port.control(this.gen, 'play');
    return Promise.resolve();
  }
  pause() {
    this.paused = true;
    this.port.control(this.gen, 'pause');
  }
  set volume(v) {
    this.port.control(this.gen, 'volume', v);
  }
  set rate(v) {
    this.port.control(this.gen, 'rate', v);
  }
  frame() {
    return this.el;
  }
  dispose() {
    if (this.gone) return;
    this.gone = true;
    clearTimeout(this.timer);
    if (this.buf) {
      chrome.webview.releaseBuffer(this.buf);
      this.buf = null;
    }
    this.port.close(this.gen);
  }
}
