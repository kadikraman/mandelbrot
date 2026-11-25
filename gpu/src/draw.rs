//! The GPU side: pipelines, the two pictures, the orbit buffer, and the passes
//! that draw the set and put it on screen.

use exact_gpu::wgpu;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{mpsc, Arc};

use crate::shaders::{composite, mandel};

/// Float, so jittered samples sum without clipping; the composite divides.
const TEXTURE_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;

pub struct Gpu {
    pub format: wgpu::TextureFormat,
    pub generation: u32,
    /// The set pass: `replace` writes a sample, `add` sums one into what is there.
    replace: wgpu::RenderPipeline,
    add: wgpu::RenderPipeline,
    set_layout: wgpu::BindGroupLayout,
    set_uniforms: exact_gpu::FrameUniform,
    /// The reference orbit's storage buffer, its capacity in points, and the
    /// set pass's bind groups over it.
    orbit: Option<(wgpu::Buffer, usize, Vec<wgpu::BindGroup>)>,
    /// The BLA table's storage buffer and its capacity in `vec4`s.
    bla: (wgpu::Buffer, usize),
    composite: wgpu::RenderPipeline,
    composite_layout: wgpu::BindGroupLayout,
    composite_uniforms: exact_gpu::FrameUniform,
    sampler: wgpu::Sampler,
    textures: Option<Textures>,
    /// The preview's counts, a copy of them to map, and the map in flight with
    /// the iteration limit the counts were taken at.
    stats: wgpu::Buffer,
    staging: wgpu::Buffer,
    copied: Option<u32>,
    pending: Option<(u32, mpsc::Receiver<Result<(), wgpu::BufferAsyncError>>)>,
    /// Submits with set passes in them that the GPU has not finished.
    queue: wgpu::Queue,
    in_flight: Arc<AtomicU32>,
    recorded: bool,
}

/// What the preview counted, at a limit: its highest escape count, and pixels left at the limit that are not inside the set.
#[derive(Clone, Copy, Debug)]
pub struct Stats {
    pub max_escape: u32,
    pub undecided: u32,
    pub escaped: u32,
    /// Escaped in the top quarter of the limit.
    pub near_limit: u32,
    pub limit: u32,
}

/// Two previews, the one shown and the one being drawn, and the sharp picture,
/// all at full device resolution, and the composite's bind groups over them for
/// either preview shown; remade when the canvas's size changes.
struct Textures {
    size: (u32, u32),
    previews: [wgpu::TextureView; 2],
    sharp: wgpu::TextureView,
    groups: [Vec<wgpu::BindGroup>; 2],
    shown: usize,
}

/// Which picture a set pass draws into: the preview not shown, or the sharp picture.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Picture {
    Preview,
    Sharp,
}

/// One set pass: the picture's pixel size, the plane width of one of its
/// pixels, the view's centre minus the reference's point, and the rows to draw.
pub struct SetPass {
    pub width: u32,
    pub height: u32,
    pub step: f64,
    pub dc: (f64, f64),
    pub max_iter: u32,
    pub orbit_len: usize,
    /// BLA levels and the orbit points the table covers; 0 levels skips nothing.
    pub bla: (u32, usize),
    pub jitter: (f32, f32),
    pub rows: (u32, u32),
    pub add: bool,
    /// Count into the stats buffer, cleared at the first row and copied out at
    /// the last; only when no count is in flight.
    pub count: bool,
}

/// What the composite shows: the preview's place in the current view, how far
/// the sharp picture has got, and the hover box.
pub struct Show {
    /// Device pixel p of the current view lies at k·p + b in the preview's full-resolution pixels.
    pub k: f64,
    pub b: (f64, f64),
    pub preview_scale: f32,
    /// Rows above `split` hold `samples` + 1 sharp samples, rows below `samples`; None shows the preview only.
    pub sharp: Option<(u32, u32)>,
    pub hover_box: Option<[f32; 4]>,
    pub zoom_out: bool,
    pub dpr: f32,
}

impl Gpu {
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue, format: wgpu::TextureFormat, generation: u32) -> Gpu {
        let set_layout = device.create_bind_group_layout(&mandel::GROUP_0);
        let set_uniforms = exact_gpu::FrameUniform::new(device, mandel::Uniforms::SIZE, "mandel uniforms");
        let sum = wgpu::BlendComponent { src_factor: wgpu::BlendFactor::One, dst_factor: wgpu::BlendFactor::One, operation: wgpu::BlendOperation::Add };
        let set = |label, blend| {
            pipeline(device, label, mandel::module(), &set_layout, mandel::entry::VS, mandel::entry::FS, TEXTURE_FORMAT, blend)
        };
        let replace = set("mandel set", None);
        let add = set("mandel set sum", Some(wgpu::BlendState { color: sum, alpha: sum }));
        let composite_layout = device.create_bind_group_layout(&composite::GROUP_0);
        let composite = pipeline(
            device,
            "mandel composite",
            composite::module(),
            &composite_layout,
            composite::entry::VS,
            composite::entry::FS,
            format,
            None,
        );
        let composite_uniforms = exact_gpu::FrameUniform::new(device, composite::Composite::SIZE, "mandel composite uniforms");
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("mandel preview"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        Gpu {
            format,
            generation,
            replace,
            add,
            set_layout,
            set_uniforms,
            orbit: None,
            composite,
            composite_layout,
            composite_uniforms,
            sampler,
            textures: None,
            stats: device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("mandel stats"),
                size: 16,
                usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }),
            staging: device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("mandel stats staging"),
                size: 16,
                usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }),
            copied: None,
            pending: None,
            bla: (empty_bla(device, 64), 64),
            queue: queue.clone(),
            in_flight: Arc::new(AtomicU32::new(0)),
            recorded: false,
        }
    }

    /// Puts a BLA table on the GPU, growing its buffer when needed.
    pub fn upload_bla(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, data: &[[f32; 4]]) {
        if data.len() > self.bla.1 {
            let capacity = data.len().next_power_of_two();
            self.bla = (empty_bla(device, capacity), capacity);
            self.orbit = None;
        }
        let bytes: Vec<u8> = data.iter().flatten().flat_map(|v| v.to_le_bytes()).collect();
        queue.write_buffer(&self.bla.0, 0, &bytes);
    }

    /// Whether the set pass's bind groups need the orbit uploaded again
    /// (a new BLA buffer drops them).
    pub fn has_orbit(&self) -> bool {
        self.orbit.is_some()
    }

    /// Whether the stats buffer is free for a new count.
    pub fn can_count(&self) -> bool {
        self.copied.is_none() && self.pending.is_none()
    }

    /// Submits with set passes the GPU has not finished. The browser reports
    /// a finish about a frame late, so one still in flight is normal.
    pub fn in_flight(&self, device: &wgpu::Device) -> u32 {
        let _ = device.poll(wgpu::PollType::Poll);
        self.in_flight.load(Ordering::Acquire)
    }

    /// Shows the preview just drawn; the other one is drawn next.
    pub fn swap_preview(&mut self) {
        if let Some(t) = self.textures.as_mut() {
            t.shown = 1 - t.shown;
        }
    }

    /// Notes when the GPU finishes the set passes, and maps the copy the last
    /// frame recorded; call after its submit.
    pub fn submitted(&mut self) {
        if std::mem::take(&mut self.recorded) {
            self.in_flight.fetch_add(1, Ordering::AcqRel);
            let in_flight = self.in_flight.clone();
            self.queue.on_submitted_work_done(move || {
                in_flight.fetch_sub(1, Ordering::AcqRel);
            });
        }
        if let Some(limit) = self.copied.take() {
            let (tx, rx) = mpsc::channel();
            self.staging.slice(..).map_async(wgpu::MapMode::Read, move |result| {
                let _ = tx.send(result);
            });
            self.pending = Some((limit, rx));
        }
    }

    /// The counts, once their map has completed.
    pub fn take_stats(&mut self, device: &wgpu::Device) -> Option<Stats> {
        let (limit, rx) = self.pending.as_ref()?;
        let _ = device.poll(wgpu::PollType::Poll);
        let result = rx.try_recv().ok()?;
        let limit = *limit;
        self.pending = None;
        result.ok()?;
        let view = self.staging.slice(..).get_mapped_range().ok()?;
        let word = |i: usize| u32::from_le_bytes(view[i * 4..i * 4 + 4].try_into().unwrap());
        let stats = Stats { max_escape: word(0), undecided: word(1), escaped: word(2), near_limit: word(3), limit };
        drop(view);
        self.staging.unmap();
        Some(stats)
    }

    /// Whether a count is waiting to be read.
    pub fn counting(&self) -> bool {
        self.copied.is_some() || self.pending.is_some()
    }

    /// Makes both pictures `width` × `height`; true when they were remade, and so hold nothing.
    pub fn ensure_textures(&mut self, device: &wgpu::Device, width: u32, height: u32) -> bool {
        if self.textures.as_ref().is_some_and(|t| t.size == (width, height)) {
            return false;
        }
        let make = |label| {
            device
                .create_texture(&wgpu::TextureDescriptor {
                    label: Some(label),
                    size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: TEXTURE_FORMAT,
                    usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
                    view_formats: &[],
                })
                .create_view(&Default::default())
        };
        let previews = [make("mandel preview"), make("mandel preview")];
        let sharp = make("mandel sharp");
        let groups = |preview: &wgpu::TextureView| -> Vec<wgpu::BindGroup> {
            (0..self.composite_uniforms.slots())
            .map(|slot| {
                device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("mandel composite"),
                    layout: &self.composite_layout,
                    entries: &[
                        wgpu::BindGroupEntry { binding: composite::C.binding, resource: self.composite_uniforms.buffer(slot).as_entire_binding() },
                        wgpu::BindGroupEntry { binding: composite::PREVIEW_TEXTURE.binding, resource: wgpu::BindingResource::TextureView(preview) },
                        wgpu::BindGroupEntry { binding: composite::SHARP_TEXTURE.binding, resource: wgpu::BindingResource::TextureView(&sharp) },
                        wgpu::BindGroupEntry { binding: composite::PREVIEW_SAMPLER.binding, resource: wgpu::BindingResource::Sampler(&self.sampler) },
                    ],
                })
            })
            .collect()
        };
        let groups = [groups(&previews[0]), groups(&previews[1])];
        self.textures = Some(Textures { size: (width, height), previews, sharp, groups, shown: 0 });
        true
    }

    /// Puts the orbit's points from `from` on the GPU, in a buffer that grows by
    /// doubling; a new buffer takes every point.
    pub fn upload_orbit(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, points: &[[f32; 2]], from: usize) {
        let fits = self.orbit.as_ref().is_some_and(|o| o.1 >= points.len());
        let from = if fits { from.min(points.len()) } else { 0 };
        if !fits {
            let capacity = points.len().next_power_of_two().max(4096);
            let buffer = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("mandel orbit"),
                size: (capacity * 8) as u64,
                usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            let groups = (0..self.set_uniforms.slots())
                .map(|slot| {
                    device.create_bind_group(&wgpu::BindGroupDescriptor {
                        label: Some("mandel set"),
                        layout: &self.set_layout,
                        entries: &[
                            wgpu::BindGroupEntry { binding: mandel::U.binding, resource: self.set_uniforms.buffer(slot).as_entire_binding() },
                            wgpu::BindGroupEntry { binding: mandel::ORBIT.binding, resource: buffer.as_entire_binding() },
                            wgpu::BindGroupEntry { binding: mandel::STATS.binding, resource: self.stats.as_entire_binding() },
                            wgpu::BindGroupEntry { binding: mandel::BLA.binding, resource: self.bla.0.as_entire_binding() },
                        ],
                    })
                })
                .collect();
            self.orbit = Some((buffer, capacity, groups));
        }
        if from >= points.len() {
            return;
        }
        let bytes: Vec<u8> = points[from..].iter().flat_map(|[x, y]| x.to_le_bytes().into_iter().chain(y.to_le_bytes())).collect();
        queue.write_buffer(&self.orbit.as_ref().unwrap().0, (from * 8) as u64, &bytes);
    }

    /// One set pass into `picture`, limited to its rows.
    pub fn draw_set(&mut self, queue: &wgpu::Queue, encoder: &mut wgpu::CommandEncoder, picture: Picture, pass: &SetPass) {
        let uniforms = mandel::Uniforms {
            dcx: pass.dc.0 as f32,
            dcy: pass.dc.1 as f32,
            step: pass.step as f32,
            width: pass.width as f32,
            height: pass.height as f32,
            max_iter: pass.max_iter as f32,
            jitter_x: pass.jitter.0,
            jitter_y: pass.jitter.1,
            orbit_len: pass.orbit_len as f32,
            count_stats: if pass.count { 1.0 } else { 0.0 },
            bla_levels: pass.bla.0 as f32,
            bla_len: pass.bla.1 as f32,
        };
        let slot = self.set_uniforms.write(queue, &uniforms.bytes());
        if pass.count && pass.rows.0 == 0 {
            queue.write_buffer(&self.stats, 0, &[0; 16]);
        }
        self.recorded = true;
        let textures = self.textures.as_ref().unwrap();
        let view = match picture {
            Picture::Preview => &textures.previews[1 - textures.shown],
            Picture::Sharp => &textures.sharp,
        };
        let mut rpass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("mandel set"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view,
                resolve_target: None,
                depth_slice: None,
                ops: wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        rpass.set_viewport(0.0, 0.0, pass.width as f32, pass.height as f32, 0.0, 1.0);
        rpass.set_scissor_rect(0, pass.rows.0, pass.width, pass.rows.1 - pass.rows.0);
        rpass.set_pipeline(if pass.add { &self.add } else { &self.replace });
        rpass.set_bind_group(0, &self.orbit.as_ref().unwrap().2[slot], &[]);
        rpass.draw(0..3, 0..1);
        drop(rpass);
        if pass.count && pass.rows.1 == pass.height {
            encoder.copy_buffer_to_buffer(&self.stats, 0, &self.staging, 0, 16);
            self.copied = Some(pass.max_iter);
        }
    }

    pub fn draw_composite(&mut self, queue: &wgpu::Queue, encoder: &mut wgpu::CommandEncoder, target: &wgpu::TextureView, show: &Show) {
        let textures = self.textures.as_ref().unwrap();
        let (width, height) = textures.size;
        let [x0, y0, x1, y1] = show.hover_box.unwrap_or([0.0; 4]);
        let (split, samples) = show.sharp.unwrap_or((0, 0));
        let dpr = show.dpr;
        let uniforms = composite::Composite {
            k: show.k as f32,
            bx: show.b.0 as f32,
            by: show.b.1 as f32,
            preview_w: (width as f32 * show.preview_scale).round(),
            preview_h: (height as f32 * show.preview_scale).round(),
            preview_scale: show.preview_scale,
            tex_w: width as f32,
            tex_h: height as f32,
            show_sharp: if show.sharp.is_some() { 1.0 } else { 0.0 },
            split: split as f32,
            samples_above: samples as f32 + 1.0,
            samples_below: samples as f32,
            box_x0: x0 * dpr,
            box_y0: y0 * dpr,
            box_x1: x1 * dpr,
            box_y1: y1 * dpr,
            show_box: if show.hover_box.is_some() { 1.0 } else { 0.0 },
            zoom_out: if show.zoom_out { 1.0 } else { 0.0 },
            line: 1.5 * dpr,
        };
        let slot = self.composite_uniforms.write(queue, &uniforms.bytes());
        let mut rpass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("mandel composite"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target,
                resolve_target: None,
                depth_slice: None,
                ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::BLACK), store: wgpu::StoreOp::Store },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        rpass.set_pipeline(&self.composite);
        rpass.set_bind_group(0, &textures.groups[textures.shown][slot], &[]);
        rpass.draw(0..3, 0..1);
    }
}

fn empty_bla(device: &wgpu::Device, capacity: usize) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("mandel bla"),
        size: (capacity * 16) as u64,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

#[allow(clippy::too_many_arguments)]
fn pipeline(
    device: &wgpu::Device,
    label: &str,
    module: wgpu::ShaderModuleDescriptor<'static>,
    layout: &wgpu::BindGroupLayout,
    vs: &str,
    fs: &str,
    format: wgpu::TextureFormat,
    blend: Option<wgpu::BlendState>,
) -> wgpu::RenderPipeline {
    let shader = device.create_shader_module(module);
    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some(label),
        bind_group_layouts: &[Some(layout)],
        immediate_size: 0,
    });
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some(label),
        layout: Some(&pipeline_layout),
        vertex: wgpu::VertexState { module: &shader, entry_point: Some(vs), buffers: &[], compilation_options: Default::default() },
        fragment: Some(wgpu::FragmentState {
            module: &shader,
            entry_point: Some(fs),
            targets: &[Some(wgpu::ColorTargetState { format, blend, write_mask: wgpu::ColorWrites::ALL })],
            compilation_options: Default::default(),
        }),
        primitive: Default::default(),
        depth_stencil: None,
        multisample: Default::default(),
        multiview_mask: None,
        cache: None,
    })
}
