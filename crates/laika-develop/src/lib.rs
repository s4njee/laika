//! laika-develop: wgpu develop pipeline (Phase 3).
//!
//! The render thread owns the wgpu device. Preview jobs coalesce to the
//! latest; export renders full resolution and replies on a oneshot. Same
//! WGSL for preview and export.

use std::collections::VecDeque;
use std::sync::mpsc;
use std::thread;

use bytemuck::{Pod, Zeroable};

use laika_raw::decode::LinearImage;

pub use laika_core::edit::PARAM_COUNT;

/// U08: render-time geometry (pre-constrained by the app): normalized
/// crop rect, straighten angle in radians, mirrors.
/// V15: `rotation` quarter-turns CW — the rect lives in display
/// (post-rotation) space; output dims swap on odd values.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CropRender {
    pub rect: [f32; 4],
    pub angle_rad: f32,
    pub flip_h: bool,
    pub flip_v: bool,
    pub rotation: u8,
    /// Crop-tool editing view: `rect` is the full frame and pixels that
    /// fall outside the (straightened) source paint dark instead of
    /// smearing edge texels, so the whole photo is visible under the box.
    pub frame_view: bool,
    /// Upright/Transform perspective warp: row-major 3×3 `G` mapping
    /// corrected centered display coords `(x, y, 1)` to homogeneous source
    /// centered coords (see [`warp_display_uv`]). Applied after straighten,
    /// before the quarter-turn is undone. [`WARP_IDENTITY`] = no warp.
    pub warp: [f32; 9],
}

/// Identity perspective warp (row-major 3×3).
pub const WARP_IDENTITY: [f32; 9] = [1., 0., 0., 0., 1., 0., 0., 0., 1.];

/// Perspective warp of a display-space uv (display = source after the
/// quarter-turn, before flips; `aspect` = display w / h). Centered coords
/// `x = (u - 0.5) * aspect`, `y = v - 0.5` (y down) go through `warp` to
/// `(X, Y, W)`; the source point `(X/W, Y/W)` maps back to display uv.
/// `None` when `|W| < 1e-6` (the shader treats that as outside). CPU
/// mirror of the `geom_uv` warp step in `develop.wgsl`.
pub fn warp_display_uv(warp: &[f32; 9], u: f32, v: f32, aspect: f32) -> Option<(f32, f32)> {
    let g = warp;
    let x = (u - 0.5) * aspect;
    let y = v - 0.5;
    let hx = g[0] * x + g[1] * y + g[2];
    let hy = g[3] * x + g[4] * y + g[5];
    let hw = g[6] * x + g[7] * y + g[8];
    if hw.abs() < 1e-6 {
        return None;
    }
    Some((hx / hw / aspect + 0.5, hy / hw + 0.5))
}

impl Default for CropRender {
    fn default() -> Self {
        Self {
            rect: [0., 0., 1., 1.],
            angle_rad: 0.,
            flip_h: false,
            flip_v: false,
            rotation: 0,
            frame_view: false,
            warp: WARP_IDENTITY,
        }
    }
}

impl CropRender {
    /// Identity check: default geometry samples uv unchanged.
    pub fn is_default(&self) -> bool {
        self.rect == [0., 0., 1., 1.]
            && self.angle_rad == 0.
            && !self.flip_h
            && !self.flip_v
            && self.rotation % 4 == 0
            && !self.warp_active()
    }

    /// True when `warp` differs from [`WARP_IDENTITY`] (beyond 1e-7).
    pub fn warp_active(&self) -> bool {
        self.warp
            .iter()
            .zip(WARP_IDENTITY.iter())
            .any(|(a, b)| (a - b).abs() > 1e-7)
    }
}

/// Output pixel dims for a crop rect on a `sw x sh` source (min 1px).
/// V15: odd rotations swap axes (rect is in display space).
pub fn crop_target(sw: u32, sh: u32, rect: [f32; 4], rotation: u8) -> (u32, u32) {
    let (dw, dh) = if rotation % 4 % 2 == 1 {
        (sh, sw)
    } else {
        (sw, sh)
    };
    (
        ((dw as f32 * rect[2]).round() as u32).max(1),
        ((dh as f32 * rect[3]).round() as u32).max(1),
    )
}

/// Live Develop frames only need to cover the display. Keeping readback at
/// this edge makes slider feedback responsive; native zoom and export retain
/// their separate full-resolution paths.
pub const PREVIEW_LONG_EDGE: u32 = 1280;
pub const PREVIEW_INTERACTIVE_EDGE: u32 = 720;

fn fit_long_edge((w, h): (u32, u32), edge: u32) -> (u32, u32) {
    let long = w.max(h);
    if long <= edge || edge == 0 {
        return (w, h);
    }
    if w >= h {
        (edge, ((h as u64 * edge as u64) / w as u64).max(1) as u32)
    } else {
        (((w as u64 * edge as u64) / h as u64).max(1) as u32, edge)
    }
}

pub struct Job {
    pub params: [f32; PARAM_COUNT],
    pub split: f32,
    pub geom: CropRender,
    /// Output edge for this live frame. Slider drags use a display-sized
    /// proxy; release submits the settled-quality frame.
    pub preview_long_edge: u32,
    /// U09: photo the job was submitted for — frames for a departed
    /// photo are dropped, never shown as the new result.
    pub photo_id: Option<i64>,
    pub seq: u64,
}

pub struct Rendered {
    /// Pixel bytes, row-major, 4 per pixel. Export renders are RGBA (ready
    /// for `image` encoders); live preview frames are BGRA — the order GPUI
    /// `RenderImage` uploads — so the readback needs no CPU swizzle.
    pub rgba: Vec<u8>,
    /// True when `rgba` holds BGRA bytes (preview frames).
    pub bgra: bool,
    pub width: u32,
    pub height: u32,
    pub photo_id: Option<i64>,
    pub seq: u64,
    /// Uniform write, encode, submit, GPU pass + copy into mapped buffer.
    pub gpu_ms: f32,
    /// CPU row copy (+ BGRA→RGBA swizzle for exports) + histogram.
    pub copy_ms: f32,
    pub histogram: [f32; 48],
    /// U09: clipped-pixel fractions (pure black, pure white) of the 8-bit
    /// output, computed on the render thread alongside the histogram.
    pub clip: (f32, f32),
}

/// Byte order requested from a readback.
#[derive(Clone, Copy, PartialEq, Eq)]
enum PixelOrder {
    Rgba,
    Bgra,
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub struct Uniforms {
    /// a, b, c param rows; d = [split, angle_rad, target_w, target_h];
    /// e = [src_w, src_h, flip_h, flip_v]; f = crop rect xywh;
    /// g = curve 20/40/60/80; h..m = HSL hue/sat/lum halves;
    /// n = detail (sharpen, radius, lum NR, color NR);
    /// o = optics (distortion, CA, rotation, frame view);
    /// p = effects (dehaze, vignette, grain, display-referred source 0/1);
    /// cg0..cg3 = color grading params 49..62 in order (+2 pad).
    pub rows: [[f32; 4]; 20],
    pub wb: [f32; 4],
    pub cam: [[f32; 4]; 3],
    /// Perspective warp rows (WGSL `warp0..warp2`): G rows in `.xyz`;
    /// `warp[0][3]` = 1 when the warp is active, else 0. Other `.w` pad.
    pub warp: [[f32; 4]; 3],
}

/// Pack develop params + image metadata into shader uniforms. Pure and unit
/// tested; the WGSL struct field order must match `Uniforms`.
pub fn pack(
    params: &[f32; PARAM_COUNT],
    split: f32,
    target: (u32, u32),
    src: &LinearImage,
    geom: &CropRender,
) -> Uniforms {
    let p = params;
    let q = |i: usize| p[i];
    Uniforms {
        rows: [
            [p[0], p[1], p[2], p[3]],
            [p[4], p[5], p[6], p[7]],
            [p[8], p[9], p[10], p[11]],
            [split, geom.angle_rad, target.0 as f32, target.1 as f32],
            [
                src.width as f32,
                src.height as f32,
                if geom.flip_h { 1. } else { 0. },
                if geom.flip_v { 1. } else { 0. },
            ],
            [geom.rect[0], geom.rect[1], geom.rect[2], geom.rect[3]],
            [q(12), q(13), q(14), q(15)],
            [q(16), q(17), q(18), q(19)],
            [q(20), q(21), q(22), q(23)],
            [q(24), q(25), q(26), q(27)],
            [q(28), q(29), q(30), q(31)],
            [q(32), q(33), q(34), q(35)],
            [q(36), q(37), q(38), q(39)],
            [q(40), q(41), q(42), q(43)],
            [
                q(44),
                q(45),
                geom.rotation as f32,
                if geom.frame_view { 1. } else { 0. },
            ],
            [
                q(46),
                q(47),
                q(48),
                if src.display_referred { 1. } else { 0. },
            ],
            [q(49), q(50), q(51), q(52)],
            [q(53), q(54), q(55), q(56)],
            [q(57), q(58), q(59), q(60)],
            [q(61), q(62), 0., 0.],
        ],
        wb: [src.wb_as_shot[0], src.wb_as_shot[1], src.wb_as_shot[2], 0.],
        cam: [
            [
                src.cam_to_xyz[0][0],
                src.cam_to_xyz[0][1],
                src.cam_to_xyz[0][2],
                0.,
            ],
            [
                src.cam_to_xyz[1][0],
                src.cam_to_xyz[1][1],
                src.cam_to_xyz[1][2],
                0.,
            ],
            [
                src.cam_to_xyz[2][0],
                src.cam_to_xyz[2][1],
                src.cam_to_xyz[2][2],
                0.,
            ],
        ],
        warp: {
            let g = &geom.warp;
            [
                [g[0], g[1], g[2], if geom.warp_active() { 1. } else { 0. }],
                [g[3], g[4], g[5], 0.],
                [g[6], g[7], g[8], 0.],
            ]
        },
    }
}

enum Msg {
    Source {
        image: LinearImage,
        warm: Option<WarmUp>,
        ack: Option<mpsc::Sender<()>>,
    },
    Preview(Job),
    Export {
        image: LinearImage,
        params: [f32; PARAM_COUNT],
        /// U07: 100% detail renders the same before/after composition as
        /// the preview so the zoomed view stays spatially aligned with it.
        split: f32,
        /// U08: render-time geometry (output dims follow the crop rect).
        geom: CropRender,
        reply: mpsc::Sender<Result<Rendered, String>>,
    },
}

/// Parameters for the throwaway interactive render performed when a photo is
/// installed. This pays the shader-compilation, target-allocation and first
/// readback costs before the user touches a Develop control.
struct WarmUp {
    params: [f32; PARAM_COUNT],
    split: f32,
    geom: CropRender,
}

#[derive(Clone)]
pub struct Renderer {
    tx: mpsc::Sender<Msg>,
}

impl Renderer {
    /// Starts the render thread. Returns once the adapter is up.
    pub fn spawn(on_frame: impl Fn(Rendered) + Send + 'static) -> Result<(Self, String), String> {
        Self::spawn_with(on_frame, false)
    }

    /// V31: start on the low-power GPU when asked (systems with one GPU
    /// get the same adapter either way).
    pub fn spawn_with(
        on_frame: impl Fn(Rendered) + Send + 'static,
        low_power: bool,
    ) -> Result<(Self, String), String> {
        let (tx, rx) = mpsc::channel::<Msg>();
        let (ready_tx, ready_rx) = mpsc::channel();
        thread::Builder::new()
            .name("laika-develop".into())
            .spawn(move || match Gpu::new(low_power) {
                Ok(gpu) => {
                    ready_tx.send(Ok(gpu.adapter_info())).ok();
                    gpu.run(rx, on_frame);
                }
                Err(e) => {
                    ready_tx.send(Err(e)).ok();
                }
            })
            .map_err(|e| e.to_string())?;
        // A dropped sender means the thread panicked during init (e.g. a
        // shader validation error) — report that, not a generic channel error.
        let info = ready_rx
            .recv()
            .map_err(|_| "render thread panicked during GPU init".to_string())??;
        Ok((Self { tx }, info))
    }

    /// Upload a new editing source; blocks until the GPU thread applied it so
    /// a subsequent `submit` renders from the right image.
    pub fn set_source(&self, image: LinearImage) {
        self.set_source_inner(image, None);
    }

    /// Upload a new editing source and prime the lower-resolution interactive
    /// path. The warm frame is discarded; the next slider render reuses its
    /// texture, readback buffer and compiled pipeline.
    ///
    /// Does not block: the channel is FIFO, so any later `submit` renders
    /// from this source, and the UI thread is not stalled on the upload.
    pub fn set_source_warm(
        &self,
        image: LinearImage,
        params: [f32; PARAM_COUNT],
        split: f32,
        geom: CropRender,
    ) {
        self.tx
            .send(Msg::Source {
                image,
                warm: Some(WarmUp {
                    params,
                    split,
                    geom,
                }),
                ack: None,
            })
            .ok();
    }

    fn set_source_inner(&self, image: LinearImage, warm: Option<WarmUp>) {
        let (ack_tx, ack_rx) = mpsc::channel();
        self.tx
            .send(Msg::Source {
                image,
                warm,
                ack: Some(ack_tx),
            })
            .ok();
        ack_rx.recv().ok();
    }

    /// Latest wins: the thread drains the queue before each render.
    pub fn submit(&self, job: Job) {
        self.tx.send(Msg::Preview(job)).ok();
    }

    /// Full-resolution render of `image`, blocking. `split` composes the
    /// same before/after divider as the preview (0 = after only).
    pub fn render_export(
        &self,
        image: LinearImage,
        params: [f32; PARAM_COUNT],
        split: f32,
        geom: CropRender,
    ) -> Result<Rendered, String> {
        let (reply_tx, reply_rx) = mpsc::channel();
        self.tx
            .send(Msg::Export {
                image,
                params,
                split,
                geom,
                reply: reply_tx,
            })
            .ok();
        reply_rx.recv().map_err(|e| e.to_string())?
    }
}

struct Targets {
    view: wgpu::TextureView,
    #[allow(dead_code)]
    texture: wgpu::Texture,
    readback: wgpu::Buffer,
    width: u32,
    height: u32,
    padded_row: u32,
}

/// A linear image uploaded to the GPU with its bind group. The CPU copy is
/// kept so later requests can be matched against the uploaded content.
struct GpuSource {
    image: LinearImage,
    bind: wgpu::BindGroup,
    #[allow(dead_code)]
    texture: wgpu::Texture,
}

/// Content identity for uploaded sources. Metadata rejects different
/// photos cheaply; the pixel compare (a memcmp) is far cheaper than
/// expanding and re-uploading the texture.
fn same_image(a: &LinearImage, b: &LinearImage) -> bool {
    a.width == b.width
        && a.height == b.height
        && a.wb_as_shot == b.wb_as_shot
        && a.cam_to_xyz == b.cam_to_xyz
        && a.display_referred == b.display_referred
        // (`camera` is not compared: editing-cache reads leave it empty.)
        && a.rgb_f16 == b.rgb_f16
}

struct ExportReq {
    image: LinearImage,
    params: [f32; PARAM_COUNT],
    split: f32,
    geom: CropRender,
    reply: mpsc::Sender<Result<Rendered, String>>,
}

struct Gpu {
    device: wgpu::Device,
    queue: wgpu::Queue,
    pipeline: wgpu::RenderPipeline,
    sampler: wgpu::Sampler,
    uniforms: wgpu::Buffer,
    layout: wgpu::BindGroupLayout,
    adapter_info: String,
}

fn fullscreen_pass(
    encoder: &mut wgpu::CommandEncoder,
    view: &wgpu::TextureView,
    pipeline: &wgpu::RenderPipeline,
    bind_group: &wgpu::BindGroup,
) {
    let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: None,
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view,
            depth_slice: None,
            resolve_target: None,
            ops: wgpu::Operations {
                load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                store: wgpu::StoreOp::Store,
            },
        })],
        depth_stencil_attachment: None,
        timestamp_writes: None,
        occlusion_query_set: None,
        multiview_mask: None,
    });
    pass.set_pipeline(pipeline);
    pass.set_bind_group(0, bind_group, &[]);
    pass.draw(0..3, 0..1);
}

impl Gpu {
    fn new(low_power: bool) -> Result<Self, String> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: if low_power {
                wgpu::PowerPreference::LowPower
            } else {
                wgpu::PowerPreference::HighPerformance
            },
            ..Default::default()
        }))
        .map_err(|e| e.to_string())?;
        let info = adapter.get_info();
        let adapter_info = format!("{} ({:?})", info.name, info.backend);
        let (device, queue) = pollster::block_on(adapter.request_device(&Default::default()))
            .map_err(|e| e.to_string())?;

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("develop"),
            source: wgpu::ShaderSource::Wgsl(include_str!("develop.wgsl").into()),
        });
        let format = wgpu::TextureFormat::Bgra8Unorm;
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("develop"),
            layout: None,
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            primitive: Default::default(),
            depth_stencil: None,
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });
        let layout = pipeline.get_bind_group_layout(0);
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let uniforms = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("uniforms"),
            size: std::mem::size_of::<Uniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        Ok(Self {
            device,
            queue,
            pipeline,
            sampler,
            uniforms,
            layout,
            adapter_info,
        })
    }

    fn adapter_info(&self) -> String {
        self.adapter_info.clone()
    }

    fn upload_source(&self, img: LinearImage) -> GpuSource {
        // Expand RGB f16 to RGBA f16 (alpha 1.0) for texture upload.
        let mut rgba = Vec::with_capacity(img.width as usize * img.height as usize * 4);
        for px in img.rgb_f16.chunks_exact(3) {
            rgba.extend_from_slice(px);
            rgba.push(0x3C00);
        }
        let tex = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("source"),
            size: wgpu::Extent3d {
                width: img.width,
                height: img.height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba16Float,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        self.queue.write_texture(
            tex.as_image_copy(),
            bytemuck::cast_slice(&rgba),
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(img.width.saturating_mul(8)),
                rows_per_image: None,
            },
            wgpu::Extent3d {
                width: img.width,
                height: img.height,
                depth_or_array_layers: 1,
            },
        );
        let view = tex.create_view(&Default::default());
        let bind = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &self.layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: self.uniforms.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
            ],
        });
        GpuSource {
            image: img,
            bind,
            texture: tex,
        }
    }

    fn run(self, rx: mpsc::Receiver<Msg>, on_frame: impl Fn(Rendered)) {
        // Interactive and settled previews deliberately keep separate target
        // sets. Alternating 720px drag frames with 1280px release frames must
        // not reallocate a texture and mapped readback buffer every time.
        let mut interactive_preview_slot: Option<Targets> = None;
        let mut settled_preview_slot: Option<Targets> = None;
        let mut export_slot: Option<Targets> = None;
        // The editing source and the export source live in separate
        // textures: an export (native detail, derivative, file export) must
        // never replace the texture live previews sample from, and an
        // export must never render a different photo that merely has the
        // same dimensions.
        let mut editing: Option<GpuSource> = None;
        let mut export_src: Option<GpuSource> = None;
        let mut exports: VecDeque<ExportReq> = VecDeque::new();
        loop {
            // Block only when idle, then drain to the latest.
            let mut pending = Vec::new();
            if exports.is_empty() {
                match rx.recv() {
                    Ok(m) => pending.push(m),
                    Err(_) => break,
                }
            }
            pending.extend(rx.try_iter());
            let mut latest_preview: Option<Job> = None;
            for m in pending {
                match m {
                    Msg::Source { image, warm, ack } => {
                        // Jobs queued before the switch belong to the old
                        // photo; the UI drops their frames anyway.
                        latest_preview = None;
                        // Free the old textures before allocating new ones.
                        editing = None;
                        export_src = None;
                        let edit = editing.insert(self.upload_source(image));
                        if let Some(warm) = warm {
                            let target = fit_long_edge(
                                crop_target(
                                    edit.image.width,
                                    edit.image.height,
                                    warm.geom.rect,
                                    warm.geom.rotation,
                                ),
                                PREVIEW_INTERACTIVE_EDGE,
                            );
                            let u = pack(&warm.params, warm.split, target, &edit.image, &warm.geom);
                            // Deliberately discard this frame. Besides shader
                            // execution it exercises the first GPU readback/map
                            // and CPU conversion so the first real drag is hot.
                            let _ = render_with(
                                &self.device,
                                &self.queue,
                                &self.pipeline,
                                &self.uniforms,
                                &mut interactive_preview_slot,
                                &edit.bind,
                                &u,
                                PixelOrder::Bgra,
                                None,
                                0,
                            );
                        }
                        if let Some(ack) = ack {
                            ack.send(()).ok();
                        }
                    }
                    Msg::Preview(job) => latest_preview = Some(job),
                    Msg::Export {
                        image,
                        params,
                        split,
                        geom,
                        reply,
                    } => exports.push_back(ExportReq {
                        image,
                        params,
                        split,
                        geom,
                        reply,
                    }),
                }
            }
            // Interactive work wins over cache/export work. A slider release
            // commonly queues both; paint the live value first so derivative
            // generation cannot delay feedback.
            if let (Some(job), Some(edit)) = (latest_preview, editing.as_ref()) {
                let target = fit_long_edge(
                    crop_target(
                        edit.image.width,
                        edit.image.height,
                        job.geom.rect,
                        job.geom.rotation,
                    ),
                    job.preview_long_edge.clamp(256, PREVIEW_LONG_EDGE),
                );
                let u = pack(&job.params, job.split, target, &edit.image, &job.geom);
                let slot = if job.preview_long_edge <= PREVIEW_INTERACTIVE_EDGE {
                    &mut interactive_preview_slot
                } else {
                    &mut settled_preview_slot
                };
                let frame = render_with(
                    &self.device,
                    &self.queue,
                    &self.pipeline,
                    &self.uniforms,
                    slot,
                    &edit.bind,
                    &u,
                    PixelOrder::Bgra,
                    job.photo_id,
                    job.seq,
                );
                on_frame(frame);
            }
            // One export per turn: previews submitted while a batch of
            // exports is queued are picked up between them.
            if let Some(req) = exports.pop_front() {
                let r = self.render_export_image(
                    req.image,
                    req.params,
                    req.split,
                    req.geom,
                    &editing,
                    &mut export_src,
                    &mut export_slot,
                );
                req.reply.send(r).ok();
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn render_export_image(
        &self,
        image: LinearImage,
        params: [f32; PARAM_COUNT],
        split: f32,
        geom: CropRender,
        editing: &Option<GpuSource>,
        export_src: &mut Option<GpuSource>,
        slot: &mut Option<Targets>,
    ) -> Result<Rendered, String> {
        if image.width == 0 || image.height == 0 {
            return Err("empty source image".to_string());
        }
        // Derivatives render from the editing source itself: reuse its
        // texture instead of re-uploading after every slider gesture.
        let src = match editing.as_ref().filter(|e| same_image(&e.image, &image)) {
            Some(e) => e,
            None => {
                if !export_src
                    .as_ref()
                    .is_some_and(|x| same_image(&x.image, &image))
                {
                    *export_src = None;
                    *export_src = Some(self.upload_source(image));
                }
                export_src.as_ref().unwrap()
            }
        };
        let (w, h) = (src.image.width, src.image.height);
        // U08: output dims follow the crop rect.
        let u = pack(
            &params,
            split,
            crop_target(w, h, geom.rect, geom.rotation),
            &src.image,
            &geom,
        );
        let frame = render_with(
            &self.device,
            &self.queue,
            &self.pipeline,
            &self.uniforms,
            slot,
            &src.bind,
            &u,
            PixelOrder::Rgba,
            None,
            0,
        );
        Ok(frame)
    }
}

/// Render using already-uploaded source state. `bind` must belong to the
/// source whose dims were packed into `u`.
#[allow(clippy::too_many_arguments)]
fn render_with(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    pipeline: &wgpu::RenderPipeline,
    uniforms: &wgpu::Buffer,
    slot: &mut Option<Targets>,
    bind: &wgpu::BindGroup,
    u: &Uniforms,
    order: PixelOrder,
    photo_id: Option<i64>,
    seq: u64,
) -> Rendered {
    use std::time::Instant;
    let t0 = Instant::now();
    // Create-or-reuse target dims from uniforms (target_w/h in rows[3]).
    let (w, h) = (u.rows[3][2] as u32, u.rows[3][3] as u32);
    let fresh = match slot.as_ref() {
        Some(t) => t.width != w || t.height != h,
        None => true,
    };
    if fresh {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("target"),
            size: wgpu::Extent3d {
                width: w,
                height: h,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Bgra8Unorm,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let align = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
        let padded_row = (w * 4).div_ceil(align) * align;
        let readback = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("readback"),
            size: (padded_row * h) as u64,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        *slot = Some(Targets {
            view: texture.create_view(&Default::default()),
            texture,
            readback,
            width: w,
            height: h,
            padded_row,
        });
    }
    let t = slot.as_ref().unwrap();
    queue.write_buffer(uniforms, 0, bytemuck::bytes_of(u));
    let mut encoder = device.create_command_encoder(&Default::default());
    fullscreen_pass(&mut encoder, &t.view, pipeline, bind);
    let tex_copy = t.texture.as_image_copy();
    let (readback, width, height, padded_row) = {
        let t = slot.as_ref().unwrap();
        (&t.readback, t.width, t.height, t.padded_row)
    };
    encoder.copy_texture_to_buffer(
        tex_copy,
        wgpu::TexelCopyBufferInfo {
            buffer: readback,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(padded_row),
                rows_per_image: None,
            },
        },
        wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
    );
    queue.submit([encoder.finish()]);
    let slice = readback.slice(..);
    slice.map_async(wgpu::MapMode::Read, |r| r.expect("map readback"));
    device
        .poll(wgpu::PollType::wait_indefinitely())
        .expect("poll");
    let t1 = Instant::now();

    let (w, h, row) = (width as usize, height as usize, padded_row as usize);
    let mut rgba = Vec::with_capacity(w * h * 4);
    let mut hist = [0f32; 48];
    let (mut shadow, mut highlight) = (0usize, 0usize);
    {
        let mapped = slice.get_mapped_range();
        for y in 0..h {
            // Target bytes are BGRA.
            let line = &mapped[y * row..y * row + w * 4];
            rgba.extend_from_slice(line);
            if y % 2 == 0 {
                for px in line.chunks_exact(4).step_by(2) {
                    let l = 0.0722 * px[0] as f32 + 0.7152 * px[1] as f32 + 0.2126 * px[2] as f32;
                    hist[((l / 256.) * 48.).min(47.) as usize] += 1.;
                }
            }
        }
    }
    readback.unmap();
    if order == PixelOrder::Rgba {
        for px in rgba.chunks_exact_mut(4) {
            px.swap(0, 2);
        }
    } else {
        // Clip stats are symmetric in R/B; only previews display them.
        for px in rgba.chunks_exact(4) {
            let (lo, hi) = (px[0].min(px[1]).min(px[2]), px[0].max(px[1]).max(px[2]));
            shadow += (hi == 0) as usize;
            highlight += (lo == 255) as usize;
        }
    }
    let n = (w * h).max(1) as f32;
    let max = hist.iter().cloned().fold(1., f32::max);
    for b in &mut hist {
        *b = (*b / max).sqrt();
    }
    Rendered {
        rgba,
        bgra: order == PixelOrder::Bgra,
        width,
        height,
        photo_id,
        seq,
        gpu_ms: t1.duration_since(t0).as_secs_f32() * 1000.,
        copy_ms: t1.elapsed().as_secs_f32() * 1000.,
        histogram: hist,
        clip: (shadow as f32 / n, highlight as f32 / n),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_image() -> LinearImage {
        LinearImage {
            width: 64,
            height: 48,
            rgb_f16: vec![0x3C00; 64 * 48 * 3],
            wb_as_shot: [2.0, 1.0, 1.5],
            cam_to_xyz: [[1., 0., 0.], [0., 1., 0.], [0., 0., 1.]],
            black: 0.0,
            white: 1.0,
            camera: "Test".into(),
            display_referred: false,
        }
    }

    /// GPU renderer for tests. Skips only when the machine has no adapter;
    /// any other init failure (shader validation!) fails the test.
    fn test_renderer(on_frame: impl Fn(Rendered) + Send + 'static) -> Option<Renderer> {
        match Renderer::spawn(on_frame) {
            Ok((r, _)) => Some(r),
            Err(e) if e.to_lowercase().contains("adapter") => {
                eprintln!("no GPU adapter; skipping ({e})");
                None
            }
            Err(e) => panic!("renderer failed to start: {e}"),
        }
    }

    fn solid(w: u32, h: u32, rgb: [f32; 3]) -> LinearImage {
        let px = rgb.map(|v| half::f16::from_f32(v).to_bits());
        LinearImage {
            width: w,
            height: h,
            rgb_f16: px.repeat((w * h) as usize),
            ..test_image()
        }
    }

    /// Exports never render a stale texture that merely shares dims, and
    /// never leave the live preview sampling the export source.
    #[test]
    fn export_and_preview_sources_stay_separate() {
        let (tx, rx) = mpsc::channel();
        let Some(r) = test_renderer(move |f| {
            tx.send(f).ok();
        }) else {
            return;
        };
        let params = laika_core::edit::defaults();
        let geom = CropRender::default();
        let red = solid(64, 48, [0.8, 0.02, 0.02]);
        let blue = solid(64, 48, [0.02, 0.02, 0.8]);
        r.set_source(red.clone());
        let a = r.render_export(blue.clone(), params, 0., geom).unwrap();
        // RGBA: blue dominates in byte 2.
        assert!(!a.bgra && a.rgba[2] > a.rgba[0], "{:?}", &a.rgba[..4]);
        let b = r.render_export(red.clone(), params, 0., geom).unwrap();
        assert!(
            b.rgba[0] > b.rgba[2],
            "same-dims export rendered stale source"
        );
        let c = r
            .render_export(solid(64, 48, [0.02, 0.8, 0.02]), params, 0., geom)
            .unwrap();
        assert!(c.rgba[1] > c.rgba[0] && c.rgba[1] > c.rgba[2]);
        // Preview still samples the editing source (red), delivered BGRA.
        r.submit(Job {
            params,
            split: 0.,
            geom,
            preview_long_edge: PREVIEW_LONG_EDGE,
            photo_id: Some(1),
            seq: 1,
        });
        let f = rx.recv().unwrap();
        assert!(f.bgra && f.rgba[2] > f.rgba[0], "{:?}", &f.rgba[..4]);
        assert_eq!(f.rgba.len(), (f.width * f.height * 4) as usize);
    }

    /// Crop tool frame view: straightened corners outside the source read
    /// as dark canvas; the normal render clamps edge texels instead.
    #[test]
    fn frame_view_paints_outside_source_dark() {
        let Some(r) = test_renderer(|_| {}) else {
            return;
        };
        let params = laika_core::edit::defaults();
        let img = solid(64, 48, [0.8, 0.8, 0.8]);
        let rotated = CropRender {
            angle_rad: 30f32.to_radians(),
            frame_view: true,
            ..CropRender::default()
        };
        let f = r.render_export(img.clone(), params, 0., rotated).unwrap();
        let corner = &f.rgba[..4];
        let center = &f.rgba[((24 * 64 + 32) * 4) as usize..((24 * 64 + 32) * 4 + 4) as usize];
        assert!(corner[0] < 40, "corner {corner:?}");
        assert!(center[0] > 150, "center {center:?}");
        let plain = CropRender {
            frame_view: false,
            ..rotated
        };
        let g = r.render_export(img, params, 0., plain).unwrap();
        assert!(g.rgba[0] > 150, "clamped corner {:?}", &g.rgba[..4]);
    }

    #[test]
    fn pack_maps_params_to_rows() {
        let img = test_image();
        let mut params = laika_core::edit::defaults();
        params[0] = 5480.;
        params[1] = 6.;
        params[2] = 0.35;
        params[3] = 12.;
        params[4] = -40.;
        params[5] = 28.;
        params[6] = 8.;
        params[7] = -14.;
        params[8] = 10.;
        params[9] = 4.;
        params[10] = 18.;
        params[11] = 0.;
        let u = pack(&params, 0.38, (64, 48), &img, &CropRender::default());
        assert_eq!(u.rows[0], [5480., 6., 0.35, 12.]);
        assert_eq!(u.rows[1], [-40., 28., 8., -14.]);
        assert_eq!(u.rows[2], [10., 4., 18., 0.]);
        assert_eq!(u.rows[3], [0.38, 0., 64., 48.]);
        assert_eq!(u.rows[4], [64., 48., 0., 0.]);
        assert_eq!(u.rows[5], [0., 0., 1., 1.]);
        assert_eq!(u.wb, [2.0, 1.0, 1.5, 0.]);
        assert_eq!(u.cam[0], [1., 0., 0., 0.]);
        let mut fx = laika_core::edit::defaults();
        fx[46] = 30.;
        fx[47] = -50.;
        fx[48] = 10.;
        let uf = pack(&fx, 0., (64, 48), &img, &CropRender::default());
        assert_eq!(uf.rows[15], [30., -50., 10., 0.]);
        let raster = LinearImage {
            display_referred: true,
            ..test_image()
        };
        let ur = pack(&fx, 0., (64, 48), &raster, &CropRender::default());
        assert_eq!(ur.rows[15], [30., -50., 10., 1.]);
        // Color grading params 49..62 pack in order across rows 16..19.
        assert_eq!(uf.rows[19], [50., 0., 0., 0.]);
        let mut cg = laika_core::edit::defaults();
        for (k, i) in (49..63).enumerate() {
            cg[i] = k as f32;
        }
        let ug = pack(&cg, 0., (64, 48), &img, &CropRender::default());
        assert_eq!(ug.rows[16], [0., 1., 2., 3.]);
        assert_eq!(ug.rows[18], [8., 9., 10., 11.]);
        assert_eq!(ug.rows[19], [12., 13., 0., 0.]);
        // Uniform buffer size matches WGSL struct (27 vec4s).
        assert_eq!(std::mem::size_of::<Uniforms>(), 27 * 16);
    }

    #[test]
    fn u08_pack_geometry_and_crop_target() {
        let img = test_image();
        let geom = CropRender {
            rect: [0.25, 0.25, 0.5, 0.5],
            angle_rad: 0.1,
            flip_h: true,
            flip_v: false,
            rotation: 0,
            frame_view: false,
            warp: WARP_IDENTITY,
        };
        assert!(!geom.is_default());
        assert!(CropRender::default().is_default());
        let u = pack(&[0f32; PARAM_COUNT], 0., (64, 48), &img, &geom);
        // Default params carry the identity curve row.
        let ud = pack(
            &laika_core::edit::defaults(),
            0.,
            (64, 48),
            &img,
            &CropRender::default(),
        );
        assert_eq!(ud.rows[6], [0.2, 0.4, 0.6, 0.8]);
        assert_eq!(u.rows[13], [0., 0., 0., 0.]);
        assert_eq!(u.rows[14][0], 0.);
        assert_eq!(u.rows[3][1], 0.1);
        assert_eq!(u.rows[4], [64., 48., 1., 0.]);
        assert_eq!(u.rows[5], [0.25, 0.25, 0.5, 0.5]);
        assert_eq!(crop_target(6000, 4000, [0., 0., 1., 1.], 0), (6000, 4000));
        assert_eq!(
            crop_target(6000, 4000, [0.25, 0.25, 0.5, 0.5], 0),
            (3000, 2000)
        );
        assert_eq!(crop_target(100, 100, [0., 0., 0., 0.], 0), (1, 1));
        assert_eq!(fit_long_edge((2048, 1365), 1280), (1280, 853));
        assert_eq!(fit_long_edge((2048, 1365), 720), (720, 479));
        assert_eq!(fit_long_edge((640, 480), 1280), (640, 480));
        assert_eq!(fit_long_edge((1000, 2000), 1280), (640, 1280));
        // V15: odd rotations swap axes (4000×6000 source → 6000×4000 out).
        assert_eq!(crop_target(4000, 6000, [0., 0., 1., 1.], 1), (6000, 4000));
        assert_eq!(crop_target(4000, 6000, [0., 0., 0.5, 1.], 1), (3000, 4000));
        assert_eq!(crop_target(4000, 6000, [0., 0., 1., 1.], 2), (4000, 6000));
        // Rotation rides the o.z uniform slot.
        let mut rgeom = CropRender::default();
        rgeom.rotation = 1;
        assert!(!rgeom.is_default());
        let ur = pack(&[0f32; PARAM_COUNT], 0., (64, 48), &img, &rgeom);
        assert_eq!(ur.rows[14][2], 1.);
    }

    #[test]
    fn pack_warp_rows_and_active_flag() {
        let img = test_image();
        let u0 = pack(
            &[0f32; PARAM_COUNT],
            0.,
            (64, 48),
            &img,
            &CropRender::default(),
        );
        assert_eq!(
            u0.warp,
            [[1., 0., 0., 0.], [0., 1., 0., 0.], [0., 0., 1., 0.]]
        );
        let geom = CropRender {
            warp: [1.1, 0.2, 0.3, 0.4, 0.9, 0.6, 0.07, 0.08, 1.0],
            ..CropRender::default()
        };
        assert!(geom.warp_active());
        assert!(!geom.is_default());
        let u = pack(&[0f32; PARAM_COUNT], 0., (64, 48), &img, &geom);
        assert_eq!(u.warp[0], [1.1, 0.2, 0.3, 1.]);
        assert_eq!(u.warp[1], [0.4, 0.9, 0.6, 0.]);
        assert_eq!(u.warp[2], [0.07, 0.08, 1.0, 0.]);
        // Sub-threshold noise is not an active warp.
        let mut tiny = CropRender::default();
        tiny.warp[2] = 1e-8;
        assert!(!tiny.warp_active() && tiny.is_default());
        let ut = pack(&[0f32; PARAM_COUNT], 0., (64, 48), &img, &tiny);
        assert_eq!(ut.warp[0][3], 0.);
    }

    fn close(a: Option<(f32, f32)>, b: (f32, f32)) -> bool {
        a.is_some_and(|(x, y)| (x - b.0).abs() < 1e-5 && (y - b.1).abs() < 1e-5)
    }

    #[test]
    fn warp_display_uv_math() {
        // Identity returns the input for any aspect.
        for &(u, v, a) in &[(0.1, 0.9, 1.0), (0.3, 0.2, 1.5), (1.0, 0.0, 0.75)] {
            assert!(close(warp_display_uv(&WARP_IDENTITY, u, v, a), (u, v)));
        }
        // Pure scale: (1,1) at a=1 → centered (0.5,0.5) → (0.25,0.25).
        let s = [0.5, 0., 0., 0., 0.5, 0., 0., 0., 1.];
        assert!(close(warp_display_uv(&s, 1., 1., 1.), (0.75, 0.75)));
        // Translation with a = 2: x shifts by 0.2 centered = 0.1 in u.
        let t = [1., 0., 0.2, 0., 1., -0.1, 0., 0., 1.];
        assert!(close(warp_display_uv(&t, 0.5, 0.5, 2.), (0.6, 0.4)));
        assert!(close(warp_display_uv(&t, 0.75, 0.25, 2.), (0.85, 0.15)));
        // Projective: u=0.75, v=0.5, a=2 → x=0.5, y=0.
        // X = 1*0.5 + 0.1*0 + 0 = 0.5; Y = 0.2*0.5 + 1*0 + 0.05 = 0.15;
        // W = 0.4*0.5 + 0 + 1 = 1.2 → (0.5/1.2/2 + 0.5, 0.15/1.2 + 0.5).
        let p = [1., 0.1, 0., 0.2, 1., 0.05, 0.4, 0., 1.];
        assert!(close(
            warp_display_uv(&p, 0.75, 0.5, 2.),
            (0.5 / 1.2 / 2. + 0.5, 0.15 / 1.2 + 0.5)
        ));
        // W → 0 is outside.
        let z = [1., 0., 0., 0., 1., 0., 0., 0., 0.];
        assert_eq!(warp_display_uv(&z, 0.5, 0.5, 1.), None);
    }

    /// 64×48 source: R ramps with x, G ramps with y (linear).
    fn gradient(w: u32, h: u32) -> LinearImage {
        let mut px = Vec::with_capacity((w * h * 3) as usize);
        for y in 0..h {
            for x in 0..w {
                let r = 0.05 + 0.6 * (x as f32 + 0.5) / w as f32;
                let g = 0.05 + 0.6 * (y as f32 + 0.5) / h as f32;
                for c in [r, g, 0.2] {
                    px.push(half::f16::from_f32(c).to_bits());
                }
            }
        }
        LinearImage {
            width: w,
            height: h,
            rgb_f16: px,
            ..test_image()
        }
    }

    fn px(f: &Rendered, x: u32, y: u32) -> [u8; 4] {
        let i = ((y * f.width + x) * 4) as usize;
        f.rgba[i..i + 4].try_into().unwrap()
    }

    /// Bilinear read of an RGBA frame at a uv (pixel centers at +0.5).
    fn sample_uv(f: &Rendered, u: f32, v: f32) -> [f32; 3] {
        let fx = (u * f.width as f32 - 0.5).clamp(0., (f.width - 1) as f32);
        let fy = (v * f.height as f32 - 0.5).clamp(0., (f.height - 1) as f32);
        let (x0, y0) = (fx.floor() as u32, fy.floor() as u32);
        let (x1, y1) = ((x0 + 1).min(f.width - 1), (y0 + 1).min(f.height - 1));
        let (tx, ty) = (fx - x0 as f32, fy - y0 as f32);
        let mut out = [0f32; 3];
        for (c, o) in out.iter_mut().enumerate() {
            let a = px(f, x0, y0)[c] as f32 * (1. - tx) + px(f, x1, y0)[c] as f32 * tx;
            let b = px(f, x0, y1)[c] as f32 * (1. - tx) + px(f, x1, y1)[c] as f32 * tx;
            *o = a * (1. - ty) + b * ty;
        }
        out
    }

    /// GPU warp matches the CPU mirror: each warped output pixel equals the
    /// unwarped render sampled where `warp_display_uv` predicts.
    #[test]
    fn warp_render_matches_cpu_mirror() {
        let Some(r) = test_renderer(|_| {}) else {
            return;
        };
        let params = laika_core::edit::defaults();
        let img = gradient(64, 48);
        let aspect = 64. / 48.;
        let base = r
            .render_export(img.clone(), params, 0., CropRender::default())
            .unwrap();
        for warp in [
            [0.5, 0., 0., 0., 0.5, 0., 0., 0., 1.],
            [0.8, 0.05, 0.02, -0.03, 0.85, 0.01, 0.1, -0.08, 1.],
        ] {
            let geom = CropRender {
                warp,
                ..CropRender::default()
            };
            let f = r.render_export(img.clone(), params, 0., geom).unwrap();
            assert_eq!((f.width, f.height), (64, 48));
            for &(x, y) in &[(32, 24), (0, 0), (63, 47), (0, 24), (63, 0), (16, 40)] {
                let (u, v) = ((x as f32 + 0.5) / 64., (y as f32 + 0.5) / 48.);
                let (su, sv) = warp_display_uv(&warp, u, v, aspect).unwrap();
                assert!((0. ..=1.).contains(&su) && (0. ..=1.).contains(&sv));
                let want = sample_uv(&base, su, sv);
                let got = px(&f, x, y);
                for c in 0..3 {
                    assert!(
                        (got[c] as f32 - want[c]).abs() <= 3.,
                        "warp {warp:?} px ({x},{y}) got {got:?} want {want:?}"
                    );
                }
            }
        }
        // Identity warp is bit-identical to the plain render.
        let id = CropRender {
            warp: WARP_IDENTITY,
            ..CropRender::default()
        };
        let g = r.render_export(img, params, 0., id).unwrap();
        assert_eq!(g.rgba, base.rgba);
    }

    /// Fixed scene-linear input for the RAW guard: row 0 a colored ramp,
    /// row 1 a neutral quadratic ramp (shadows to past the shoulder).
    fn raw_guard_image() -> LinearImage {
        let (w, h) = (32u32, 2u32);
        let mut px = Vec::with_capacity((w * h * 3) as usize);
        for y in 0..h {
            for x in 0..w {
                let t = x as f32 / (w - 1) as f32;
                let rgb = if y == 0 {
                    [t * 0.45, t * 0.35 + 0.005, (1. - t) * 0.3]
                } else {
                    [t * t * 0.8; 3]
                };
                px.extend(rgb.map(|v| half::f16::from_f32(v).to_bits()));
            }
        }
        LinearImage {
            width: w,
            height: h,
            rgb_f16: px,
            ..test_image()
        }
    }

    fn raw_guard_configs() -> Vec<[f32; PARAM_COUNT]> {
        let d = laika_core::edit::defaults();
        let mut tone = d;
        for (i, v) in [
            (0, 6500.),
            (1, 20.),
            (2, 0.5),
            (3, 40.),
            (4, -30.),
            (5, 25.),
            (6, 10.),
            (7, -5.),
            (10, 20.),
            (11, -10.),
        ] {
            tone[i] = v;
        }
        let mut veil = d;
        veil[46] = -50.;
        veil[3] = -30.;
        let mut clear = d;
        clear[46] = 40.;
        vec![d, tone, veil, clear]
    }

    /// RAW path guard: scene-referred sources keep the base look. Goldens
    /// are every 4th pixel of both rows, captured from the shader before
    /// display-referred sources were split off (±1 level for GPU drift).
    #[test]
    fn raw_path_output_unchanged() {
        const GOLDEN: [[[u8; 3]; 16]; 4] = [
            [
                [0, 15, 233],
                [171, 110, 230],
                [211, 156, 225],
                [226, 182, 218],
                [234, 199, 208],
                [239, 209, 191],
                [242, 217, 162],
                [245, 223, 102],
                [0, 0, 0],
                [72, 40, 57],
                [166, 114, 145],
                [212, 172, 198],
                [232, 206, 223],
                [242, 224, 236],
                [247, 234, 243],
                [250, 241, 247],
            ],
            [
                [36, 55, 241],
                [214, 165, 242],
                [237, 203, 240],
                [244, 219, 237],
                [248, 228, 232],
                [250, 234, 224],
                [252, 239, 213],
                [253, 242, 190],
                [11, 11, 11],
                [110, 68, 85],
                [209, 163, 186],
                [238, 212, 226],
                [247, 232, 240],
                [251, 242, 247],
                [253, 245, 249],
                [254, 249, 252],
            ],
            [
                [36, 47, 230],
                [165, 118, 223],
                [200, 153, 215],
                [216, 174, 207],
                [225, 188, 197],
                [231, 199, 183],
                [235, 206, 165],
                [238, 212, 139],
                [26, 26, 26],
                [87, 61, 75],
                [161, 120, 143],
                [202, 165, 188],
                [222, 195, 212],
                [234, 213, 226],
                [240, 225, 235],
                [245, 232, 240],
            ],
            [
                [0, 15, 233],
                [169, 94, 231],
                [210, 140, 226],
                [226, 169, 216],
                [234, 187, 200],
                [239, 203, 176],
                [242, 214, 142],
                [245, 222, 82],
                [0, 0, 0],
                [71, 33, 54],
                [165, 101, 140],
                [211, 159, 194],
                [232, 195, 221],
                [241, 217, 234],
                [247, 229, 242],
                [250, 237, 246],
            ],
        ];
        let Some(r) = test_renderer(|_| {}) else {
            return;
        };
        let img = raw_guard_image();
        assert!(!img.display_referred);
        for (ci, (p, want)) in raw_guard_configs().into_iter().zip(GOLDEN).enumerate() {
            let f = r
                .render_export(img.clone(), p, 0., CropRender::default())
                .unwrap();
            let coords = (0..2).flat_map(|y| (0..32).step_by(4).map(move |x| (x, y)));
            for ((x, y), w) in coords.zip(want) {
                let got = px(&f, x, y);
                for c in 0..3 {
                    assert!(
                        (got[c] as i32 - w[c] as i32).abs() <= 1,
                        "config {ci} px ({x},{y}) got {got:?} want {w:?}"
                    );
                }
            }
        }
    }

    /// An 8-bit sRGB PNG through the real raster bridge.
    fn raster(
        name: &str,
        w: u32,
        h: u32,
        f: impl Fn(u32, u32) -> [u8; 3],
    ) -> (image::RgbImage, LinearImage) {
        let dir = std::env::temp_dir().join(format!("laika-raster-{}-{name}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("src.png");
        let src = image::RgbImage::from_fn(w, h, |x, y| image::Rgb(f(x, y)));
        src.save(&path).unwrap();
        let img = laika_raw::decode::linear_from_raster(&path, None).unwrap();
        std::fs::remove_dir_all(&dir).ok();
        (src, img)
    }

    /// Display-referred sources render as themselves at default params:
    /// no base gain or filmic look, before and after alike.
    #[test]
    fn raster_defaults_render_near_identity() {
        let Some(r) = test_renderer(|_| {}) else {
            return;
        };
        // Every code value in every channel, in varied (incl. saturated)
        // combinations.
        let (src, img) = raster("identity", 64, 64, |x, y| {
            let i = y * 64 + x;
            [
                (i % 256) as u8,
                ((i * 7 + 50) % 256) as u8,
                ((i * 13 + 200) % 256) as u8,
            ]
        });
        assert!(img.display_referred);
        let params = laika_core::edit::defaults();
        for split in [0., 1.] {
            let f = r
                .render_export(img.clone(), params, split, CropRender::default())
                .unwrap();
            assert_eq!((f.width, f.height), (64, 64));
            let (mut max, mut sum) = (0i32, 0i64);
            for (o, s) in f.rgba.chunks_exact(4).zip(src.pixels()) {
                for c in 0..3 {
                    let d = (o[c] as i32 - s[c] as i32).abs();
                    max = max.max(d);
                    sum += d as i64;
                }
            }
            let mean = sum as f64 / (64. * 64. * 3.);
            assert!(
                max <= 2 && mean < 0.5,
                "split {split}: max {max} mean {mean}"
            );
        }
        // The flag rides through the source identity: the same pixels as a
        // scene-referred source get the base look (much brighter).
        let raw_like = LinearImage {
            display_referred: false,
            ..img.clone()
        };
        let g = r
            .render_export(raw_like, params, 0., CropRender::default())
            .unwrap();
        let mean = |b: &[u8]| b.iter().map(|&v| v as f64).sum::<f64>() / b.len() as f64;
        let src_mean = mean(src.as_raw());
        let look_mean = mean(&g.rgba) - 255. / 4.;
        assert!(
            look_mean * 4. / 3. > src_mean + 20.,
            "{look_mean} vs {src_mean}"
        );
    }

    /// Edits on a display-referred source still act around the original:
    /// exposure scales linear light, contrast pivots at middle gray, tint
    /// counts from its default.
    #[test]
    fn raster_edits_behave() {
        let Some(r) = test_renderer(|_| {}) else {
            return;
        };
        let codes = [30u8, 64, 118, 128, 200];
        let (_, img) = raster("edits", codes.len() as u32, 1, |x, _| {
            [codes[x as usize]; 3]
        });
        let render = |edit: &dyn Fn(&mut [f32; PARAM_COUNT])| {
            let mut p = laika_core::edit::defaults();
            edit(&mut p);
            let f = r
                .render_export(img.clone(), p, 0., CropRender::default())
                .unwrap();
            (0..codes.len() as u32)
                .map(|x| px(&f, x, 0))
                .collect::<Vec<_>>()
        };
        let to_lin = |c: u8| {
            let v = c as f32 / 255.;
            if v <= 0.04045 {
                v / 12.92
            } else {
                ((v + 0.055) / 1.055).powf(2.4)
            }
        };
        let to_code = |l: f32| {
            let v = if l <= 0.0031308 {
                l * 12.92
            } else {
                1.055 * l.powf(1. / 2.4) - 0.055
            };
            (v.clamp(0., 1.) * 255.).round() as i32
        };
        // Exposure: ±1 EV doubles / halves linear light.
        for (ev, scale) in [(1f32, 2f32), (-1., 0.5)] {
            let out = render(&|p| p[2] = ev);
            for (o, &c) in out.iter().zip(&codes) {
                let want = to_code(to_lin(c) * scale);
                assert!(
                    (o[1] as i32 - want).abs() <= 2,
                    "EV {ev}: code {c} got {o:?} want {want}"
                );
            }
        }
        // Contrast: middle gray (linear 0.18, code 118) holds; darker
        // codes darken, brighter ones brighten.
        let out = render(&|p| p[3] = 50.);
        assert!((out[2][1] as i32 - 118).abs() <= 2, "pivot {:?}", out[2]);
        assert!(out[0][1] < 30 && out[1][1] < 64, "{out:?}");
        assert!(out[4][1] > 200, "{out:?}");
        // Tint: default is neutral; positive (magenta) lowers green.
        let out = render(&|p| p[1] = 40.);
        assert!(out[3][1] < out[3][0] && out[3][0] == out[3][2], "{out:?}");
    }

    /// Expanding warp uncovers the corners: white in the normal render,
    /// dark canvas in the crop tool's frame view.
    #[test]
    fn expanding_warp_corners_white_or_dark() {
        let Some(r) = test_renderer(|_| {}) else {
            return;
        };
        let params = laika_core::edit::defaults();
        let img = solid(64, 48, [0.02, 0.02, 0.02]);
        let geom = CropRender {
            warp: [2., 0., 0., 0., 2., 0., 0., 0., 1.],
            ..CropRender::default()
        };
        let f = r.render_export(img.clone(), params, 0., geom).unwrap();
        for &(x, y) in &[(0, 0), (63, 0), (0, 47), (63, 47)] {
            assert_eq!(px(&f, x, y), [255, 255, 255, 255], "corner ({x},{y})");
        }
        let center = px(&f, 32, 24);
        assert!(center[0] < 100, "center {center:?}");
        let fv = CropRender {
            frame_view: true,
            ..geom
        };
        let g = r.render_export(img, params, 0., fv).unwrap();
        for &(x, y) in &[(0, 0), (63, 47)] {
            let c = px(&g, x, y);
            assert!(
                c[0] < 40 && c[1] < 40 && c[2] < 40,
                "frame view corner {c:?}"
            );
        }
        assert_eq!(px(&g, 32, 24)[..3], center[..3]);
    }

    /// V22 gate: a keystoned, rolled building is detected, solved (Full
    /// Upright) and rendered through the GPU warp; the exported frame's
    /// verticals and horizontals come back straight.
    #[test]
    fn v22_upright_straightens_a_keystoned_building_on_gpu() {
        use laika_core::{lines, upright};
        let Some(r) = test_renderer(|_| {}) else {
            return;
        };
        let (w, h) = (480u32, 320u32);
        let aspect = w as f32 / h as f32;
        let truth = upright::Transform {
            vertical: -35.,
            horizontal: 10.,
            rotate: 2.,
            ..upright::Transform::default()
        };
        let f = upright::forward(&truth, aspect);
        // Corrected-space scene: two dark towers with a lit gap.
        let dark = |x: f64, y: f64| {
            let in_y = (-0.38..0.36).contains(&y);
            in_y && ((-0.55..-0.2).contains(&x) || (0.1..0.5).contains(&x))
        };
        let mut rgb = Vec::with_capacity((w * h * 3) as usize);
        let mut rgb_f32 = Vec::with_capacity((w * h * 3) as usize);
        for y in 0..h {
            for x in 0..w {
                let mut cover = 0.;
                for (sx, sy) in [(0.25, 0.25), (0.75, 0.25), (0.25, 0.75), (0.75, 0.75)] {
                    let (u, v) = ((x as f64 + sx) / w as f64, (y as f64 + sy) / h as f64);
                    let (cx, cy) = upright::to_centered(u, v, aspect as f64);
                    if let Some((qx, qy)) = upright::apply(&f, cx, cy) {
                        if dark(qx, qy) {
                            cover += 0.25;
                        }
                    }
                }
                let lum = (0.8 * (1. - cover) + 0.03 * cover) as f32;
                for _ in 0..3 {
                    rgb.push(half::f16::from_f32(lum).to_bits());
                    rgb_f32.push(lum);
                }
            }
        }
        let img = LinearImage {
            width: w,
            height: h,
            rgb_f16: rgb,
            wb_as_shot: [1., 1., 1.],
            ..test_image()
        };
        let (lum, lw, lh, _) =
            lines::luminance_for_analysis(&rgb_f32, w as usize, h as usize, 1024);
        let segs = lines::detect_segments(&lum, lw, lh);
        let found = upright::segments_to_lines(&segs, lw, lh, 0);
        let sol = upright::solve(upright::UprightMode::Full, &found, aspect).expect("solution");
        assert!(sol.before_deg > 2. && sol.after_deg < 0.3, "{sol:?}");
        let t = upright::Transform::default().with_auto(sol.auto);
        let warp = upright::warp_matrix(&t, aspect);
        let rect = laika_core::edit::constrain_crop_warp(
            [0., 0., 1., 1.],
            0.,
            0,
            w as f32,
            h as f32,
            &warp,
        );
        let geom = CropRender {
            rect,
            warp,
            ..CropRender::default()
        };
        let params = laika_core::edit::defaults();
        let out = r.render_export(img, params, 0., geom).unwrap();
        // Re-detect on the exported pixels.
        let mut out_rgb = Vec::with_capacity((out.width * out.height * 3) as usize);
        for p in out.rgba.chunks_exact(4) {
            for c in 0..3 {
                out_rgb.push((p[c] as f32 / 255.).powf(2.2));
            }
        }
        let (lum, lw, lh, _) =
            lines::luminance_for_analysis(&out_rgb, out.width as usize, out.height as usize, 1024);
        let segs = lines::detect_segments(&lum, lw, lh);
        let long: Vec<_> = segs
            .iter()
            .filter(|s| s.length() > 0.15 * lh.min(lw) as f32)
            .collect();
        let verticals = long.iter().filter(|s| s.is_near_vertical(20.)).count();
        assert!(verticals >= 3, "verticals {verticals} in {long:?}");
        for s in &long {
            let dev = if s.is_near_vertical(20.) {
                (s.angle_deg().abs() - 90.).abs()
            } else {
                s.angle_deg().abs()
            };
            assert!(
                dev < 0.6,
                "segment {s:?} still {dev}° off axis (auto {:?})",
                sol.auto
            );
        }
    }
}
