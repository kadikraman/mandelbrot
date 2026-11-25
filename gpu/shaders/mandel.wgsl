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

// Inigo Quilez's cosine palette: a + b·cos(2π(c·t + d)).
fn palette(t: f32) -> vec3<f32> {
    let d = vec3<f32>(0.0, 0.10, 0.20);
    return 0.5 + 0.5 * cos(6.2831853 * (t + d));
}

fn mul(a: vec2<f32>, b: vec2<f32>) -> vec2<f32> {
    return vec2<f32>(a.x * b.x - a.y * b.y, a.x * b.y + a.y * b.x);
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
    // dz/dz₀, only while counting: it shrinks towards zero inside the set and
    // grows outside, so a pixel left at the limit can be told apart.
    let counting = u.count_stats > 0.5;
    var dzz = vec2<f32>(1.0, 0.0);
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
        // The longest skip that starts here and is valid for this |dz|.
        var skipped = false;
        if m > 0u && m + 1u < bla_len {
            let dz_length = length(dz);
            var k = levels;
            loop {
                if k == 0u {
                    break;
                }
                k = k - 1u;
                if ((m - 1u) & ((1u << k) - 1u)) != 0u {
                    continue;
                }
                let header = bla[k];
                let j = (m - 1u) >> k;
                if j >= u32(header.y) {
                    continue;
                }
                let at = 32u + 2u * (u32(header.x) + j);
                let ab = bla[at];
                let rl = bla[at + 1u];
                let l = u32(rl.y);
                if dz_length < rl.x && n + l <= limit && m + l < len {
                    der = mul(ab.xy, der) + ab.zw * u.step;
                    if counting {
                        dzz = mul(ab.xy, dzz);
                    }
                    dz = mul(ab.xy, dz) + mul(ab.zw, dc);
                    m = m + l;
                    n = n + l;
                    skipped = true;
                    break;
                }
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
            continue;
        }
        let zm = orbit[m];
        der = 2.0 * mul(zm + dz, der) + vec2<f32>(u.step, 0.0);
        if counting {
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
    }
    if counting {
        if !escaped {
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
    // Colour changes with the logarithm of the count, so bands stay wide at any depth.
    let base = palette(0.45 * log2(smooth_n + 1.0));
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
