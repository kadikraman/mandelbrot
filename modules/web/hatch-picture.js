// The `picture` hatch: touches on the canvas reach the renderer from touch
// events, not pointer events. Safari on an iPhone ends a drag's pointer events
// at its first move, but its touch events run until the finger lifts. A
// cancelled touchmove keeps the page from scrolling or zooming meanwhile.
import { send } from './hatch-touch-bridge.js';

const taken = new WeakMap();

export function element(e) {
  if (!e.isNew) return;
  const el = e.element;
  const debug = new URLSearchParams(location.search).get('debug') === '1';
  let finger = null;
  let move = null;
  let frame = 0;
  const at = (t) => {
    const box = el.getBoundingClientRect();
    return `${(t.clientX - box.left).toFixed(1)} ${(t.clientY - box.top).toFixed(1)}`;
  };
  const own = (list) => [...list].find((t) => t.identifier === finger);
  // Moves go out at most once a frame, the latest; a down, up or cancel at once.
  const flush = () => {
    cancelAnimationFrame(frame);
    frame = 0;
    if (move) send(`touch move ${move}`);
    move = null;
  };
  const end = (phase, t) => {
    flush();
    send(`touch ${phase} ${at(t)}`);
    finger = null;
  };
  const listeners = {
    touchstart: (event) => {
      if (finger !== null) {
        // A second finger: no pinch here, so the press ends.
        const t = own(event.touches);
        if (t) end('cancel', t);
        return;
      }
      const t = event.changedTouches[0];
      finger = t.identifier;
      send(`touch down ${at(t)}`);
    },
    touchmove: (event) => {
      event.preventDefault();
      const t = own(event.changedTouches);
      if (!t) return;
      move = at(t);
      if (!frame) frame = requestAnimationFrame(flush);
    },
    touchend: (event) => {
      const t = own(event.changedTouches);
      if (t) end('up', t);
    },
    touchcancel: (event) => {
      const t = own(event.changedTouches);
      if (t) end('cancel', t);
    },
  };
  for (const [type, listener] of Object.entries(listeners)) {
    el.addEventListener(type, listener, { passive: type !== 'touchmove' });
  }
  const log = debug ? rawLog(e) : null;
  taken.set(el, { listeners, log });
}

export function elementEnded(e) {
  const kept = taken.get(e.element);
  if (!kept) return;
  for (const [type, listener] of Object.entries(kept.listeners)) e.element.removeEventListener(type, listener);
  kept.log?.();
  taken.delete(e.element);
}

// With ?debug=1: the browser's own pointer and touch events on the canvas, in order.
function rawLog(e) {
  const el = e.element;
  const box = document.createElement('pre');
  box.style.cssText = 'position:fixed;left:12px;right:12px;top:330px;margin:0;padding:8px;border-radius:8px;font:11px/1.5 ui-monospace,monospace;background:rgba(0,0,0,.75);color:#ffd27c;pointer-events:none;z-index:10;white-space:pre-wrap';
  document.body.append(box);
  e.owns(box, 'the ?debug=1 log of the browser events on the canvas');
  const lines = [];
  let last = '';
  const note = (event) => {
    const word = event.type.replace('pointer', 'p:').replace('touch', 't:');
    if (word === last && lines.length) {
      lines[lines.length - 1] = lines[lines.length - 1].replace(/( ×(\d+))?$/, (_, __, n) => ` ×${(+n || 1) + 1}`);
    } else {
      lines.push(word + (event.type === 'touchmove' ? ` cancelable=${event.cancelable}` : ''));
      if (lines.length > 10) lines.shift();
    }
    last = word;
    box.textContent = 'Browser events:\n' + lines.join('\n');
  };
  const types = ['pointerdown', 'pointermove', 'pointerup', 'pointercancel', 'pointerout', 'lostpointercapture', 'touchstart', 'touchmove', 'touchend', 'touchcancel'];
  for (const type of types) el.addEventListener(type, note, true);
  return () => {
    for (const type of types) el.removeEventListener(type, note, true);
    box.remove();
  };
}
