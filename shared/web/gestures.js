// The trackpad, as the Mac's own programs read it. WKWebView gives a two-finger scroll as `wheel`
// events (the system's momentum included, in pixels), and a pinch or a rotation as WebKit's own `gesturestart`,
// `gesturechange`, `gestureend` (`scale` and `rotation` since the gesture began). The smart zoom (a double tap with two
// fingers) reaches no page at all, so the program sends it as `smart-zoom` (dotany-base's smart_zoom.rs). WheelEvent
// says nothing of phases (began, momentum, ended), so what needs them is read from the events' shape and timing, by the
// small pieces below that know no page: they are fed the events and answer, so a test can feed them recorded
// sequences and run them on any system.
//
// Everything here is for the Mac only: a tool calls these behind `platform === 'mac'`, and on Windows they are not
// used (a Windows precision touchpad's pinch is a `wheel` with ctrlKey, which the tools take as it is).
import { platform } from './keys.js';

export const mac = platform === 'mac';

/**
 * Whether a wheel event came from a trackpad (or a Magic Mouse's surface: both scroll in pixels, with momentum) or from
 * a mouse's wheel. On the Mac (macOS 27, WKWebView) WebKit gives a trackpad's pixels as they come, and a wheel's lines as 40 pixels a line,
 * the line counted as the system counts it, in 65536ths. The system's acceleration counts a wheel's turn in steps of
 * 6554 of those (a tenth of a line), so a notch is a whole number of 4.000244140625 pixels, never a whole number of
 * pixels; a wheel without acceleration gives whole lines, 40 pixels each. A trackpad's first event is a few whole
 * pixels, often with a sideways part. So: a mouse when the event is straight up or down and a whole number of those
 * steps or of lines; a trackpad otherwise. One stream keeps what its first event decided (a trackpad's gesture and its
 * momentum can pass through a value that looks like a notch); a pause of `gap` ms starts a new one.
 */
export const NOTCH = (6554 / 65536) * 40;
export const isNotch = (dy) => {
  const steps = (d, size) => Math.abs(d / size - Math.round(d / size)) * size < 1e-6;
  return dy !== 0 && (steps(dy, NOTCH) || steps(dy, 40));
};
export function wheelSource({ gap = 300 } = {}) {
  let last = -Infinity,
    kind = null;
  return (e, now = e.timeStamp) => {
    const fresh = now - last > gap;
    last = now;
    if (!fresh && kind) return kind;
    kind = e.deltaMode !== 0 || (e.deltaX === 0 && isNotch(e.deltaY)) ? 'mouse' : 'trackpad';
    return kind;
  };
}

/**
 * One page per swipe, as Books turns them: the swipe's deltas add up, and once past `distance` pixels it turns one
 * page (+1 forward: fingers moving left or up, the content following them). The rest of that swipe and all its
 * momentum turn nothing. The next swipe is told by a pause (no event for `quiet` ms) or, when it comes while the
 * momentum is still dying away, by deltas that grow again (momentum only ever slows down).
 */
export function swipeTurns({ distance = 60, quiet = 160, axis = 'x' } = {}) {
  let sum = 0,
    last = -Infinity,
    spent = false,
    low = Infinity;
  return (dx, dy, now) => {
    const d = axis === 'x' ? dx : axis === 'y' ? dy : Math.abs(dx) >= Math.abs(dy) ? dx : dy,
      size = Math.abs(d);
    if (now - last > quiet || (spent && size > 4 && size > low * 2.5)) {
      sum = 0;
      spent = false;
      low = Infinity;
    }
    last = now;
    if (spent) {
      low = Math.min(low, size);
      return 0;
    }
    sum += d;
    if (Math.abs(sum) < distance) return 0;
    spent = true;
    low = size;
    return Math.sign(sum);
  };
}

/**
 * A rotation turns the picture a quarter once per gesture, when the fingers have turned past `threshold` degrees
 * (Preview turns in quarters too). WebKit's `rotation` is in degrees since the gesture began, clockwise positive.
 */
export function quarterTurns({ threshold = 30 } = {}) {
  let done = false;
  return {
    begin() {
      done = false;
    },
    /** +1 a quarter clockwise, -1 counter-clockwise, 0 nothing (yet, or any more this gesture) */
    change(rotation) {
      if (done || Math.abs(rotation) < threshold) return 0;
      done = true;
      return Math.sign(rotation);
    },
  };
}

/**
 * The pinch, from WebKit's gesture events: `begin(x, y)` as the fingers land, `zoom(factor, x, y)` with the factor since
 * the last change (so a tool multiplies its zoom by it, about the fingers' middle), `rotate(degrees)` with the rotation
 * since the gesture began, `end()`. Nothing of it on Windows (no such events there). Returns a function that stops it.
 */
export function pinch(target, { begin, zoom, rotate, end } = {}) {
  if (!mac) return () => {};
  let scale = 1;
  const start = (e) => {
    e.preventDefault();
    scale = 1;
    begin?.(e.clientX, e.clientY);
  };
  const change = (e) => {
    e.preventDefault();
    const s = e.scale || 1;
    if (s !== scale) zoom?.(s / scale, e.clientX, e.clientY);
    scale = s;
    if (e.rotation) rotate?.(e.rotation);
  };
  const stop = (e) => {
    e.preventDefault();
    end?.();
  };
  target.addEventListener('gesturestart', start);
  target.addEventListener('gesturechange', change);
  target.addEventListener('gestureend', stop);
  return () => {
    target.removeEventListener('gesturestart', start);
    target.removeEventListener('gesturechange', change);
    target.removeEventListener('gestureend', stop);
  };
}
