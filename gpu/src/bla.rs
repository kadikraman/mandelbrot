//! Bilinear approximation (BLA): a table that lets a pixel skip l iterations
//! at once with dz ← A·dz + B·dc while |dz| < r (Zhuoran, 2021; formulas from
//! mathr.co.uk, "Deep zoom theory and practice (again)", 2022).
//!
//! Level 0 holds one step for each reference iteration m ≥ 1: A = 2Zₘ, B = 1,
//! r = max(0, ε(|Zₘ|/2 − max|c|) / (|2Zₘ| + 1)), halving |Z| as mathr does to
//! be safe. Level k merges pairs of level k − 1 without overlap, so its entry
//! j starts at iteration 1 + j·2ᵏ and skips 2ᵏ. Merging x then y:
//! A = A_y·A_x, B = A_y·B_x + B_y, r = min(r_x, max(0, (r_y − |B_x|·max|c|) / |A_x|)).

/// f32's precision, the ε of the validity radius.
const EPSILON: f64 = 1.0 / 16_777_216.0;
/// Header slots: one `[offset, count, 0, 0]` per level, before the entries.
pub const HEADER: usize = 32;
/// An entry past this |A| or |B| would overflow f32 on the GPU; it is left invalid.
const LARGEST: f64 = 1e30;

#[derive(Clone, Copy)]
struct Entry {
    a: (f64, f64),
    b: (f64, f64),
    r: f64,
    l: u32,
}

fn mul(x: (f64, f64), y: (f64, f64)) -> (f64, f64) {
    (x.0 * y.0 - x.1 * y.1, x.0 * y.1 + x.1 * y.0)
}

fn abs(x: (f64, f64)) -> f64 {
    x.0.hypot(x.1)
}

/// The table for an orbit, valid while every pixel's |dc| is at most `cmax`,
/// laid out for the GPU: the level header, then two `vec4`s an entry,
/// `[A.x, A.y, B.x, B.y]` and `[r, l, 0, 0]`.
pub struct Table {
    pub cmax: f64,
    /// Orbit points the table covers.
    pub len: usize,
    pub levels: u32,
    pub data: Vec<[f32; 4]>,
}

pub fn build(orbit: &[[f32; 2]], cmax: f64) -> Table {
    let mut level: Vec<Entry> = orbit
        .iter()
        .enumerate()
        .skip(1)
        .take(orbit.len().saturating_sub(2))
        .map(|(_, z)| {
            let z = (z[0] as f64, z[1] as f64);
            let a = (2.0 * z.0, 2.0 * z.1);
            let r = (EPSILON * (abs(z) * 0.5 - cmax) / (abs(a) + 1.0)).max(0.0);
            Entry { a, b: (1.0, 0.0), r, l: 1 }
        })
        .collect();
    let mut header = Vec::new();
    let mut entries: Vec<[f32; 4]> = Vec::new();
    while !level.is_empty() && header.len() < HEADER {
        header.push([(entries.len() / 2) as f32, level.len() as f32, 0.0, 0.0]);
        for e in &level {
            let usable = e.r > 0.0 && abs(e.a) < LARGEST && abs(e.b) < LARGEST;
            let r = if usable { e.r } else { 0.0 };
            entries.push([e.a.0 as f32, e.a.1 as f32, e.b.0 as f32, e.b.1 as f32]);
            entries.push([r as f32, e.l as f32, 0.0, 0.0]);
        }
        level = level
            .chunks_exact(2)
            .map(|pair| {
                let (x, y) = (pair[0], pair[1]);
                let ax = abs(x.a);
                let r_after = if ax > 0.0 { ((y.r - abs(x.b) * cmax) / ax).max(0.0) } else { 0.0 };
                Entry { a: mul(y.a, x.a), b: { let ab = mul(y.a, x.b); (ab.0 + y.b.0, ab.1 + y.b.1) }, r: x.r.min(r_after), l: x.l + y.l }
            })
            .collect();
    }
    let levels = header.len() as u32;
    header.resize(HEADER, [0.0; 4]);
    header.extend(entries);
    Table { cmax, len: orbit.len(), levels, data: header }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn levels_halve_and_start_at_one() {
        let orbit: Vec<[f32; 2]> = (0..18).map(|i| [0.5 + i as f32 * 0.01, 0.1]).collect();
        let t = build(&orbit, 1e-20);
        // 16 single steps (iterations 1 to 16), then 8, 4, 2, 1.
        assert_eq!(t.levels, 5);
        let counts: Vec<f32> = (0..5).map(|k| t.data[k][1]).collect();
        assert_eq!(counts, [16.0, 8.0, 4.0, 2.0, 1.0]);
        // The top entry skips 16.
        let top = t.data[4][0] as usize;
        assert_eq!(t.data[HEADER + 2 * top + 1][1], 16.0);
    }
}
