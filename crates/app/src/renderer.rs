use std::sync::Arc;

use aperion_logger::{LogOptions, log_as};

use crate::window;
use crate::intro::IntroPlayer;

use winit::dpi::PhysicalSize;
use winit::window::Window;

pub struct Renderer {
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    size: PhysicalSize<u32>,
    intro: Option<IntroRendererState>,
}

impl Renderer {
    /// creates the renderer. if there is an intro registered, it plays it
    pub async fn new(window: Arc<Window>) -> Self {
        let log = log_as(Some("RENDERER"), LogOptions::default());
        let log_verbose = log_as(Some("RENDERER"), LogOptions { verbose_only: true });

        let size = window.inner_size();

        let instance = wgpu::Instance::default();

        let surface = instance
            .create_surface(window)
            .expect("failed to create surface");
        log_verbose("surface created");

        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: Some(&surface),
                force_fallback_adapter: false,
            })
            .await
            .expect("failed to find a suitable GPU adapter");

        let adapter_info = adapter.get_info();
        log(&format!("found GPU {}", adapter_info.name));
        log(&format!("using {:?} backend", adapter_info.backend));
        log_verbose(&format!("vendor id: {}", adapter_info.vendor));
        log_verbose(&format!("device id: {}", adapter_info.device));
        let (device, queue) = adapter
            .request_device(
                &wgpu::DeviceDescriptor {
                    label: Some("aperion device"),
                    required_features: wgpu::Features::empty(),
                    required_limits: wgpu::Limits::default(),
                },
                None,
            )
            .await
            .expect("failed to create device");
        log_verbose("device and queue created");

        let surface_caps = surface.get_capabilities(&adapter);

        let surface_format = surface_caps
            .formats
            .iter()
            .copied()
            .find(|format| format.is_srgb())
            .unwrap_or(surface_caps.formats[0]);

        let present_mode = surface_caps
            .present_modes
            .iter()
            .copied()
            .find(|mode| *mode == wgpu::PresentMode::Fifo)
            .unwrap_or(surface_caps.present_modes[0]);

        let alpha_mode = surface_caps.alpha_modes[0];

        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format: surface_format,
            width: size.width.max(1),
            height: size.height.max(1),
            present_mode,
            alpha_mode,
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
        };

        surface.configure(&device, &config);
        log_verbose("surface configured");

        let intro = match window::intro_asset_path() {
            Some(path) if path.exists() => {
                match IntroRendererState::new(&device, &queue, surface_format, &path) {
                    Ok(state) => Some(state),
                    Err(err) => {
                        log(&format!("intro load failed: {err}"));
                        None
                    }
                }
            }
            Some(path) => {
                log_verbose(&format!("intro asset not found at {}", path.display()));
                None
            }
            None => {
                log_verbose("intro playback disabled by window config");
                None
            }
        };

        Self {
            surface,
            device,
            queue,
            config,
            size,
            intro,
        }
    }

    pub fn resize(&mut self, new_size: PhysicalSize<u32>) {
        let log_verbose = log_as(Some("RENDERER"), LogOptions { verbose_only: true });
        self.size = new_size;

        if new_size.width == 0 || new_size.height == 0 {
            log_verbose("resize skipped because one dimension is zero");
            return;
        }

        self.config.width = new_size.width;
        self.config.height = new_size.height;
        self.surface.configure(&self.device, &self.config);
        log_verbose(&format!(
            "resized surface to {}x{}",
            new_size.width, new_size.height
        ));
    }

    /// render the current frame
    pub fn render(&mut self) -> Result<(), wgpu::SurfaceError> {
        if self.config.width == 0 || self.config.height == 0 {
            return Ok(());
        }

        let output = self.surface.get_current_texture()?;

        let view = output
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("render encoder"),
            });

        let played_intro = if let Some(intro) = self.intro.as_mut() {
            match intro.render(&self.queue, &mut encoder, &view, self.size) {
                Ok(true) => true,
                Ok(false) => {
                    self.intro = None;
                    false
                }
                Err(err) => {
                    let log = log_as(Some("RENDERER"), LogOptions::default());
                    log(&format!("intro render failed: {err}"));
                    self.intro = None;
                    false
                }
            }
        } else {
            false
        };

        if !played_intro {
            let _render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("clear pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: 0.02,
                            g: 0.02,
                            b: 0.05,
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
        }

        self.queue.submit(std::iter::once(encoder.finish()));
        output.present();

        Ok(())
    }
}

struct IntroRendererState {
    player: IntroPlayer,
    pipeline: wgpu::RenderPipeline,
    bind_group: wgpu::BindGroup,
    texture: wgpu::Texture,
    transform_buffer: wgpu::Buffer,
    current_frame_index: Option<usize>,
}

impl IntroRendererState {
    fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        surface_format: wgpu::TextureFormat,
        intro_path: &std::path::Path,
    ) -> Result<Self, String> {
        let player = IntroPlayer::load_from_path(intro_path).map_err(|err| err.to_string())?;
        let first_frame = player
            .first_frame()
            .cloned()
            .ok_or_else(|| "intro contained no frames".to_string())?;

        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("intro texture"),
            size: wgpu::Extent3d {
                width: first_frame.width,
                height: first_frame.height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });

        queue.write_texture(
            wgpu::ImageCopyTexture {
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &first_frame.rgba,
            wgpu::ImageDataLayout {
                offset: 0,
                bytes_per_row: Some(first_frame.width * 4),
                rows_per_image: Some(first_frame.height),
            },
            wgpu::Extent3d {
                width: first_frame.width,
                height: first_frame.height,
                depth_or_array_layers: 1,
            },
        );

        let texture_view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("intro sampler"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });
        let transform_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("intro transform buffer"),
            size: std::mem::size_of::<[f32; 4]>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("intro bind group layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::VERTEX,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });

        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("intro bind group"),
            layout: &bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&texture_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: transform_buffer.as_entire_binding(),
                },
            ],
        });

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("intro shader"),
            source: wgpu::ShaderSource::Wgsl(INTRO_SHADER.into()),
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("intro pipeline layout"),
            bind_group_layouts: &[&bind_group_layout],
            push_constant_ranges: &[],
        });

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("intro pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: "vs_main",
                buffers: &[],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: "fs_main",
                targets: &[Some(wgpu::ColorTargetState {
                    format: surface_format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
        });

        Ok(Self {
            player,
            pipeline,
            bind_group,
            texture,
            transform_buffer,
            current_frame_index: None,
        })
    }

    fn render(
        &mut self,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        view: &wgpu::TextureView,
        surface_size: PhysicalSize<u32>,
    ) -> Result<bool, String> {
        if !self.player.is_started() {
            self.player.start().map_err(|e| e.to_string())?;
        }
            
        if self.player.is_finished() {
            self.player.stop_audio();
            
            return Ok(false);
        }

        let Some((frame_index, frame)) = self.player.frame_to_present() else {
            return Ok(false);
        };

        let scale = intro_scale(
            frame.width,
            frame.height,
            surface_size.width,
            surface_size.height,
        );
        let scale_data = [scale.0, scale.1, 0.0f32, 0.0f32];
        queue.write_buffer(&self.transform_buffer, 0, bytemuck::cast_slice(&scale_data));

        if self.current_frame_index != Some(frame_index) {
            queue.write_texture(
                wgpu::ImageCopyTexture {
                    texture: &self.texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                &frame.rgba,
                wgpu::ImageDataLayout {
                    offset: 0,
                    bytes_per_row: Some(frame.width * 4),
                    rows_per_image: Some(frame.height),
                },
                wgpu::Extent3d {
                    width: frame.width,
                    height: frame.height,
                    depth_or_array_layers: 1,
                },
            );

            self.current_frame_index = Some(frame_index);
        }

        let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("intro pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
        });

        render_pass.set_pipeline(&self.pipeline);
        render_pass.set_bind_group(0, &self.bind_group, &[]);
        render_pass.draw(0..6, 0..1);

        Ok(true)
    }
}

fn intro_scale(
    video_width: u32,
    video_height: u32,
    surface_width: u32,
    surface_height: u32,
) -> (f32, f32) {
    let video_aspect = video_width as f32 / video_height as f32;
    let surface_aspect = surface_width.max(1) as f32 / surface_height.max(1) as f32;

    if video_aspect > surface_aspect {
        (1.0, surface_aspect / video_aspect)
    } else {
        (video_aspect / surface_aspect, 1.0)
    }
}

const INTRO_SHADER: &str = r#"
@group(0) @binding(0)
var intro_texture: texture_2d<f32>;

@group(0) @binding(1)
var intro_sampler: sampler;

struct IntroTransform {
    scale: vec2<f32>,
    _padding: vec2<f32>,
}

@group(0) @binding(2)
var<uniform> intro_transform: IntroTransform;

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
}

@vertex
fn vs_main(@builtin(vertex_index) vertex_index: u32) -> VertexOutput {
    var positions = array<vec2<f32>, 6>(
        vec2<f32>(-1.0, -1.0),
        vec2<f32>( 1.0, -1.0),
        vec2<f32>( 1.0,  1.0),
        vec2<f32>(-1.0, -1.0),
        vec2<f32>( 1.0,  1.0),
        vec2<f32>(-1.0,  1.0),
    );

    var uvs = array<vec2<f32>, 6>(
        vec2<f32>(0.0, 1.0),
        vec2<f32>(1.0, 1.0),
        vec2<f32>(1.0, 0.0),
        vec2<f32>(0.0, 1.0),
        vec2<f32>(1.0, 0.0),
        vec2<f32>(0.0, 0.0),
    );

    var out: VertexOutput;
    out.position = vec4<f32>(positions[vertex_index] * intro_transform.scale, 0.0, 1.0);
    out.uv = uvs[vertex_index];
    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    return textureSample(intro_texture, intro_sampler, in.uv);
}
"#;
