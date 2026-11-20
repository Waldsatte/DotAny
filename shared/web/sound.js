// ---------- DotAny's one sound: a soft tik. No pitch, no melody, the same for everything ----------
// A few milliseconds of filtered noise, synthesized, so it adds no files.
const CENTER = 2600,
  Q = 0.8,
  ATTACK = 0.001,
  DECAY = 0.014,
  PEAK = 0.09;
let ctx = null,
  lastAt = -1;

function tik(c, out) {
  const t = c.currentTime,
    len = Math.ceil((ATTACK + DECAY + 0.01) * c.sampleRate);
  const buf = c.createBuffer(1, len, c.sampleRate),
    d = buf.getChannelData(0);
  for (let i = 0; i < len; i++) d[i] = Math.random() * 2 - 1;
  const s = c.createBufferSource(),
    f = c.createBiquadFilter(),
    g = c.createGain();
  s.buffer = buf;
  f.type = 'bandpass';
  f.frequency.value = CENTER;
  f.Q.value = Q;
  g.gain.setValueAtTime(0.0001, t);
  g.gain.exponentialRampToValueAtTime(PEAK, t + ATTACK);
  g.gain.exponentialRampToValueAtTime(0.0001, t + ATTACK + DECAY);
  s.connect(f).connect(g).connect(out);
  s.start(t);
}
export function playTik() {
  try {
    if (!ctx) ctx = new (window.AudioContext || window.webkitAudioContext)();
    if (ctx.state === 'suspended') ctx.resume();
    // one action can pass through two handlers; it still makes one sound
    if (ctx.currentTime - lastAt < 0.04) return;
    lastAt = ctx.currentTime;
    tik(ctx, ctx.destination);
  } catch {
    /* no sound is always acceptable */
  }
}
// for measuring: draws the sound offline and reports what it really does
export async function measureTik() {
  const off = new OfflineAudioContext(1, 11025, 44100);
  tik(off, off.destination);
  const d = (await off.startRendering()).getChannelData(0);
  let peak = 0,
    last = 0,
    first = -1;
  for (let i = 0; i < d.length; i++) {
    const a = Math.abs(d[i]);
    if (a > peak) peak = a;
    if (a > 0.001) {
      last = i;
      if (first < 0) first = i;
    }
  }
  return { peakDb: +(20 * Math.log10(peak)).toFixed(1), ms: +((last - first) / 44.1).toFixed(1) };
}
