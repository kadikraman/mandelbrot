// Puts the set on screen and draws the hover box. Two pictures feed it: the
// last preview finished, drawn at a low resolution and moved to the current
// view, and the sharp picture of the current view, which fills in a
// band of rows a frame and sums jittered samples. Rows above `split` hold one
// sample more than rows below; a row with none shows the preview.
struct Composite {
    k: f32,
    bx: f32,
    by: f32,
    preview_w: f32,
    preview_h: f32,
    preview_scale: f32,
    tex_w: f32,
    tex_h: f32,
    show_sharp: f32,
    split: f32,
    samples_above: f32,
    samples_below: f32,
    box_x0: f32,
    box_y0: f32,
    box_x1: f32,
    box_y1: f32,
    show_box: f32,
    zoom_out: f32,
    line: f32,
}

@group(0) @binding(0) var<uniform> c: Composite;
@group(0) @binding(1) var preview_texture: texture_2d<f32>;
@group(0) @binding(2) var sharp_texture: texture_2d<f32>;
@group(0) @binding(3) var preview_sampler: sampler;

@vertex
fn vs(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
    return vec4<f32>(f32(i32(i & 1u) * 4 - 1), f32(i32(i >> 1u) * 4 - 1), 0.0, 1.0);
}

fn segment_distance(p: vec2<f32>, a: vec2<f32>, b: vec2<f32>) -> f32 {
    let ab = b - a;
    let t = clamp(dot(p - a, ab) / dot(ab, ab), 0.0, 1.0);
    return length(p - (a + t * ab));
}

@fragment
fn fs(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let p = position.xy;
    var colour = vec3<f32>(0.0, 0.0, 0.0);
    let samples = select(c.samples_below, c.samples_above, p.y < c.split);
    if c.show_sharp > 0.5 && samples > 0.0 {
        colour = textureLoad(sharp_texture, vec2<i32>(p), 0).rgb / samples;
    } else {
        // The pixel in the preview's own view at full resolution, then at the resolution it was drawn at.
        let drawn = vec2<f32>(c.k * p.x + c.bx, c.k * p.y + c.by) * c.preview_scale;
        if drawn.x >= 0.0 && drawn.y >= 0.0 && drawn.x <= c.preview_w && drawn.y <= c.preview_h {
            colour = textureSampleLevel(preview_texture, preview_sampler, drawn / vec2<f32>(c.tex_w, c.tex_h), 0.0).rgb;
        }
    }
    if c.show_box > 0.5 {
        let a = vec2<f32>(c.box_x0, c.box_y0);
        let b = vec2<f32>(c.box_x1, c.box_y1);
        var d = min(
            min(segment_distance(p, a, vec2<f32>(b.x, a.y)), segment_distance(p, vec2<f32>(b.x, a.y), b)),
            min(segment_distance(p, b, vec2<f32>(a.x, b.y)), segment_distance(p, vec2<f32>(a.x, b.y), a))
        );
        if c.zoom_out > 0.5 {
            // Lines from the screen's corners to the box: the view shrinking into it.
            let w = c.tex_w;
            let h = c.tex_h;
            d = min(d, min(segment_distance(p, vec2<f32>(0.0, 0.0), a), segment_distance(p, vec2<f32>(w, 0.0), vec2<f32>(b.x, a.y))));
            d = min(d, min(segment_distance(p, vec2<f32>(w, h), b), segment_distance(p, vec2<f32>(0.0, h), vec2<f32>(a.x, b.y))));
        } else if p.x < a.x || p.x > b.x || p.y < a.y || p.y > b.y {
            // Outside the box is dimmed.
            colour = colour * 0.55;
        }
        let edge = 1.0 - smoothstep(c.line * 0.5, c.line * 0.5 + 1.0, d);
        let halo = 1.0 - smoothstep(c.line * 1.5, c.line * 1.5 + 1.0, d);
        colour = mix(colour, vec3<f32>(0.0, 0.0, 0.0), halo * 0.5);
        colour = mix(colour, vec3<f32>(1.0, 1.0, 1.0), edge);
    }
    return vec4<f32>(colour, 1.0);
}
