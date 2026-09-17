//! The render thread. It owns the wgpu device, keeps only the newest job, renders it, reads the
//! pixels back as BGRA and hands them to the UI. This is the plan's "render, readback, upload" loop.
//!
//! The source image is a half-float texture rendered once at startup, standing in for a decoded
//! RAW. The per-frame develop pass only samples it.

use std::sync::{Arc, Condvar, Mutex, mpsc};
use std::thread;
use std::time::Instant;

use bytemuck::{Pod, Zeroable};

pub struct Job {
    pub values: [f32; 12],
    pub split: f32,
    pub seq: u64,
}

pub struct Rendered {
    pub bgra: Vec<u8>,
    pub width: u32,
    pub height: u32,
    pub seq: u64,
    /// Uniform write, encode, submit, and waiting for the GPU pass plus the copy into the mapped buffer.
    pub gpu_ms: f32,
    /// CPU copy of the mapped rows into the image buffer, plus the sparse histogram.
    pub copy_ms: f32,
    pub histogram: [f32; 48],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Uniforms {
    rows: [[f32; 4]; 4],
}

type Slot = Arc<(Mutex<Option<Job>>, Condvar)>;

pub struct Renderer {
    slot: Slot,
}

impl Renderer {
    /// Starts the render thread. Returns once the adapter is up, with its description.
    pub fn spawn(
        width: u32,
        height: u32,
        on_frame: impl Fn(Rendered) + Send + 'static,
    ) -> Result<(Self, String), String> {
        let slot: Slot = Arc::new((Mutex::new(None), Condvar::new()));
        let (ready_tx, ready_rx) = mpsc::channel();
        let thread_slot = slot.clone();
        thread::Builder::new()
            .name("laika-develop".into())
            .spawn(move || match Gpu::new(width, height) {
                Ok(gpu) => {
                    ready_tx.send(Ok(gpu.adapter_info.clone())).ok();
                    gpu.run(thread_slot, on_frame);
                }
                Err(e) => {
                    ready_tx.send(Err(e)).ok();
                }
            })
            .map_err(|e| e.to_string())?;
        let info = ready_rx.recv().map_err(|e| e.to_string())??;
        Ok((Self { slot }, info))
    }

    /// Replaces any job the thread hasn't started. Dragging faster than the GPU just skips values.
    pub fn submit(&self, job: Job) {
        let (lock, cvar) = &*self.slot;
        *lock.lock().unwrap() = Some(job);
        cvar.notify_one();
    }
}

struct Gpu {
    device: wgpu::Device,
    queue: wgpu::Queue,
    pipeline: wgpu::RenderPipeline,
    bind_group: wgpu::BindGroup,
    uniforms: wgpu::Buffer,
    view: wgpu::TextureView,
    texture: wgpu::Texture,
    readback: wgpu::Buffer,
    width: u32,
    height: u32,
    padded_row: u32,
    adapter_info: String,
}

fn pipeline(
    device: &wgpu::Device,
    shader: &wgpu::ShaderModule,
    fragment: &str,
    format: wgpu::TextureFormat,
) -> wgpu::RenderPipeline {
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some(fragment),
        layout: None,
        vertex: wgpu::VertexState {
            module: shader,
            entry_point: Some("vs"),
            compilation_options: Default::default(),
            buffers: &[],
        },
        primitive: Default::default(),
        depth_stencil: None,
        multisample: Default::default(),
        fragment: Some(wgpu::FragmentState {
            module: shader,
            entry_point: Some(fragment),
            compilation_options: Default::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format,
                blend: None,
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        multiview_mask: None,
        cache: None,
    })
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
    fn new(width: u32, height: u32) -> Result<Self, String> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
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
        let source_format = wgpu::TextureFormat::Rgba16Float;
        let format = wgpu::TextureFormat::Bgra8Unorm;
        let scene_pipeline = pipeline(&device, &shader, "scene_fs", source_format);
        let develop_pipeline = pipeline(&device, &shader, "fs", format);

        let uniforms = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("uniforms"),
            size: std::mem::size_of::<Uniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let extent = wgpu::Extent3d { width, height, depth_or_array_layers: 1 };
        let texture_desc = |label, format, usage| wgpu::TextureDescriptor {
            label: Some(label),
            size: extent,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage,
            view_formats: &[],
        };

        // The stand-in for a decoded RAW, rendered once.
        let source = device.create_texture(&texture_desc(
            "source",
            source_format,
            wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
        ));
        let source_view = source.create_view(&Default::default());
        let scene_bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &scene_pipeline.get_bind_group_layout(0),
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: uniforms.as_entire_binding() }],
        });
        let mut u = Uniforms::zeroed();
        u.rows[3] = [0., 0., width as f32, height as f32];
        queue.write_buffer(&uniforms, 0, bytemuck::bytes_of(&u));
        let mut encoder = device.create_command_encoder(&Default::default());
        fullscreen_pass(&mut encoder, &source_view, &scene_pipeline, &scene_bind);
        queue.submit([encoder.finish()]);
        device.poll(wgpu::PollType::wait_indefinitely()).map_err(|e| e.to_string())?;

        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &develop_pipeline.get_bind_group_layout(0),
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: uniforms.as_entire_binding() },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&source_view),
                },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::Sampler(&sampler) },
            ],
        });

        let texture = device.create_texture(&texture_desc(
            "preview",
            format,
            wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        ));
        let view = texture.create_view(&Default::default());

        let align = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
        let padded_row = (width * 4).div_ceil(align) * align;
        let readback = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("readback"),
            size: (padded_row * height) as u64,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        Ok(Self {
            device,
            queue,
            pipeline: develop_pipeline,
            bind_group,
            uniforms,
            view,
            texture,
            readback,
            width,
            height,
            padded_row,
            adapter_info,
        })
    }

    fn run(self, slot: Slot, on_frame: impl Fn(Rendered)) {
        let started = Instant::now();
        loop {
            let job = {
                let (lock, cvar) = &*slot;
                let mut guard = lock.lock().unwrap();
                while guard.is_none() {
                    guard = cvar.wait(guard).unwrap();
                }
                guard.take().unwrap()
            };
            let frame = self.render(&job, started.elapsed().as_secs_f32());
            on_frame(frame);
        }
    }

    fn render(&self, job: &Job, time: f32) -> Rendered {
        let t0 = Instant::now();
        let v = job.values;
        let u = Uniforms {
            rows: [
                [v[0], v[1], v[2], v[3]],
                [v[4], v[5], v[6], v[7]],
                [v[8], v[9], v[10], v[11]],
                [job.split, time, self.width as f32, self.height as f32],
            ],
        };
        self.queue.write_buffer(&self.uniforms, 0, bytemuck::bytes_of(&u));

        let mut encoder = self.device.create_command_encoder(&Default::default());
        fullscreen_pass(&mut encoder, &self.view, &self.pipeline, &self.bind_group);
        encoder.copy_texture_to_buffer(
            self.texture.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &self.readback,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(self.padded_row),
                    rows_per_image: None,
                },
            },
            wgpu::Extent3d { width: self.width, height: self.height, depth_or_array_layers: 1 },
        );
        self.queue.submit([encoder.finish()]);

        let slice = self.readback.slice(..);
        slice.map_async(wgpu::MapMode::Read, |r| r.expect("map readback"));
        self.device.poll(wgpu::PollType::wait_indefinitely()).expect("poll");
        let t1 = Instant::now();

        let (w, h, row) = (self.width as usize, self.height as usize, self.padded_row as usize);
        let mut bgra = Vec::with_capacity(w * h * 4);
        let mut hist = [0f32; 48];
        {
            let mapped = slice.get_mapped_range();
            for y in 0..h {
                let line = &mapped[y * row..y * row + w * 4];
                bgra.extend_from_slice(line);
                if y % 6 == 0 {
                    for px in line.chunks_exact(4).step_by(6) {
                        let l = 0.0722 * px[0] as f32 + 0.7152 * px[1] as f32 + 0.2126 * px[2] as f32;
                        hist[((l / 256.) * 48.) as usize] += 1.;
                    }
                }
            }
        }
        self.readback.unmap();
        let max = hist.iter().cloned().fold(1., f32::max);
        for b in &mut hist {
            *b = (*b / max).sqrt();
        }

        Rendered {
            bgra,
            width: self.width,
            height: self.height,
            seq: job.seq,
            gpu_ms: t1.duration_since(t0).as_secs_f32() * 1000.,
            copy_ms: t1.elapsed().as_secs_f32() * 1000.,
            histogram: hist,
        }
    }
}
