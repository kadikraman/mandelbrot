// The Mandelbrot set by perturbation: each pixel iterates its small difference
// dz from a reference orbit Z computed in high precision on the CPU, as
// dz ← 2·Z·dz + dz² + dc, and rebases (z becomes the new dz, the reference
// restarts at 0) when |Z + dz| < |dz| or the reference runs out (Zhuoran, 2021;
// see mathr.co.uk, "Deep zoom theory and practice (again)", 2022).
// The output is one sample; the composite averages them.
struct Uniforms {
    // The view's centre minus the reference's point, in plane units.
    dcx: f32,
    dcy: f32,
    step: f32,
    width: f32,
    height: f32,
    max_iter: f32,
    jitter_x: f32,
    jitter_y: f32,
    orbit_len: f32,
    // 1 to count into `stats`: the preview counts, so the CPU can pick the iterations.
    count_stats: f32,
    // BLA levels in `bla` (0: none yet), and the orbit points the table covers.
    bla_levels: f32,
    bla_len: f32,
    // The palette: 0 Warm, 1 Classic, 2 Ice, 3 Magma, 4 Ink, 5 Cool.
    theme: f32,
}

@group(0) @binding(0) var<uniform> u: Uniforms;
@group(0) @binding(1) var<storage, read> orbit: array<vec2<f32>>;
// [0] the highest escape count, [1] pixels that did not escape, [2] pixels that
// escaped, [3] pixels that escaped in the top quarter of the limit.
@group(0) @binding(2) var<storage, read_write> stats: array<atomic<u32>, 4>;
// The BLA table (bla.rs): 32 level headers [offset, count, 0, 0], then two
// vec4s an entry, [A, B] and [r, l, 0, 0]; level k's entry j starts at
// reference iteration 1 + j·2ᵏ and skips 2ᵏ.
@group(0) @binding(3) var<storage, read> bla: array<vec4<f32>>;

@vertex
fn vs(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
    return vec4<f32>(f32(i32(i & 1u) * 4 - 1), f32(i32(i >> 1u) * 4 - 1), 0.0, 1.0);
}

// Five colours around a loop, eased from one to the next as t goes 0 to 1.
fn ramp(t: f32, c0: vec3<f32>, c1: vec3<f32>, c2: vec3<f32>, c3: vec3<f32>, c4: vec3<f32>) -> vec3<f32> {
    let x = fract(t) * 5.0;
    let i = u32(x);
    let f = smoothstep(0.0, 1.0, fract(x));
    var a = c4;
    var b = c0;
    if i == 0u { a = c0; b = c1; }
    if i == 1u { a = c1; b = c2; }
    if i == 2u { a = c2; b = c3; }
    if i == 3u { a = c3; b = c4; }
    return mix(a, b, f);
}

// Warm and Cool are Inigo Quilez's cosine palette, a + b·cos(2π(c·t + d)).
// Classic is Ultra Fractal's default gradient.
fn palette(t: f32, theme: u32) -> vec3<f32> {
    switch theme {
        case 1u {
            return ramp(t, vec3(0.0, 0.03, 0.39), vec3(0.13, 0.42, 0.8), vec3(0.93, 1.0, 1.0), vec3(1.0, 0.67, 0.0), vec3(0.0, 0.01, 0.0));
        }
        case 2u {
            return ramp(t, vec3(0.01, 0.02, 0.08), vec3(0.03, 0.17, 0.36), vec3(0.15, 0.55, 0.75), vec3(0.75, 0.95, 1.0), vec3(0.1, 0.3, 0.5));
        }
        case 3u {
            return ramp(t, vec3(0.02, 0.01, 0.06), vec3(0.32, 0.07, 0.47), vec3(0.72, 0.21, 0.47), vec3(0.98, 0.53, 0.31), vec3(0.99, 0.95, 0.75));
        }
        case 4u {
            return ramp(t, vec3(0.95, 0.93, 0.88), vec3(0.7, 0.69, 0.66), vec3(0.35, 0.35, 0.35), vec3(0.6, 0.59, 0.57), vec3(0.88, 0.86, 0.82));
        }
        case 5u {
            return 0.5 + 0.5 * cos(6.2831853 * (t + vec3<f32>(0.3, 0.2, 0.2)));
        }
        default {
            return 0.5 + 0.5 * cos(6.2831853 * (t + vec3<f32>(0.0, 0.10, 0.20)));
        }
    }
}

// |dz/dz₁|² below which a pixel is taken to be inside the set and stops. The
// Rust test `the_interior_exit_never_stops_an_escaping_pixel` checks it.
const INTERIOR: f32 = 1.0e-6;

fn mul(a: vec2<f32>, b: vec2<f32>) -> vec2<f32> {
    return vec2<f32>(a.x * b.x - a.y * b.y, a.x * b.y + a.y * b.x);
}

// Where level k's entry starting at reference iteration m is, if it exists and
// its skip fits this |dz|, the limit and the orbit; else 0.
fn bla_fit(k: u32, m: u32, n: u32, limit: u32, len: u32, dz_length: f32) -> u32 {
    let header = bla[k];
    let j = (m - 1u) >> k;
    if j >= u32(header.y) {
        return 0u;
    }
    let at = 32u + 2u * (u32(header.x) + j);
    let rl = bla[at + 1u];
    let l = u32(rl.y);
    if dz_length < rl.x && n + l <= limit && m + l < len {
        return at;
    }
    return 0u;
}

@fragment
fn fs(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let pixel = position.xy + vec2<f32>(u.jitter_x, u.jitter_y);
    let offset = vec2<f32>(pixel.x - 0.5 * u.width, 0.5 * u.height - pixel.y);
    let dc = vec2<f32>(u.dcx, u.dcy) + offset * u.step;
    let len = u32(u.orbit_len);
    let limit = u32(u.max_iter);
    var dz = vec2<f32>(0.0, 0.0);
    // dz/dc times the step: the derivative in pixels, which stays in f32's range at depth.
    var der = vec2<f32>(0.0, 0.0);
    // dz/dz₁, the orbit's derivative from its first point (z₀ = 0 would make it
    // 0): it shrinks towards zero inside the set and grows outside, so a pixel
    // can stop early inside, and a pixel left at the limit can be told apart.
    let counting = u.count_stats > 0.5;
    var dzz = vec2<f32>(1.0, 0.0);
    var interior = false;
    var z = vec2<f32>(0.0, 0.0);
    var m = 0u;
    var n = 0u;
    var escaped = false;
    let levels = u32(u.bla_levels);
    let bla_len = u32(u.bla_len);
    loop {
        if n >= limit {
            break;
        }
        // The longest skip that starts here and is valid for this |dz|. A level's
        // radius is never above the level below it at the same start, so when
        // level 0 does not fit nothing does, and otherwise a binary search finds
        // the highest that fits.
        var skipped = false;
        if m > 0u && m + 1u < bla_len && levels > 0u {
            let dz_length = length(dz);
            var at = bla_fit(0u, m, n, limit, len, dz_length);
            if at != 0u {
                var lo = 0u;
                var hi = min(countTrailingZeros(m - 1u), levels - 1u);
                while lo < hi {
                    let mid = (lo + hi + 1u) / 2u;
                    let found = bla_fit(mid, m, n, limit, len, dz_length);
                    if found != 0u {
                        lo = mid;
                        at = found;
                    } else {
                        hi = mid - 1u;
                    }
                }
                let ab = bla[at];
                let l = u32(bla[at + 1u].y);
                der = mul(ab.xy, der) + ab.zw * u.step;
                dzz = mul(ab.xy, dzz);
                dz = mul(ab.xy, dz) + mul(ab.zw, dc);
                m = m + l;
                n = n + l;
                skipped = true;
            }
        }
        if skipped {
            z = orbit[m] + dz;
            let r2 = dot(z, z);
            if r2 > 65536.0 {
                escaped = true;
                break;
            }
            if r2 < dot(dz, dz) || m + 1u >= len {
                dz = z;
                m = 0u;
            }
            if dot(dzz, dzz) < INTERIOR {
                interior = true;
                break;
            }
            continue;
        }
        let zm = orbit[m];
        der = 2.0 * mul(zm + dz, der) + vec2<f32>(u.step, 0.0);
        if n > 0u {
            dzz = 2.0 * mul(zm + dz, dzz);
        }
        dz = 2.0 * mul(zm, dz) + mul(dz, dz) + dc;
        m = m + 1u;
        z = orbit[m] + dz;
        n = n + 1u;
        let r2 = dot(z, z);
        if r2 > 65536.0 {
            escaped = true;
            break;
        }
        if r2 < dot(dz, dz) || m + 1u >= len {
            dz = z;
            m = 0u;
        }
        if dot(dzz, dzz) < INTERIOR {
            interior = true;
            break;
        }
    }
    if counting {
        if !escaped && !interior {
            // Overflow to infinity or NaN also fails the test: outside, not yet escaped.
            if !(dot(dzz, dzz) < 1.0) {
                atomicAdd(&stats[1], 1u);
            }
        } else {
            atomicAdd(&stats[2], 1u);
            if n * 4u > limit * 3u {
                atomicAdd(&stats[3], 1u);
            }
            if n > atomicLoad(&stats[0]) {
                atomicMax(&stats[0], n);
            }
        }
    }
    if !escaped {
        return vec4<f32>(0.0, 0.0, 0.0, 1.0);
    }
    let r2 = dot(z, z);
    let smooth_n = f32(n) - log2(log2(r2)) + 4.0;
    // Colour changes with the logarithm of the count, so bands stay wide near the
    // whole set, plus one loop each 512 counts, so a deep view, where every count
    // is large and close to the others, still shows several bands.
    let base = palette(0.45 * log2(smooth_n + 1.0) + smooth_n / 512.0, u32(u.theme));
    // Exterior distance estimate in pixels: 0.5·|z|·ln|z|² / |dz/dc · step|.
    // Filaments darken within about a pixel of the set and fade out smoothly;
    // a derivative past f32's range (or NaN) is a pixel at the edge.
    let der_length = length(der);
    var edge = 0.0;
    if der_length < 3.0e38 {
        let distance = 0.5 * sqrt(r2) * log(r2) / der_length;
        edge = clamp(pow(distance * 0.5, 0.45), 0.0, 1.0);
    }
    return vec4<f32>(base * edge, 1.0);
}
