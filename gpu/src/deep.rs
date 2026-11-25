//! High precision for deep zoom: a plane point in `astro-float`, and a
//! reference orbit computed in that precision a budget of iterations at a time,
//! kept as f32 for the GPU (K. I. Martin's perturbation; the shader rebases,
//! after Zhuoran, 2021).

use astro_float::{BigFloat, Consts, Radix, RoundingMode, Sign, Word};

const RM: RoundingMode = RoundingMode::ToEven;
/// |Z|² past this ends the orbit, as the shader's bailout.
const BAILOUT: f64 = 65536.0;

/// Bits for a view whose point is `step` wide: the zoom's own bits and 64 more,
/// rounded up to 64 so a small zoom does not change the precision every frame.
pub fn bits_for(step: f64) -> usize {
    let needed = 64 + (-step.log2()).max(0.0).ceil() as usize;
    needed.div_ceil(64) * 64
}

/// The value as f64, from the mantissa's two most significant words (Word is
/// u64 on 64-bit targets and u32 on wasm32).
pub fn to_f64(x: &BigFloat) -> f64 {
    let Some((m, _, sign, e, _)) = x.as_raw_parts() else { return f64::NAN };
    let bits = Word::BITS as i32;
    let top = m
        .iter()
        .rev()
        .take(2)
        .enumerate()
        .fold(0.0f64, |acc, (i, w)| acc + *w as f64 * 2f64.powi(-bits * (i as i32 + 1)));
    let v = top * 2f64.powi(e);
    if sign == Sign::Neg {
        -v
    } else {
        v
    }
}

/// A plane point in high precision.
#[derive(Clone, Debug)]
pub struct Point {
    pub x: BigFloat,
    pub y: BigFloat,
}

impl Point {
    pub fn from_f64(x: f64, y: f64, p: usize) -> Point {
        Point { x: BigFloat::from_f64(x, p), y: BigFloat::from_f64(y, p) }
    }

    /// A point from decimal strings, such as a published deep coordinate.
    pub fn parse(x: &str, y: &str, p: usize) -> Option<Point> {
        let mut cc = Consts::new().ok()?;
        let x = BigFloat::parse(x, Radix::Dec, p, RM, &mut cc);
        let y = BigFloat::parse(y, Radix::Dec, p, RM, &mut cc);
        (!x.is_nan() && !y.is_nan()).then_some(Point { x, y })
    }

    /// This point moved by an f64 offset, in `p` bits.
    pub fn offset(&self, dx: f64, dy: f64, p: usize) -> Point {
        Point {
            x: self.x.add(&BigFloat::from_f64(dx, p), p, RM),
            y: self.y.add(&BigFloat::from_f64(dy, p), p, RM),
        }
    }

    pub fn approx(&self) -> (f64, f64) {
        (to_f64(&self.x), to_f64(&self.y))
    }
}

/// Z₀ = 0, Zₙ₊₁ = Zₙ² + C in `p` bits, kept as f32 pairs.
pub struct Orbit {
    pub c: Point,
    pub p: usize,
    zx: BigFloat,
    zy: BigFloat,
    pub points: Vec<[f32; 2]>,
    pub escaped: bool,
}

impl Orbit {
    pub fn new(c: Point, p: usize) -> Orbit {
        Orbit { zx: BigFloat::from_f64(0.0, p), zy: BigFloat::from_f64(0.0, p), c, p, points: vec![[0.0, 0.0]], escaped: false }
    }

    /// Whether the orbit is as long as `len` points or has escaped.
    pub fn done(&self, len: usize) -> bool {
        self.escaped || self.points.len() >= len
    }

    /// Up to `budget` more iterations towards `len` points.
    pub fn advance(&mut self, budget: usize, len: usize) {
        let p = self.p;
        for _ in 0..budget {
            if self.done(len) {
                return;
            }
            let xx = self.zx.mul(&self.zx, p, RM);
            let yy = self.zy.mul(&self.zy, p, RM);
            let xy = self.zx.mul(&self.zy, p, RM);
            self.zx = xx.sub(&yy, p, RM).add(&self.c.x, p, RM);
            self.zy = xy.add(&xy, p, RM).add(&self.c.y, p, RM);
            let (x, y) = (to_f64(&self.zx), to_f64(&self.zy));
            self.points.push([x as f32, y as f32]);
            if x * x + y * y > BAILOUT {
                self.escaped = true;
            }
        }
    }
}

/// The escape count of plane point `c` by direct iteration in `p` bits: the
/// slow, certain answer the perturbed count is checked against.
#[cfg(test)]
pub fn direct_count(c: &Point, p: usize, max_iter: u32) -> u32 {
    let mut orbit = Orbit::new(c.clone(), p);
    orbit.advance(max_iter as usize, max_iter as usize + 1);
    // The count is the number of updates before |z|² passed the bailout.
    (orbit.points.len() - 1) as u32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn to_f64_reads_back_what_from_f64_wrote() {
        for v in [1.0, -0.6, 0.131_825_904_205_33, -1.749_935_721_892_098_3, 3.5e-28, -2.0e-30, 65536.0] {
            assert_eq!(to_f64(&BigFloat::from_f64(v, 256)), v);
        }
    }

    #[test]
    fn bits_grow_with_zoom() {
        assert_eq!(bits_for(0.002), 128);
        assert_eq!(bits_for(1e-30), 192);
        assert!(bits_for(1e-100) >= 64 + 333);
    }
}

#[cfg(test)]
mod probe {
    use super::*;

    /// Escape counts across a view 2.3e-29 wide at the published deep point.
    #[test]
    #[ignore]
    fn counts_at_the_deep_point() {
        let p = 192;
        let c = Point::parse("-1.7499357218920984460646651243594", "0.0000000890808697365708495087578", p).unwrap();
        for (dx, dy) in [(0.0, 0.0), (1.1e-29, 0.0), (-1.1e-29, 0.7e-29), (0.0, -0.7e-29), (5e-30, 5e-30)] {
            let n = direct_count(&c.offset(dx, dy, p), p, 200_000);
            println!("offset ({dx:e}, {dy:e}): {n}");
        }
    }
}
