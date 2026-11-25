# Mandelbrot

An explorable Mandelbrot set for the web, with a deep zoom that stays sharp far past where
ordinary floating point gives up. Built with [Exact](https://github.com/expo/exact) and WebGPU.

> Work in progress. The page draws and zooms today; the explainer and the automatic zoom are next.

## What it does

- **Click to zoom.** A click zooms 4× around the point under the pointer. Option-click or
  right-click zooms out. Drag pans and the wheel zooms.
- **Deep zoom.** The set stays sharp past 10²⁸×. One point is computed at high precision on the
  CPU, and the GPU draws every other pixel as a small difference from it.
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
app.contract        the page: the canvas and its panel
app.test.contract   the page's tests
app.json            the app's name, id and host
gpu/                the renderer, a Rust crate on Exact's GPU surface
  src/              the view, the reference orbit, BLA and the GPU passes
  shaders/          WGSL for the set and the composite
```

## Licence

MIT. See [LICENSE](LICENSE).
