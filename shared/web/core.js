// The typing feel (a drawn caret that glides, new letters that fade in): the numbers and the pure functions every
// surface shares. No DOM.

export const TAU = 38; // ms: caret ease-out time constant (~30ms halfway, ~110ms to land)
export const FOLLOW = 320; // ms: keep re-measuring the caret target this long after a change (marks widen in 140ms)
export const REST = 520; // ms: still this long before the caret starts to blink
export const FRESH = 240; // ms: how long a typed letter takes to fade in
export const EASE = 'cubic-bezier(.2, .7, .2, 1)';

// one frame of the glide: cover 1 - e^(-dt/τ) of what is left; land exactly once within 0.3px
export function glideStep(cur, target, dt) {
  const k = 1 - Math.exp(-Math.max(0, dt) / TAU),
    next = {};
  let moving = false;
  for (const key of Object.keys(target)) {
    const v = cur[key] + (target[key] - cur[key]) * k;
    if (Math.abs(target[key] - v) > 0.3) moving = true;
    next[key] = v;
  }
  return moving ? { pos: next, moving } : { pos: { ...target }, moving };
}

// a jump to another line or far along one lands at once: the glide is for writing and stepping
export const farJump = (cur, target, lineH) =>
  !cur || Math.abs(target.y - cur.y) > lineH * 2.2 || Math.abs(target.x - cur.x) > 360;

// the one stretch that changed between two values: before[from, oldTo) became after[from, newTo)
export function changeOf(before, after) {
  let s = 0;
  const n = Math.min(before.length, after.length);
  while (s < n && before.charCodeAt(s) === after.charCodeAt(s)) s++;
  let e = 0;
  while (e < n - s && before.charCodeAt(before.length - 1 - e) === after.charCodeAt(after.length - 1 - e)) e++;
  return { from: s, oldTo: before.length - e, newTo: after.length - e };
}

// what typing added, or null
export function insertedRange(before, after) {
  if (after.length <= before.length) return null;
  const c = changeOf(before, after);
  return c.newTo > c.from ? { from: c.from, to: c.newTo } : null;
}

// a settling range carried across a later edit; null once the edit eats into it
export function mapRange(r, c) {
  const d = c.newTo - c.from - (c.oldTo - c.from);
  if (r.to <= c.from) return r;
  if (r.from >= c.oldTo) return { ...r, from: r.from + d, to: r.to + d };
  return null;
}

/* Shared CSS (adapt the variable names to the project):

:root { --inkfall-caret: #FF6A00; --inkfall-ease: cubic-bezier(.2, .7, .2, 1); }
@keyframes inkfall-blink { 0%, 44% { opacity: 1; } 54%, 92% { opacity: 0; } 100% { opacity: 1; } }
@keyframes inkfall-fresh { from { opacity: .06; } }
.inkfall-caret { position: absolute; left: 0; top: 0; width: 2px; margin-left: -1px; border-radius: 1px;
  background: var(--inkfall-caret); pointer-events: none; will-change: transform; }
.inkfall-caret.is-blink { animation: inkfall-blink 1.06s linear infinite; }
.inkfall-fresh { position: relative; animation: inkfall-fresh 240ms var(--inkfall-ease) both; }
@media (prefers-reduced-motion: reduce) { .inkfall-caret.is-blink, .inkfall-fresh { animation: none; } }
*/
