// The `picture` hatch on the web: the canvas keeps every touch that starts on it.
// Safari on an iPhone takes a drag for itself at its first move and cancels the
// pointer, whatever the canvas's touch-action says; a touchmove that is
// cancelled keeps the drag with the page. Pointer events go on as before.
const stops = new WeakMap();

/** Built (`e.isNew`) and changed (a `data-*` word moved, in `e.data`). */
export function element(e) {
  if (!e.isNew) return;
  const stop = (event) => event.preventDefault();
  e.element.addEventListener('touchmove', stop, { passive: false });
  stops.set(e.element, stop);
  e.diagnostics.log(`touchmove kept on <${e.element.localName}>`);
}

/** The node is leaving: take back everything built added. */
export function elementEnded(e) {
  const stop = stops.get(e.element);
  if (stop) e.element.removeEventListener('touchmove', stop);
  stops.delete(e.element);
}
