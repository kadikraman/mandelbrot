# Mandelbrot

An explorable Mandelbrot set for the web, with a deep zoom that stays sharp far past where
ordinary floating point gives up. Built with [Exact](https://github.com/expo/exact) and WebGPU.

> Work in progress. The explainer has its first two chapters; chapters 3 to 7 and the automatic
> zoom's path to a tiny copy of the set are next.

## What it does

- **Click to zoom.** A click zooms 4× around the point under the pointer. Option-click or
  right-click zooms out. Drag pans and the wheel zooms.
- **Deep zoom.** The set stays sharp past 10²⁸×. One point is computed at high precision on the
  CPU, and the GPU draws every other pixel as a small difference from it.
- **An explainer beside the picture.** Seven short chapters, each with a plain text that is always
  shown, and a code tier and a maths tier that open on request. On a phone it is a sheet.
  The keyboard works too: E hides it, + and − zoom, the arrows pan.
- **Readable without WebGPU.** Where the browser cannot draw the picture, the page becomes a
  reading layout with a notice.
- **Smooth while moving.** The view draws at a lower resolution while it moves, then sharpens to
  16 samples a pixel when it rests.

## How it works

| Idea | In short | Where |
| --- | --- | --- |
| Perturbation | One reference orbit at high precision; each pixel iterates only its offset in `f32`. | `gpu/src/deep.rs`, `gpu/shaders/mandel.wgsl` |
| Bilinear approximation (BLA) | Skips many iterations at once where the offset stays small. | `gpu/src/bla.rs` |
| Distance estimation | Colours by distance to the set, so thin filaments stay visible. | `gpu/shaders/mandel.wgsl` |
| Progressive drawing | A low-resolution preview drawn in bands, then sharp samples at rest. | `gpu/src/lib.rs`, `gpu/src/draw.rs` |

## Run it

You need [Bun](https://bun.sh), [Rust](https://rustup.rs) and a browser with WebGPU, such as Chrome.

Clone Exact next to this repo, and fetch what it needs once:

```sh
git clone https://github.com/expo/exact.git
git clone https://github.com/kadikraman/mandelbrot.git
cd exact && bun scripts/exact.mjs setup && cd ..
```

Then start the page:

```sh
cd mandelbrot
bun exact.mjs web
```

Open the URL it prints. This README was last checked against Exact at `523457e1a`.

## Test it

```sh
bun exact.mjs test web     # the page's Contract tests, in a browser
cd gpu && cargo test       # the renderer's Rust tests
```

Run the Contract tests first: they generate the Rust workspace that `cargo test` uses.

## Layout

```
app.contract        the page: the canvas, the explore controls and the explainer
app.test.contract   the page's tests
content.ts          the chapters' text, code and maths
app.ts              serves the chapters to the page
app.json            the app's name, id and host
gpu/                the renderer, a Rust crate on Exact's GPU surface
  src/              the view, the reference orbit, BLA and the GPU passes
  shaders/          WGSL for the set and the composite
```

## Licence

MIT. See [LICENSE](LICENSE).
