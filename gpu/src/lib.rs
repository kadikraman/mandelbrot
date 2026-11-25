//! The Mandelbrot set on a GPU surface, by perturbation from a
//! high-precision reference orbit, so it stays sharp far past f32's 10⁴×.
//!
//! A click zooms 4× in or out around the point under the pointer, which stays
//! put; drag pans; the wheel zooms. The set is drawn into its own texture only
//! when the view changes, at a lower resolution while the view moves, and a
//! cheap pass puts that texture on screen with the hover box over it.
//!
//! Views are f64 offsets from a high-precision anchor (`deep::Point`), which
//! moves to the view when the offset grows, so f64 never runs out.

mod bla;
mod deep;
mod draw;

use exact_gpu::json::number;
use exact_gpu::wgpu;
use exact_gpu::{Frame, InputEvent, PointerKind, PointerPhase, Registry, Surface, SurfaceError, Value};

/// Shader interfaces reflected from WGSL during the build.
pub mod shaders {
    include!(concat!(env!("OUT_DIR"), "/shaders.rs"));
}

use deep::{Orbit, Point};
use draw::{Gpu, Picture, SetPass, Show};

const CLICK_ZOOM: f64 = 4.0;
const EASE_MS: f64 = 400.0;
/// How far a press may move and still be a tap, in points. A finger drifts
/// further than a mouse (Android's touch slop is 8 dp).
const TAP_SLOP: f32 = 6.0;
const TOUCH_TAP_SLOP: f32 = 12.0;
const TAP_MS: f64 = 500.0;
/// A wheel or drag counts as motion this long after its last event.
const SETTLE_MS: f64 = 150.0;
const AUTO_TARGET: (f64, f64) = (-0.743_643_887_037_151, 0.131_825_904_205_330);
const HOME: (f64, f64) = (-0.6, 0.0);
/// A reference this many pixels from the view is replaced: dc loses f32 bits.
const REFERENCE_REACH_PX: f64 = 1e5;
/// An offset from the anchor this many steps long moves the anchor.
const ANCHOR_REACH_STEPS: f64 = 1e6;
/// The smallest step, in plane units a canvas point, the zoom goes to. The GPU
/// works in f32, whose smallest normal number is about 1.2e-38; at 3 device
/// pixels a point, a sample's offset within a pixel still stays above it.
const MIN_STEP: f64 = 2e-37;
/// The palettes in `mandel.wgsl`.
const THEMES: u32 = 6;
/// Samples summed per pixel once the view is still.
const SAMPLES: u32 = 16;
/// The share of escapes in the limit's top quarter that raises the limit.
const LATE_SHARE: f64 = 0.02;

/// A view of the plane: its centre and the complex width of one point.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct View {
    pub cx: f64,
    pub cy: f64,
    pub step: f64,
}

impl View {
    /// The view `t` of the way from `a` to `b`. A zoom scales about the one
    /// plane point both views show at the same place, so that point stays
    /// still and everything else moves straight to or from it; a pure pan is
    /// a straight line.
    fn lerp(a: View, b: View, t: f64) -> View {
        let f = b.step / a.step;
        if (f - 1.0).abs() < 1e-9 {
            return View { cx: a.cx + (b.cx - a.cx) * t, cy: a.cy + (b.cy - a.cy) * t, step: a.step };
        }
        let px = (b.cx - f * a.cx) / (1.0 - f);
        let py = (b.cy - f * a.cy) / (1.0 - f);
        a.scaled_about(px, py, f.powf(t))
    }

    fn shifted(self, dx: f64, dy: f64) -> View {
        View { cx: self.cx - dx, cy: self.cy - dy, ..self }
    }

    /// This view scaled by `factor` around plane point (px, py), which stays put.
    /// The step stops at `MIN_STEP`.
    fn scaled_about(self, px: f64, py: f64, factor: f64) -> View {
        let factor = factor.max(MIN_STEP / self.step).min(factor.max(1.0));
        View { cx: px + (self.cx - px) * factor, cy: py + (self.cy - py) * factor, step: self.step * factor }
    }

    fn deepest(self) -> bool {
        self.step <= MIN_STEP * (1.0 + 1e-9)
    }
}

struct Ease {
    from: View,
    start_ms: Option<f64>,
}

struct Press {
    id: u32,
    x: f32,
    y: f32,
    at_ms: f64,
    dragged: bool,
    zoom_out: bool,
}

/// What a picture shows: a view at an iteration count, from a reference.
#[derive(Clone, Copy, PartialEq)]
struct Key {
    view: View,
    max_iter: u32,
    reference: u64,
    bla: bool,
    theme: u32,
}

/// The preview shown: drawn at a low resolution, moved to the current view.
#[derive(Clone, Copy)]
struct Preview {
    key: Key,
    scale: f32,
}

/// The preview being drawn, a band of rows a frame, down to `row`; shown once
/// done. `frames` counts the frames it has taken.
#[derive(Clone, Copy)]
struct Drawing {
    key: Key,
    size: (u32, u32),
    row: u32,
    frames: u32,
    count: bool,
}

/// The sharp picture of the current view: `samples` complete passes, and the
/// next pass done down to `row`.
#[derive(Clone, Copy)]
struct Sharp {
    key: Key,
    samples: u32,
    row: u32,
}

/// A reference orbit and its point as an offset from the anchor.
struct Reference {
    orbit: Orbit,
    offset: (f64, f64),
}

/// The surface: the shown view, the target it eases to, and gesture state.
pub struct MandelSurface {
    view: View,
    target: View,
    ease: Option<Ease>,
    press: Option<Press>,
    hover: Option<(f32, f32)>,
    alt_held: bool,
    size: (f32, f32),
    /// 0 picks the iteration count from the zoom.
    max_iter_cap: u32,
    /// Multiplies the iterations the zoom picks.
    iter_scale: f64,
    /// Iterations the picture asked for: twice its highest escape count.
    picture_iter: u32,
    zoom_out_mode: bool,
    /// Whether the hover box is shown, as the panel's switch says.
    show_box: bool,
    auto_rate: f64,
    /// The page's count of automatic zoom starts; a new one restarts a stopped zoom.
    auto_run: f64,
    /// A tap, drag or scroll stopped the automatic zoom.
    auto_stopped: bool,
    /// Canvas points on the right that the explainer covers.
    free_right: f32,
    last_ms: Option<f64>,
    last_motion_ms: f64,
    /// Resolution of the set while the view moves, as a fraction of device pixels.
    moving_scale: f32,
    intervals: Vec<f64>,
    stats_ms: f64,
    clicks: u32,
    /// The step that fits the whole set across the canvas: zoom 1.
    home_step: f64,
    touched: bool,
    dirty: bool,
    preview: Option<Preview>,
    drawing: Option<Drawing>,
    sharp: Option<Sharp>,
    /// Pixel-iterations a frame may spend on a band, steered by whether the GPU
    /// finished the last band within a frame.
    band_budget: f64,
    banded: bool,
    /// The plane point views are offsets from, in `bits` bits, and its f64 value.
    anchor: Point,
    anchor_f64: (f64, f64),
    bits: usize,
    auto_target: (f64, f64),
    reference: Option<Reference>,
    /// The next reference, built a budget at a time while the current one draws.
    next: Option<Reference>,
    reference_generation: u64,
    /// Points of the current reference already on the GPU.
    uploaded: usize,
    /// The BLA table on the GPU: its reference, levels, orbit points covered and max|dc|.
    bla: Option<(u64, u32, usize, f64)>,
    /// Draw without BLA, to compare.
    bla_off: bool,
    /// The colour theme, an index into the shader's palettes.
    theme: u32,
    gpu: Option<Gpu>,
}

impl Default for MandelSurface {
    fn default() -> Self {
        let view = View { cx: 0.0, cy: 0.0, step: 3.2 / 800.0 };
        let bits = deep::bits_for(view.step);
        Self {
            view,
            target: view,
            ease: None,
            press: None,
            hover: None,
            alt_held: false,
            size: (800.0, 600.0),
            max_iter_cap: 0,
            iter_scale: 1.0,
            picture_iter: 0,
            zoom_out_mode: false,
            show_box: false,
            auto_rate: 0.0,
            auto_run: 0.0,
            auto_stopped: false,
            free_right: 0.0,
            last_ms: None,
            last_motion_ms: f64::NEG_INFINITY,
            moving_scale: 0.5,
            intervals: Vec::new(),
            stats_ms: 0.0,
            clicks: 0,
            home_step: view.step,
            touched: false,
            dirty: true,
            preview: None,
            drawing: None,
            sharp: None,
            band_budget: 5e7,
            banded: false,
            anchor: Point::from_f64(HOME.0, HOME.1, bits),
            anchor_f64: HOME,
            bits,
            auto_target: (AUTO_TARGET.0 - HOME.0, AUTO_TARGET.1 - HOME.1),
            reference: None,
            next: None,
            reference_generation: 0,
            uploaded: 0,
            bla: None,
            bla_off: false,
            theme: 0,
            gpu: None,
        }
    }
}

impl MandelSurface {
    /// The plane point under canvas point (x, y) in `view`.
    fn plane_at(&self, view: View, x: f32, y: f32) -> (f64, f64) {
        let dx = x as f64 - self.size.0 as f64 * 0.5;
        let dy = self.size.1 as f64 * 0.5 - y as f64;
        (view.cx + dx * view.step, view.cy + dy * view.step)
    }

    fn ease_to(&mut self, target: View) {
        self.touched = true;
        self.ease = Some(Ease { from: self.view, start_ms: None });
        self.target = target;
        self.dirty = true;
    }

    /// A click zooms 4× in, or out, keeping the point under it where it is.
    /// Chained clicks add to an ease in flight.
    pub fn click(&mut self, x: f32, y: f32, zoom_out: bool) {
        self.stop_auto();
        let (px, py) = self.plane_at(self.target, x, y);
        let factor = if zoom_out { CLICK_ZOOM } else { 1.0 / CLICK_ZOOM };
        self.clicks += 1;
        self.ease_to(self.target.scaled_about(px, py, factor));
    }

    /// The whole set, centred in the canvas. It ignores the explainer, so opening
    /// or closing it never moves or rescales the picture.
    fn home_view(&self) -> View {
        let step = 3.2 / self.size.0.max(1.0) as f64;
        let (ax, ay) = self.anchor_f64;
        View { cx: HOME.0 - ax, cy: HOME.1 - ay, step }
    }

    /// Back to the whole set.
    pub fn reset(&mut self) {
        self.ease_to(self.home_view());
    }

    fn auto_running(&self) -> bool {
        self.auto_rate > 0.0 && !self.auto_stopped
    }

    fn set_auto_run(&mut self, run: f64) {
        if run != self.auto_run {
            self.auto_run = run;
            self.auto_stopped = false;
        }
    }

    /// The reader takes over: the automatic zoom stops until the page starts it again.
    fn stop_auto(&mut self) {
        if self.auto_running() {
            self.auto_stopped = true;
            self.dirty = true;
        }
    }

    /// Wheel zoom keeps the point under the pointer fixed.
    pub fn wheel(&mut self, dy: f32, x: f32, y: f32) {
        self.stop_auto();
        let (px, py) = self.plane_at(self.view, x, y);
        self.jump(self.view.scaled_about(px, py, (dy as f64 * 0.002).exp()));
    }

    fn pan(&mut self, dx: f32, dy: f32) {
        self.stop_auto();
        let v = self.view;
        self.jump(View { cx: v.cx - dx as f64 * v.step, cy: v.cy + dy as f64 * v.step, ..v });
    }

    fn jump(&mut self, view: View) {
        self.touched = true;
        self.view = view;
        self.target = view;
        self.ease = None;
        self.dirty = true;
        self.last_motion_ms = f64::INFINITY;
    }

    /// Whether a click now zooms out: the chosen mode, flipped while Option is held.
    fn zooms_out(&self) -> bool {
        self.zoom_out_mode != self.alt_held
    }

    /// The hover box in canvas points: where the view lands after a click here.
    /// The same box for both directions; zooming out, the current view shrinks into it.
    pub fn hover_box(&self) -> Option<[f32; 4]> {
        let (x, y) = self.hover?;
        if !self.show_box || self.press.as_ref().is_some_and(|p| p.dragged) || (self.target.deepest() && !self.zooms_out()) {
            return None;
        }
        let k = 1.0 / CLICK_ZOOM as f32;
        let (w, h) = self.size;
        Some([x - x * k, y - y * k, x + (w - x) * k, y + (h - y) * k])
    }

    /// Before any gesture, the view refits the whole set to the new size.
    fn resize(&mut self, width: f32, height: f32) {
        self.size = (width, height);
        self.refit();
    }

    /// A panel opens or closes: the whole set refits; any other view moves by
    /// half the panel's width, so what was centred in the free area stays centred.
    /// The explainer's width over the canvas; keyboard zoom centres on the area
    /// left of it.
    fn set_free_right(&mut self, free_right: f32) {
        self.free_right = free_right;
    }

    fn refit(&mut self) {
        let home = self.home_view();
        self.home_step = home.step;
        if !self.touched {
            self.view = home;
            self.target = home;
        }
        self.dirty = true;
    }

    /// Brings the view up to `now_ms`; true while an ease is still running.
    fn step_ease(&mut self, now_ms: f64) -> bool {
        let Some(ease) = self.ease.as_mut() else { return false };
        let start = *ease.start_ms.get_or_insert(now_ms);
        let t = ((now_ms - start) / EASE_MS).clamp(0.0, 1.0);
        let eased = 1.0 - (1.0 - t).powi(3);
        self.view = View::lerp(ease.from, self.target, eased);
        self.dirty = true;
        if t >= 1.0 {
            self.view = self.target;
            self.ease = None;
            return false;
        }
        true
    }

    fn zoom(&self) -> f64 {
        self.home_step / self.view.step
    }

    /// Iterations for the current zoom: deeper views need more to show their edges.
    fn max_iter(&self) -> u32 {
        if self.max_iter_cap > 0 {
            return self.max_iter_cap;
        }
        self.by_zoom().max(self.picture_iter).min(100_000)
    }

    /// Iterations for the zoom alone: deeper views need more to show their edges.
    fn by_zoom(&self) -> u32 {
        let depth = self.zoom().max(1.0).log2();
        ((200.0 + 60.0 * depth) * self.iter_scale) as u32
    }

    /// Sets the iterations from what the preview counted. No pixel escaping,
    /// with some left at the limit that are not inside the set, means a
    /// minibrot's edge is cut off, so the limit doubles, up to 16 times what the
    /// zoom alone picks. A view wholly inside the set does not raise it. Many escaping in the limit's top quarter
    /// means edges are cut off, so it doubles up to 4 times. Some late escapes
    /// are always there at any limit, so a few do not raise it. A highest escape
    /// under half the limit lowers it to twice that escape. Small changes are
    /// ignored, because each change restarts the pictures.
    fn take_stats(&mut self, stats: draw::Stats) {
        // A count taken at a limit far from the current one says nothing about it;
        // during a zoom the limit drifts a little every frame, which is close enough.
        let current = self.max_iter() as f64;
        if (stats.limit as f64 - current).abs() > current * 0.1 {
            return;
        }
        let near = stats.near_limit as f64 / stats.escaped.max(1) as f64;
        let wanted = if stats.escaped == 0 && stats.undecided > 0 {
            (stats.limit * 2).min(self.by_zoom() * 16)
        } else if near > LATE_SHARE && stats.limit < self.by_zoom() * 4 {
            (stats.limit * 2).min(self.by_zoom() * 4)
        } else if stats.max_escape * 2 < stats.limit {
            stats.max_escape * 2
        } else {
            return;
        };
        let current = self.max_iter().max(1);
        if wanted as f64 > current as f64 * 1.25 || (wanted as f64) < current as f64 * 0.5 {
            self.picture_iter = wanted.min(100_000);
            self.dirty = true;
        }
    }

    /// Steers the moving resolution towards a preview every few frames.
    fn adapt(&mut self, frames: u32) {
        if frames > 4 {
            self.moving_scale = (self.moving_scale * 0.8).max(0.03);
        } else if frames <= 2 {
            self.moving_scale = (self.moving_scale * 1.1).min(1.0);
        }
    }

    /// Jumps to a published coordinate at a zoom: `goto <re> <im> <zoom>`.
    fn goto(&mut self, args: &str) {
        let parts: Vec<&str> = args.split_whitespace().collect();
        let [x, y, zoom] = parts[..] else { return };
        let Ok(zoom) = zoom.parse::<f64>() else { return };
        let step = (self.home_step / zoom.max(1.0)).max(MIN_STEP);
        let bits = deep::bits_for(step / CLICK_ZOOM);
        let Some(anchor) = Point::parse(x, y, bits) else { return };
        self.anchor_f64 = anchor.approx();
        self.anchor = anchor;
        self.bits = bits;
        self.auto_target = (0.0, 0.0);
        self.reference = None;
        self.next = None;
        self.jump(View { cx: 0.0, cy: 0.0, step });
    }

    /// Moves the anchor to the view once the offset is long for its step, and
    /// raises the anchor's precision ahead of the zoom.
    fn reanchor(&mut self) {
        let step = self.view.step.min(self.target.step);
        let bits = deep::bits_for(step / CLICK_ZOOM);
        let far = self.view.cx.abs().max(self.view.cy.abs()) > ANCHOR_REACH_STEPS * self.view.step;
        if !far && bits <= self.bits {
            return;
        }
        let (dx, dy) = if far { (self.view.cx, self.view.cy) } else { (0.0, 0.0) };
        self.bits = self.bits.max(bits);
        self.anchor = self.anchor.offset(dx, dy, self.bits);
        self.anchor_f64 = self.anchor.approx();
        self.view = self.view.shifted(dx, dy);
        self.target = self.target.shifted(dx, dy);
        if let Some(ease) = self.ease.as_mut() {
            ease.from = ease.from.shifted(dx, dy);
        }
        if let Some(p) = self.preview.as_mut() {
            p.key.view = p.key.view.shifted(dx, dy);
        }
        if let Some(d) = self.drawing.as_mut() {
            d.key.view = d.key.view.shifted(dx, dy);
        }
        if let Some(sharp) = self.sharp.as_mut() {
            sharp.key.view = sharp.key.view.shifted(dx, dy);
        }
        for r in [self.reference.as_mut(), self.next.as_mut()].into_iter().flatten() {
            r.offset = (r.offset.0 - dx, r.offset.1 - dy);
        }
        self.auto_target = (self.auto_target.0 - dx, self.auto_target.1 - dy);
    }

    /// Whether `r` can draw the view: enough bits, and near enough for dc's f32.
    fn reaches(&self, r: &Reference, bits: usize) -> bool {
        let (dx, dy) = (self.target.cx - r.offset.0, self.target.cy - r.offset.1);
        r.orbit.p >= bits && dx.hypot(dy) / self.target.step < REFERENCE_REACH_PX
    }

    /// Builds references a budget at a time: a new one at the view's destination
    /// when the current one cannot draw it, else more of the current one.
    /// A reference that grows draws the same picture (the shader rebases at its
    /// end), so only a new reference changes the pictures.
    fn advance_reference(&mut self) {
        let len = self.max_iter() as usize + 2;
        let bits = deep::bits_for(self.target.step);
        let budget = (300_000 / self.bits).max(500);
        if !self.reference.as_ref().is_some_and(|r| self.reaches(r, bits)) && !self.next.as_ref().is_some_and(|r| self.reaches(r, bits)) {
            let offset = (self.target.cx, self.target.cy);
            let c = self.anchor.offset(offset.0, offset.1, self.bits);
            self.next = Some(Reference { orbit: Orbit::new(c, self.bits), offset });
        }
        if let Some(next) = self.next.as_mut() {
            // With nothing to draw from yet, the first reference is built at once.
            let budget = if self.reference.is_none() { usize::MAX } else { budget };
            next.orbit.advance(budget, len);
            if next.orbit.done(len) {
                self.reference = self.next.take();
                self.reference_generation += 1;
                self.uploaded = 0;
            }
            return;
        }
        let r = self.reference.as_mut().unwrap();
        r.orbit.advance(budget, len);
    }

    /// The view shown now.
    pub fn view(&self) -> View {
        self.view
    }

    /// The view an ease in flight ends at.
    pub fn target(&self) -> View {
        self.target
    }
}

impl Surface for MandelSurface {
    fn arguments(&self) -> Vec<(&'static str, Value)> {
        vec![
            ("maxIter", Value::Number(0.0)),
            ("autoZoom", Value::Number(0.0)),
            ("zoomOut", Value::Bool(false)),
            ("iterScale", Value::Number(1.0)),
            ("bla", Value::Bool(true)),
            ("hoverBox", Value::Bool(false)),
            ("autoRun", Value::Number(0.0)),
            ("freeRight", Value::Number(0.0)),
            ("theme", Value::Number(0.0)),
        ]
    }

    fn bind(&mut self, inputs: &[Value], _: Option<f64>) -> Result<(), SurfaceError> {
        let [max_iter, auto, zoom_out, iter_scale, bla, hover_box, auto_run, free_right, theme] = inputs else {
            return Err(SurfaceError(format!("mandel: expected 9 inputs, got {}", inputs.len())));
        };
        self.theme = number(theme, "theme")?.clamp(0.0, (THEMES - 1) as f64) as u32;
        self.set_auto_run(number(auto_run, "autoRun")?);
        self.set_free_right(number(free_right, "freeRight")?.max(0.0) as f32);
        let Value::Bool(bla) = bla else {
            return Err(SurfaceError("mandel.bla: expected a boolean".into()));
        };
        self.bla_off = !*bla;
        self.iter_scale = number(iter_scale, "iterScale")?.clamp(0.25, 64.0);
        let max_iter = number(max_iter, "maxIter")?;
        if !(0.0..=1_000_000.0).contains(&max_iter) {
            return Err(SurfaceError("mandel.maxIter: expected 0 to 1000000".into()));
        }
        let Value::Bool(zoom_out) = zoom_out else {
            return Err(SurfaceError("mandel.zoomOut: expected a boolean".into()));
        };
        self.max_iter_cap = max_iter as u32;
        self.auto_rate = number(auto, "autoZoom")?.clamp(0.0, 8.0);
        self.zoom_out_mode = *zoom_out;
        let Value::Bool(hover_box) = hover_box else {
            return Err(SurfaceError("mandel.hoverBox: expected a boolean".into()));
        };
        self.show_box = *hover_box;
        self.dirty = true;
        Ok(())
    }

    fn size(&mut self, width: f32, height: f32) -> bool {
        self.resize(width, height);
        true
    }

    fn wants_input(&self) -> bool {
        true
    }

    fn input(&mut self, event: &InputEvent) {
        match event {
            InputEvent::Pointer { id, phase, x, y, dx, dy, kind, buttons, alt_key, at_ms, .. } => {
                self.alt_held = *alt_key;
                if *kind != PointerKind::Touch {
                    self.hover = Some((*x, *y));
                }
                match phase {
                    PointerPhase::Down => {
                        // The secondary button zooms out, whatever the mode.
                        let zoom_out = self.zooms_out() || buttons & 2 != 0;
                        self.press = Some(Press { id: *id, x: *x, y: *y, at_ms: *at_ms, dragged: false, zoom_out });
                    }
                    PointerPhase::Move => {
                        let Some(press) = self.press.as_mut().filter(|p| p.id == *id) else { return };
                        let slop = if *kind == PointerKind::Touch { TOUCH_TAP_SLOP } else { TAP_SLOP };
                        if !press.dragged && ((x - press.x).powi(2) + (y - press.y).powi(2)).sqrt() > slop {
                            press.dragged = true;
                        }
                        if press.dragged {
                            self.pan(*dx, *dy);
                        }
                    }
                    PointerPhase::Up => {
                        if let Some(press) = self.press.take().filter(|p| p.id == *id) {
                            if !press.dragged && at_ms - press.at_ms < TAP_MS {
                                self.click(press.x, press.y, press.zoom_out);
                            }
                        }
                        if *kind == PointerKind::Touch {
                            self.hover = None;
                        }
                    }
                    PointerPhase::Cancel => self.press = None,
                }
            }
            InputEvent::Wheel { dy, x, y, alt_key, .. } => {
                self.alt_held = *alt_key;
                self.hover = Some((*x, *y));
                self.wheel(*dy, *x, *y);
            }
            InputEvent::Key { code, down, .. } if code.starts_with("Alt") => self.alt_held = *down,
            InputEvent::Blur { .. } => {
                self.alt_held = false;
                self.press = None;
            }
            InputEvent::Message { text, .. } => match text.as_str() {
                "hover-off" => self.hover = None,
                "reset" => self.reset(),
                "zoom-in" | "zoom-out" => {
                    let (w, h) = self.size;
                    self.click((w - self.free_right) * 0.5, h * 0.5, text == "zoom-out");
                }
                _ => {
                    if let Some(args) = text.strip_prefix("goto ") {
                        self.goto(args);
                    } else if let Some(args) = text.strip_prefix("pan ") {
                        let mut parts = args.split_whitespace().map(str::parse::<f32>);
                        if let (Some(Ok(dx)), Some(Ok(dy))) = (parts.next(), parts.next()) {
                            self.pan(dx, dy);
                        }
                    }
                }
            },
            _ => {}
        }
    }

    fn published(&mut self) -> Option<String> {
        if !std::mem::take(&mut self.dirty) {
            return None;
        }
        let zoom = self.home_step / self.target.step;
        let mut sorted = self.intervals.clone();
        sorted.sort_by(f64::total_cmp);
        let at = |q: f64| sorted.get(((sorted.len() as f64 - 1.0) * q).round() as usize).copied().unwrap_or(0.0);
        Some(format!(
            r#"{{"zoom":{},"zoomLabel":"{}","deepest":{},"autoRunning":{},"clicks":{},"frameP50":{:.1},"frameP95":{:.1},"frames":{},"scale":{:.2},"iterations":{},"bits":{},"orbit":{},"sharp":{},"bla":{}}}"#,
            zoom,
            zoom_label(zoom),
            self.target.deepest(),
            self.auto_running(),
            self.clicks,
            at(0.5),
            at(0.95),
            sorted.len(),
            self.moving_scale,
            self.max_iter(),
            self.bits,
            self.reference.as_ref().map_or(0, |r| r.orbit.points.len()),
            self.sharp.map_or(0, |sh| sh.samples),
            self.bla.map_or(0, |b| b.1)
        ))
    }

    fn render(
        &mut self,
        frame: &Frame,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::TextureView,
        format: wgpu::TextureFormat,
    ) -> bool {
        if self.gpu.as_ref().map(|g| (g.format, g.generation)) != Some((format, frame.shader_generation)) {
            self.gpu = Some(Gpu::new(device, queue, format, frame.shader_generation));
            self.uploaded = 0;
            self.preview = None;
            self.drawing = None;
            self.sharp = None;
        }
        if self.size != (frame.width, frame.height) {
            self.resize(frame.width, frame.height);
        }
        let now = frame.now_ms;
        let elapsed = self.last_ms.map_or(0.0, |last| (now - last).max(0.0));
        self.last_ms = Some(now);
        if self.last_motion_ms == f64::INFINITY {
            self.last_motion_ms = now;
        }

        let easing = self.step_ease(now);
        if self.auto_running() && !easing {
            let factor = (-(self.auto_rate * elapsed / 1000.0) * std::f64::consts::LN_2).exp();
            let (tx, ty) = self.auto_target;
            self.view = self.view.scaled_about(tx, ty, factor);
            self.target = self.view;
            self.dirty = true;
        }
        self.reanchor();
        self.advance_reference();
        if let Some(stats) = self.gpu.as_mut().unwrap().take_stats(device) {
            self.take_stats(stats);
        }
        let max_iter = self.max_iter();
        let building = self.next.is_some() || self.reference.as_ref().is_some_and(|r| !r.orbit.done(max_iter as usize + 2));
        let moving = easing || self.auto_running() || now - self.last_motion_ms < SETTLE_MS;
        if elapsed > 0.0 && elapsed < 1000.0 {
            if moving || self.banded {
                if self.intervals.len() == 240 {
                    self.intervals.remove(0);
                }
                self.intervals.push(elapsed);
                if now - self.stats_ms > 500.0 {
                    self.stats_ms = now;
                    self.dirty = true;
                }
            }
        }

        let (width, height) = frame.pixels();
        let dpr = frame.scale as f64;
        let key = Key { view: self.view, max_iter, reference: self.reference_generation, bla: !self.bla_off, theme: self.theme };
        let (hover_box, zoom_out) = (self.hover_box(), self.zooms_out());
        let reference = self.reference.as_ref().unwrap();
        let (ref_offset, orbit_len) = (reference.offset, reference.orbit.points.len());
        // The largest |dc| in the picture: the view's offset from the reference
        // and half the diagonal, at the preview's coarsest step.
        let half_diagonal = (width as f64).hypot(height as f64) * 0.5 / (dpr * 0.1);
        let cmax = (key.view.cx - ref_offset.0).hypot(key.view.cy - ref_offset.1) + half_diagonal * key.view.step;
        let done = reference.orbit.done(max_iter as usize + 2);
        // A table for the finished orbit, made again when the picture's |dc| outgrows it.
        let stale_bla = self.bla.is_none_or(|(generation, _, len, table_cmax)| {
            generation != self.reference_generation || (done && len != orbit_len) || cmax > table_cmax
        });
        let gpu = self.gpu.as_mut().unwrap();
        if stale_bla && done {
            let table = bla::build(&reference.orbit.points, cmax * 2.0);
            gpu.upload_bla(device, queue, &table.data);
            self.bla = Some((self.reference_generation, table.levels, table.len, table.cmax));
        } else if self.bla.is_some_and(|(generation, ..)| generation != self.reference_generation) || (stale_bla && self.bla.is_some_and(|b| cmax > b.3)) {
            self.bla = None;
        }
        if !gpu.has_orbit() {
            self.uploaded = 0;
        }
        gpu.upload_orbit(device, queue, &reference.orbit.points, self.uploaded);
        self.uploaded = orbit_len;
        let bla_now = match (self.bla, self.bla_off) {
            (Some((_, levels, len, _)), false) => (levels, len),
            _ => (0, 0),
        };
        if gpu.ensure_textures(device, width, height) {
            self.preview = None;
            self.drawing = None;
            self.sharp = None;
        }
        let pass = |key: &Key, (w, h): (u32, u32), rows: (u32, u32)| SetPass {
            width: w,
            height: h,
            step: key.view.step / (dpr * w as f64 / width as f64),
            dc: (key.view.cx - ref_offset.0, key.view.cy - ref_offset.1),
            max_iter: key.max_iter,
            theme: key.theme,
            orbit_len,
            bla: bla_now,
            jitter: (0.0, 0.0),
            rows,
            add: false,
            count: false,
        };

        // New work goes to the GPU only while at most one frame's is unfinished,
        // so putting the picture on screen never waits long behind it; a band
        // grows while the GPU keeps up and shrinks when work piles up.
        let in_flight = gpu.in_flight(device);
        let free = in_flight <= 1;
        if self.banded {
            let factor = match in_flight {
                0 => 1.15,
                1 => 1.0,
                _ => 0.7,
            };
            self.band_budget = (self.band_budget * factor).clamp(1e5, 1e10);
        }
        self.banded = false;
        if moving {
            self.sharp = None;
        }
        // A preview keeps the view it started with until done, so a moving
        // view still gets one every few frames; a new reference ends it.
        if self.drawing.is_some_and(|d| d.key.reference != key.reference) {
            self.drawing = None;
        }
        if self.drawing.is_none() && self.preview.is_none_or(|p| p.key != key) {
            let scale = self.moving_scale;
            let size = (((width as f32 * scale).round() as u32).clamp(1, width), ((height as f32 * scale).round() as u32).clamp(1, height));
            self.drawing = Some(Drawing { key, size, row: 0, frames: 0, count: gpu.can_count() });
        }
        let mut finished_in = None;
        if let Some(drawing) = self.drawing.as_mut() {
            drawing.frames += 1;
            if free {
                let (w, h) = drawing.size;
                let rows = (self.band_budget / (w as f64 * drawing.key.max_iter as f64)).clamp(1.0, h as f64) as u32;
                let end = (drawing.row + rows).min(h);
                let mut band = pass(&drawing.key, drawing.size, (drawing.row, end));
                band.count = drawing.count;
                gpu.draw_set(queue, encoder, Picture::Preview, &band);
                drawing.row = end;
                self.banded = true;
                if end == h {
                    let done = *drawing;
                    self.drawing = None;
                    gpu.swap_preview();
                    self.preview = Some(Preview { key: done.key, scale: w as f32 / width as f32 });
                    finished_in = moving.then_some(done.frames);
                }
            }
        } else if !moving && free {
            // At rest the sharp picture fills in a band of rows a frame.
            if self.sharp.is_none_or(|sh| sh.key != key) {
                self.sharp = Some(Sharp { key, samples: 0, row: 0 });
            }
            let sharp = self.sharp.as_mut().unwrap();
            if sharp.samples < SAMPLES {
                let rows = (self.band_budget / (width as f64 * max_iter as f64)).clamp(1.0, height as f64) as u32;
                let end = (sharp.row + rows).min(height);
                let mut band = pass(&key, (width, height), (sharp.row, end));
                band.jitter = jitter(sharp.samples);
                band.add = sharp.samples > 0;
                gpu.draw_set(queue, encoder, Picture::Sharp, &band);
                sharp.row = end;
                if end == height {
                    sharp.samples += 1;
                    sharp.row = 0;
                    self.dirty = true;
                }
                self.banded = true;
            }
        }

        let preview = self.preview.unwrap_or(Preview { key, scale: 0.0 });
        let k = key.view.step / preview.key.view.step;
        let px_step = preview.key.view.step / dpr;
        let (half_w, half_h) = (width as f64 * 0.5, height as f64 * 0.5);
        let show = Show {
            k,
            b: (
                (key.view.cx - preview.key.view.cx) / px_step + half_w - k * half_w,
                -(key.view.cy - preview.key.view.cy) / px_step + half_h - k * half_h,
            ),
            preview_scale: preview.scale,
            sharp: self.sharp.filter(|sh| sh.key == key).map(|sh| (sh.row, sh.samples)),
            hover_box,
            zoom_out,
            dpr: frame.scale,
        };
        gpu.draw_composite(queue, encoder, target, &show);
        let keep = moving || building || !free || self.drawing.is_some() || gpu.counting() || self.sharp.is_none_or(|sh| sh.samples < SAMPLES);
        if let Some(frames) = finished_in {
            self.adapt(frames);
        }
        keep
    }

    fn submitted(&mut self) {
        if let Some(gpu) = self.gpu.as_mut() {
            gpu.submitted();
        }
    }

    fn opaque(&self) -> bool {
        true
    }
}

/// Sample `index`'s shift within the pixel, along the R2 sequence (Martin Roberts, 2018); the first is unshifted.
/// A zoom for people: "4", "320,000", then "1.29 × 10²⁸" from a million.
fn zoom_label(zoom: f64) -> String {
    if zoom < 0.995 {
        let digits = if zoom < 0.01 { 4 } else { 2 };
        return format!("{zoom:.digits$}").trim_end_matches('0').to_string();
    }
    if zoom < 1e6 {
        let digits = (zoom.round() as u64).to_string();
        let mut out = String::new();
        for (i, d) in digits.chars().enumerate() {
            if i > 0 && (digits.len() - i) % 3 == 0 {
                out.push(',');
            }
            out.push(d);
        }
        return out;
    }
    let mut exponent = zoom.log10().floor() as i32;
    let mut mantissa = zoom / 10f64.powi(exponent);
    if mantissa >= 9.995 {
        mantissa /= 10.0;
        exponent += 1;
    }
    let sup: String = exponent.to_string().chars().map(|d| "⁰¹²³⁴⁵⁶⁷⁸⁹".chars().nth(d.to_digit(10).unwrap() as usize).unwrap()).collect();
    format!("{mantissa:.2} × 10{sup}")
}

fn jitter(index: u32) -> (f32, f32) {
    if index == 0 {
        return (0.0, 0.0);
    }
    let n = index as f64;
    (((n * 0.754_877_666_246_692_7).fract() - 0.5) as f32, ((n * 0.569_840_290_998_053_3).fract() - 0.5) as f32)
}

fn mandel_surface() -> Box<dyn Surface> {
    Box::new(MandelSurface::default())
}

/// The module's one surface and its reflected shaders.
pub static REGISTRY: Registry = Registry { surfaces: &[("mandel", 6, mandel_surface)], shaders: shaders::SHADERS };

exact_gpu::module!(REGISTRY);

#[cfg(test)]
mod tests {
    use super::*;

    fn surface() -> MandelSurface {
        let mut s = MandelSurface::default();
        s.resize(800.0, 600.0);
        s
    }

    #[test]
    fn a_click_keeps_its_point_and_zooms_in_then_out() {
        let mut s = surface();
        let before = s.target();
        let point = s.plane_at(before, 600.0, 150.0);
        s.click(600.0, 150.0, false);
        let zoomed = s.target();
        assert!((zoomed.step - before.step / CLICK_ZOOM).abs() < 1e-15);
        let still = s.plane_at(zoomed, 600.0, 150.0);
        assert!((still.0 - point.0).abs() < 1e-12 && (still.1 - point.1).abs() < 1e-12);
        s.click(600.0, 150.0, true);
        let back = s.target();
        assert!((back.cx - before.cx).abs() < 1e-12 && (back.cy - before.cy).abs() < 1e-12);
        assert!((back.step - before.step).abs() < 1e-15);
    }

    #[test]
    fn the_hover_box_is_where_the_view_lands() {
        let mut s = surface();
        s.hover = Some((600.0, 150.0));
        assert!(s.hover_box().is_none());
        s.show_box = true;
        let [x0, y0, x1, y1] = s.hover_box().unwrap();
        assert_eq!([x0, y0, x1, y1], [450.0, 112.5, 650.0, 262.5]);
        // The box's corners are the zoomed view's corners.
        let corner = s.plane_at(s.target(), x0, y0);
        s.click(600.0, 150.0, false);
        let landed = s.plane_at(s.target(), 0.0, 0.0);
        assert!((corner.0 - landed.0).abs() < 1e-12 && (corner.1 - landed.1).abs() < 1e-12);
    }

    #[test]
    fn an_ease_keeps_the_clicked_point_still() {
        let mut s = surface();
        let from = s.target();
        let point = s.plane_at(from, 600.0, 150.0);
        s.click(600.0, 150.0, false);
        for t in [0.1, 0.25, 0.5, 0.9] {
            let mid = View::lerp(from, s.target(), t);
            let at = s.plane_at(mid, 600.0, 150.0);
            assert!((at.0 - point.0).abs() < 1e-12 && (at.1 - point.1).abs() < 1e-12, "drifted at t = {t}");
        }
    }

    /// The shader's `INTERIOR`.
    const INTERIOR: f32 = 1.0e-6;

    /// The shader's loop in Rust, f32 as on the GPU.
    fn perturbed_count(orbit: &[[f32; 2]], dc: [f32; 2], max_iter: u32) -> u32 {
        perturbed_count_bla(orbit, None, dc, max_iter, false).0
    }

    /// The shader's loop with its BLA lookup, and with its interior exit when
    /// `exit` is set; also the iterations skipped, and whether it stopped inside.
    fn perturbed_count_bla(orbit: &[[f32; 2]], table: Option<&bla::Table>, dc: [f32; 2], max_iter: u32, exit: bool) -> (u32, u32, bool) {
        let mul = |a: [f32; 2], b: [f32; 2]| [a[0] * b[0] - a[1] * b[1], a[0] * b[1] + a[1] * b[0]];
        let (mut dz, mut m, mut n, mut skipped_total) = ([0.0f32, 0.0], 0usize, 0u32, 0u32);
        let mut dzz = [1.0f32, 0.0];
        let inside = |dzz: [f32; 2]| exit && dzz[0] * dzz[0] + dzz[1] * dzz[1] < INTERIOR;
        while n < max_iter {
            if let Some(t) = table.filter(|t| m > 0 && m + 1 < t.len && t.levels > 0) {
                let dz_length = dz[0].hypot(dz[1]);
                let fit = |k: usize| {
                    let header = t.data[k];
                    let j = (m - 1) >> k;
                    if j >= header[1] as usize {
                        return None;
                    }
                    let at = bla::HEADER + 2 * (header[0] as usize + j);
                    let l = t.data[at + 1][1] as u32;
                    (dz_length < t.data[at + 1][0] && n + l <= max_iter && m + (l as usize) < orbit.len()).then_some(at)
                };
                let mut skipped = false;
                if let Some(mut at) = fit(0) {
                    let (mut lo, mut hi) = (0, ((m - 1).trailing_zeros() as usize).min(t.levels as usize - 1));
                    while lo < hi {
                        let mid = (lo + hi + 1) / 2;
                        match fit(mid) {
                            Some(found) => (lo, at) = (mid, found),
                            None => hi = mid - 1,
                        }
                    }
                    let (ab, l) = (t.data[at], t.data[at + 1][1] as u32);
                    dzz = mul([ab[0], ab[1]], dzz);
                    let a = mul([ab[0], ab[1]], dz);
                    let b = mul([ab[2], ab[3]], dc);
                    dz = [a[0] + b[0], a[1] + b[1]];
                    m += l as usize;
                    n += l;
                    skipped_total += l;
                    skipped = true;
                }
                if skipped {
                    let z = [orbit[m][0] + dz[0], orbit[m][1] + dz[1]];
                    let r2 = z[0] * z[0] + z[1] * z[1];
                    if r2 > 65536.0 {
                        return (n, skipped_total, false);
                    }
                    if r2 < dz[0] * dz[0] + dz[1] * dz[1] || m + 1 >= orbit.len() {
                        dz = z;
                        m = 0;
                    }
                    if inside(dzz) {
                        return (n, skipped_total, true);
                    }
                    continue;
                }
            }
            let zm = orbit[m];
            if n > 0 {
                let d = mul([2.0 * (zm[0] + dz[0]), 2.0 * (zm[1] + dz[1])], dzz);
                dzz = d;
            }
            let a = mul([2.0 * zm[0], 2.0 * zm[1]], dz);
            let b = mul(dz, dz);
            dz = [a[0] + b[0] + dc[0], a[1] + b[1] + dc[1]];
            m += 1;
            let z = [orbit[m][0] + dz[0], orbit[m][1] + dz[1]];
            n += 1;
            let r2 = z[0] * z[0] + z[1] * z[1];
            if r2 > 65536.0 {
                return (n, skipped_total, false);
            }
            if r2 < dz[0] * dz[0] + dz[1] * dz[1] || m + 1 >= orbit.len() {
                dz = z;
                m = 0;
            }
            if inside(dzz) {
                return (n, skipped_total, true);
            }
        }
        (n, skipped_total, false)
    }

    /// Over a grid of pixels at four depths, the interior exit stops only
    /// pixels that never escape without it, and saves most of their work. On
    /// a 120 × 80 grid no escaping pixel stopped at 1e-4 either; 0.1 stops some.
    #[test]
    fn the_interior_exit_never_stops_an_escaping_pixel() {
        let deep = ("-1.7499357218920984460646651243594", "0.0000000890808697365708495087578");
        // Centre, view width, limit: the whole set, Seahorse Valley at 256×,
        // the deep point 1e-27 wide, and the minibrot at about 10²⁹×.
        let views = [
            (("-0.5", "0"), 3.0, 2_000u32),
            (("-0.7453", "0.1127"), 3.0 / 256.0, 4_000),
            (deep, 1e-27, 8_000),
            (deep, 2.4e-29, 30_000),
        ];
        let (w, h) = (60, 40);
        let (mut all_saved, mut all_work) = (0u64, 0u64);
        for ((x, y), width, max_iter) in views {
            let step = width / w as f64;
            let bits = deep::bits_for(step).max(64);
            let centre = Point::parse(x, y, bits).unwrap();
            let mut orbit = Orbit::new(centre, bits);
            orbit.advance(usize::MAX, max_iter as usize + 2);
            let table = bla::build(&orbit.points, step * w.max(h) as f64);
            let (mut inside, mut stopped, mut saved, mut work) = (0, 0, 0u64, 0u64);
            for j in 0..h {
                for i in 0..w {
                    let dc = [((i as f64 - w as f64 / 2.0) * step) as f32, ((h as f64 / 2.0 - j as f64) * step) as f32];
                    let (full, ..) = perturbed_count_bla(&orbit.points, Some(&table), dc, max_iter, false);
                    let (n, _, stop) = perturbed_count_bla(&orbit.points, Some(&table), dc, max_iter, true);
                    if stop {
                        assert_eq!(full, max_iter, "a pixel at {dc:?} in the {width:e} view escapes at {full}, but the exit stopped it at {n}");
                        stopped += 1;
                    } else {
                        assert_eq!(n, full, "the exit changed a count at {dc:?}");
                    }
                    if full == max_iter {
                        inside += 1;
                        saved += (max_iter - n) as u64;
                        work += max_iter as u64;
                    }
                }
            }
            println!("{width:e}: {stopped} of {inside} pixels at the limit stopped early, saving {saved} of {work} iterations");
            all_saved += saved;
            all_work += work;
        }
        assert!(all_saved * 2 > all_work, "the exit saved under half the work at the limit");
    }

    #[test]
    fn bla_counts_match_direct_iteration_and_skip_most() {
        let (x, y) = ("-1.7499357218920984460646651243594", "0.0000000890808697365708495087578");
        let bits = 192;
        let centre = Point::parse(x, y, bits).unwrap();
        let max_iter = 30_000;
        let mut orbit = Orbit::new(centre.clone(), bits);
        orbit.advance(usize::MAX, max_iter as usize + 2);
        let table = bla::build(&orbit.points, 2e-29);
        let (mut matched, mut total, mut skipped, mut iterated) = (0, 0, 0u64, 0u64);
        for (dx, dy) in [(1.1e-29, 0.0), (0.6e-29, 0.3e-29), (0.3e-29, -0.5e-29), (-1.1e-29, 0.7e-29), (-0.6e-29, 0.0), (0.0, 1.0e-29)] {
            let direct = deep::direct_count(&centre.offset(dx, dy, bits), bits, max_iter);
            let (n, s, _) = perturbed_count_bla(&orbit.points, Some(&table), [dx as f32, dy as f32], max_iter, false);
            println!("({dx:e},{dy:e}): direct {direct} bla {n}, skipped {s}");
            total += 1;
            if n == direct {
                matched += 1;
            }
            skipped += s as u64;
            iterated += n as u64;
        }
        println!("skipped {skipped} of {iterated}");
        assert_eq!(matched, total, "BLA counts must match direct counts");
        assert!(skipped * 2 > iterated, "BLA should skip most iterations here");
    }

    #[test]
    fn perturbed_counts_match_direct_iteration_at_1e_minus_27() {
        // A published deep coordinate (mrob.com, "deepest"), at a view 1e-27 wide.
        let (x, y) = ("-1.7499357218920984460646651243594", "0.0000000890808697365708495087578");
        let step = 1e-27 / 1000.0;
        let bits = deep::bits_for(step);
        let centre = Point::parse(x, y, bits).unwrap();
        let max_iter = 4000;
        let mut orbit = Orbit::new(centre.clone(), bits);
        orbit.advance(usize::MAX, max_iter as usize + 2);
        let mut matched = 0;
        let mut escaped = 0;
        for i in 0..16 {
            let (px, py) = ((i % 4) as f64 * 300.0 - 450.0, (i / 4) as f64 * 300.0 - 450.0);
            let c = centre.offset(px * step, py * step, bits);
            let direct = deep::direct_count(&c, bits, max_iter);
            let perturbed = perturbed_count(&orbit.points, [(px * step) as f32, (py * step) as f32], max_iter);
            if direct < max_iter {
                escaped += 1;
            }
            if direct == perturbed {
                matched += 1;
            }
        }
        assert!(escaped > 0, "no sample escaped, so the test shows nothing");
        assert_eq!(matched, 16, "perturbed counts must match direct counts");
    }

    /// Perturbed against direct counts across a 2.4e-29-wide view at the deep point, at high limits.
    #[test]
    #[ignore]
    fn probe_long_iterations() {
        let (x, y) = ("-1.7499357218920984460646651243594", "0.0000000890808697365708495087578");
        let bits = 192;
        let centre = Point::parse(x, y, bits).unwrap();
        for max_iter in [6_000u32, 30_000, 88_000] {
            let mut orbit = Orbit::new(centre.clone(), bits);
            orbit.advance(usize::MAX, max_iter as usize + 2);
            for (dx, dy) in [(1.1e-29, 0.0), (0.6e-29, 0.3e-29), (0.3e-29, -0.5e-29), (-1.1e-29, 0.7e-29), (-0.6e-29, 0.0)] {
                let c = centre.offset(dx, dy, bits);
                let direct = deep::direct_count(&c, bits, max_iter);
                let perturbed = perturbed_count(&orbit.points, [dx as f32, dy as f32], max_iter);
                println!("limit {max_iter} orbit {} offset ({dx:e},{dy:e}): direct {direct} perturbed {perturbed}", orbit.points.len());
            }
        }
    }

    #[test]
    fn zoom_labels_read_as_people_write_them() {
        assert_eq!(zoom_label(1.0), "1");
        assert_eq!(zoom_label(0.25), "0.25");
        assert_eq!(zoom_label(0.0625), "0.06");
        assert_eq!(zoom_label(4.0), "4");
        assert_eq!(zoom_label(320_000.0), "320,000");
        assert_eq!(zoom_label(1.2864e28), "1.29 × 10²⁸");
        assert_eq!(zoom_label(9.999e6), "1.00 × 10⁷");
    }

    #[test]
    fn a_click_stops_the_automatic_zoom_until_the_page_starts_it_again() {
        let mut s = surface();
        s.auto_rate = 3.0;
        s.set_auto_run(1.0);
        s.click(400.0, 300.0, false);
        assert!(!s.auto_running());
        s.set_auto_run(1.0);
        assert!(!s.auto_running());
        s.set_auto_run(2.0);
        assert!(s.auto_running());
    }

    #[test]
    fn opening_the_panel_never_moves_the_view() {
        let mut s = surface();
        let home = s.target;
        s.set_free_right(400.0);
        assert_eq!(s.target, home);
        s.click(300.0, 200.0, false);
        s.view = s.target;
        s.ease = None;
        let zoomed = s.target;
        s.set_free_right(0.0);
        assert_eq!((s.view, s.target), (zoomed, zoomed));
        s.set_free_right(400.0);
        s.reset();
        assert_eq!(s.target, home);
    }

    #[test]
    fn the_zoom_stops_where_f32_runs_out() {
        let mut s = surface();
        let deep = s.home_step / MIN_STEP;
        s.goto(&format!("-0.75 0.1 {}", deep / 2.0));
        assert!(!s.target.deepest());
        let point = s.plane_at(s.target, 600.0, 150.0);
        s.click(600.0, 150.0, false);
        assert!(s.target.deepest() && s.target.step == MIN_STEP);
        let still = s.plane_at(s.target, 600.0, 150.0);
        assert!((still.0 - point.0).abs() < MIN_STEP && (still.1 - point.1).abs() < MIN_STEP);
        let stopped = s.target;
        s.click(600.0, 150.0, false);
        assert_eq!(s.target, stopped);
        s.view = s.target;
        s.wheel(-500.0, 100.0, 100.0);
        assert_eq!(s.target, stopped);
        s.click(600.0, 150.0, true);
        assert!(s.target.step > stopped.step * 3.9);
    }

    fn pointer(phase: PointerPhase, x: f32, y: f32, kind: PointerKind, at_ms: f64) -> InputEvent {
        InputEvent::Pointer { id: 1, phase, x, y, dx: 0.0, dy: 0.0, kind, buttons: 1, shift_key: false, ctrl_key: false, alt_key: false, meta_key: false, at_ms }
    }

    #[test]
    fn a_finger_that_drifts_a_little_still_taps() {
        for (kind, taps) in [(PointerKind::Touch, true), (PointerKind::Mouse, false)] {
            let mut s = surface();
            let step = s.target.step;
            s.input(&pointer(PointerPhase::Down, 400.0, 300.0, kind, 0.0));
            s.input(&pointer(PointerPhase::Move, 406.0, 306.0, kind, 40.0));
            s.input(&pointer(PointerPhase::Up, 406.0, 306.0, kind, 80.0));
            assert_eq!(s.target.step < step, taps, "{kind:?}");
        }
    }

    #[test]
    fn option_flips_the_direction() {
        let mut s = surface();
        assert!(!s.zooms_out());
        s.alt_held = true;
        assert!(s.zooms_out());
        s.zoom_out_mode = true;
        assert!(!s.zooms_out());
    }
}
