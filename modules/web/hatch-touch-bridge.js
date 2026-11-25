// The `touch-bridge` hatch: a hidden text field the picture hatch writes each
// touch into, as `touch <phase> <x> <y>`; its input action posts the text to
// the renderer. A hatch reaches Contract only by acting on an authored node.
let field = null;

export function send(text) {
  field?.input(text);
}

export function element(e) {
  if (e.isNew) field = e;
}

export function elementEnded(e) {
  if (field === e) field = null;
}
