// The family's motion for a box whose contents change (MOVE, --t-2): the save card's rows and quiet line, the side
// panels' glass (side.js, sideMorph), and the tools, which import morph from shell.js.
import { reduce, EASE } from '../shared.js';

export const MOVE = { duration: 240, easing: EASE };
// whatever changes size eases to it, and what's new fades in; nothing jumps. Boxes that change together (the save
// card's quiet line and its rows) are given as a list and move as one: the same MOVE, from the same frame
export function morph(box, change, fresh) {
  const boxes = [].concat(box);
  if (!fresh) fresh = () => boxes.flatMap((b) => [...b.children]);
  if (reduce) {
    change();
    return;
  }
  boxes.forEach((b) => b.getAnimations().forEach((a) => a.cancel()));
  // a box's whole height, its padding included, and the padding itself (a line that appears gains its padding too):
  // eased as one, so the box is its real height in every frame and doesn't snap when the motion is over
  const shape = (b) => {
    const c = getComputedStyle(b);
    return { height: b.offsetHeight + 'px', paddingTop: c.paddingTop, paddingBottom: c.paddingBottom };
  };
  const from = boxes.map(shape);
  change();
  const to = boxes.map(shape);
  fresh().forEach((c) => c && c.animate([{ opacity: 0 }, { opacity: 1 }], MOVE));
  boxes.forEach((b, n) => {
    if (from[n].height === to[n].height) return;
    b.style.overflow = 'hidden';
    b.style.boxSizing = 'border-box';
    const a = b.animate([from[n], to[n]], MOVE);
    a.onfinish = a.oncancel = () => {
      b.style.overflow = '';
      b.style.boxSizing = '';
    };
  });
}
