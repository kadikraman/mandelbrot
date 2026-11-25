<!-- exact:begin (exact new writes this block; bun exact.mjs update rewrites it) -->
# mandelbrot: an Exact app

Run `bun exact.mjs --help` first to create `.exact/checkout`; set `EXACT=<checkout>` if needed.

`app.contract` is the view, `app.ts` the data module, `app.test.contract` the tests, `app.json`
the manifest: set `app.name` and `app.id` before building. The app uses Exact at
`.exact/checkout` by path (`EXACT` overrides it).

Start from the nearest pattern: `bun exact.mjs
pattern` lists them and `pattern <name>` writes one, tested. .exact/checkout/docs/start-here-site.md (a site) or
.exact/checkout/docs/start-here.md is the only required reading. `bun exact.mjs contract vocab <name>` answers a
name (none: the card); `bun exact.mjs docs <heading words>` a guide's
section. Run `bun exact.mjs verify` when the features work, and `verify ios|android` at the end for a
phone app. When an iPhone app is ready to try, run `bun exact.mjs share` and send its link;
repeat for each change the person should see. `bun exact.mjs help` lists the verbs, including
app.json's `"commands"`.
`bun exact.mjs update` refreshes files. Native verbs (`ios`, `mac`, `linux`, `android`, `windows`)
name their platform in app.json on first use and build in a generated workspace;
`update --without <platform>` stops shipping it.
`bun .exact/checkout/scripts/exact.mjs setup --check` names what a native build lacks (`--platform ios --device` for a phone's: `share`, `ios --device`).

Generated, so don't edit: `exact.mjs` and this block.

## Feedback (required)

When a doc was wrong or missing, a workaround was needed, something took several tries or a
command confused you, record it: `bun exact.mjs feedback note "<what happened>"`. It attaches
exact2's commit and the last exit code, never output or code. `bun exact.mjs feedback status` says
whether notes are sent and whether to keep a diary; under `never`, skip all of this.
<!-- exact:end -->

## This app

A web-only Exact app: a full-screen Mandelbrot canvas with a deep zoom. The README says what it does
and how to run it; read it first.

- **The renderer is `gpu/`**, a Rust crate named `mandelbrot-gpu` (Exact expects `<app>-gpu`). It
  implements `exact_gpu::Surface`, and `app.contract` shows it with `exactSurface("mandel")`.
- **The explainer's words are in `content.ts`**, not in `app.contract`. Prose marks maths with
  `$…$`, bold with `**…**` and code with backticks; the page draws the runs. The look (colours,
  sizes, glass) is the `style` blocks at the top of `app.contract`; keep new parts on them.
- **Shaders are in `gpu/shaders/`.** `build.rs` generates their Rust bindings, so edit the WGSL and
  rebuild; never edit the generated code.
- **Web only.** Do not run the native verbs (`ios`, `mac`, `linux`, `android`, `windows`): each one
  adds a platform to `app.json`.
- **Commit on `main`.** There are no task branches or PRs in this repo.
- **Keep the README and this file current** when a command, a file or the layout changes. People
  read both.

### Check a change

```sh
bun exact.mjs test web     # Contract tests; also generates the Rust workspace cargo uses
cd gpu && cargo test       # the renderer's Rust tests
bun exact.mjs web          # the page, for a look in Chrome
```

A screenshot from Playwright's `page.screenshot` sends a pointer-leave to the canvas and hides the
zoom box. Use the DevTools protocol's `Page.captureScreenshot` instead.
