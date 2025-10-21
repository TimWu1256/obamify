/// Headless GPU渲染模組
/// 不依賴 egui，可獨立使用 wgpu 進行 Voronoi diagram 計算和渲染

use bytemuck::{Pod, Zeroable};
use wgpu::util::DeviceExt;

// 使用共享類型
pub use crate::types::{SeedPos, SeedColor};

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct ParamsCommon {
    width: u32,
    height: u32,
    n_seeds: u32,
    _pad: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct ParamsJfa {
    width: u32,
    height: u32,
    step: u32,
    _pad: u32,
}

#[allow(dead_code)]
pub struct HeadlessRenderer {
    device: wgpu::Device,
    queue: wgpu::Queue,

    size: (u32, u32),
    seed_count: u32,

    // Buffers
    seed_buf: wgpu::Buffer,
    color_buf: wgpu::Buffer,
    params_common_buf: wgpu::Buffer,
    params_jfa_buf: wgpu::Buffer,

    // Textures
    seed_tex: wgpu::Texture,
    seed_tex_view: wgpu::TextureView,
    color_lookup_tex: wgpu::Texture,
    color_lookup_tex_view: wgpu::TextureView,

    ids_a: wgpu::Texture,
    ids_b: wgpu::Texture,
    ids_a_view: wgpu::TextureView,
    ids_b_view: wgpu::TextureView,

    color_tex: wgpu::Texture,
    color_view: wgpu::TextureView,

    // Pipelines
    clear_pipeline: wgpu::RenderPipeline,
    seed_splat_pipeline: wgpu::RenderPipeline,
    jfa_pipeline: wgpu::RenderPipeline,
    shade_pipeline: wgpu::RenderPipeline,

    // Bind group layouts
    clear_bgl: wgpu::BindGroupLayout,
    seed_bgl: wgpu::BindGroupLayout,
    jfa_bgl: wgpu::BindGroupLayout,
    shade_bgl: wgpu::BindGroupLayout,

    // Sampler
    nearest_sampler: wgpu::Sampler,

    // Bind groups
    clear_bg_a: wgpu::BindGroup,
    clear_bg_b: wgpu::BindGroup,
    seed_bg: wgpu::BindGroup,
    jfa_bg_a_to_b: wgpu::BindGroup,
    jfa_bg_b_to_a: wgpu::BindGroup,
    shade_bg: wgpu::BindGroup,
}

impl HeadlessRenderer {
    pub async fn new(size: (u32, u32), seed_count: u32, seeds: Vec<SeedPos>, colors: Vec<SeedColor>) -> Self {
        // 初始化 wgpu instance (headless)
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
            backends: wgpu::Backends::all(),
            ..Default::default()
        });

        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: None,
                force_fallback_adapter: false,
            })
            .await
            .expect("Failed to find an appropriate adapter");

        let (device, queue) = adapter
            .request_device(
                &wgpu::DeviceDescriptor {
                    label: Some("headless_device"),
                    required_features: wgpu::Features::empty(),
                    required_limits: wgpu::Limits::default(),
                    memory_hints: wgpu::MemoryHints::default(),
                    trace: Default::default(),
                },
            )
            .await
            .expect("Failed to create device");

        // 建立 buffers
        let seed_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("seeds"),
            contents: bytemuck::cast_slice(&seeds),
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
        });

        let color_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("colors"),
            contents: bytemuck::cast_slice(&colors),
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
        });

        let (seed_tex, seed_tex_view) = Self::make_seed_texture(&device, &queue, &seeds, seed_count);
        let (color_lookup_tex, color_lookup_tex_view) = Self::make_color_lookup_texture(&device, &queue, &colors, seed_count);

        let params_common = ParamsCommon {
            width: size.0,
            height: size.1,
            n_seeds: seed_count,
            _pad: 0,
        };
        let params_common_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("params_common"),
            contents: bytemuck::bytes_of(&params_common),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });

        let params_jfa = ParamsJfa {
            width: size.0,
            height: size.1,
            step: 1,
            _pad: 0,
        };
        let params_jfa_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("params_jfa"),
            contents: bytemuck::bytes_of(&params_jfa),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });

        // 建立 textures
        let (ids_a, ids_a_view) = Self::make_ids_texture(&device, size, Some("ids_a"));
        let (ids_b, ids_b_view) = Self::make_ids_texture(&device, size, Some("ids_b"));
        let (color_tex, color_view) = Self::make_color_texture(&device, size, Some("color"));

        // 建立 pipelines
        let clear_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("bgl_clear"),
            entries: &[],
        });

        let seed_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("bgl_seed_splat"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: false },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::VERTEX,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: std::num::NonZeroU64::new(std::mem::size_of::<ParamsCommon>() as u64),
                    },
                    count: None,
                },
            ],
        });

        let jfa_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("bgl_jfa"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: false },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: false },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::NonFiltering),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: std::num::NonZeroU64::new(std::mem::size_of::<ParamsJfa>() as u64),
                    },
                    count: None,
                },
            ],
        });

        let shade_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("bgl_shade"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: false },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::NonFiltering),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: false },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: false },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 4,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: std::num::NonZeroU64::new(std::mem::size_of::<ParamsCommon>() as u64),
                    },
                    count: None,
                },
            ],
        });

        let nearest_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("nearest_sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            mipmap_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });

        // Shader modules
        let clear_sm = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("clear.wgsl"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders/clear.wgsl").into()),
        });
        let seed_sm = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("seed_splat.wgsl"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders/seed.wgsl").into()),
        });
        let jfa_sm = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("jfa.wgsl"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders/jfa.wgsl").into()),
        });
        let shade_sm = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("shade.wgsl"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders/shade.wgsl").into()),
        });

        // Create pipelines
        let clear_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("clear_pipeline"),
            layout: Some(&device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("pl_clear"),
                bind_group_layouts: &[&clear_bgl],
                push_constant_ranges: &[],
            })),
            vertex: wgpu::VertexState {
                module: &clear_sm,
                entry_point: Some("vs_main"),
                buffers: &[],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &clear_sm,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: wgpu::TextureFormat::Rgba8Unorm,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleStrip,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        let seed_splat_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("seed_splat_pipeline"),
            layout: Some(&device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("pl_seed"),
                bind_group_layouts: &[&seed_bgl],
                push_constant_ranges: &[],
            })),
            vertex: wgpu::VertexState {
                module: &seed_sm,
                entry_point: Some("vs_main"),
                buffers: &[],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &seed_sm,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: wgpu::TextureFormat::Rgba8Unorm,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::PointList,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        let jfa_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("jfa_pipeline"),
            layout: Some(&device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("pl_jfa"),
                bind_group_layouts: &[&jfa_bgl],
                push_constant_ranges: &[],
            })),
            vertex: wgpu::VertexState {
                module: &jfa_sm,
                entry_point: Some("vs_main"),
                buffers: &[],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &jfa_sm,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: wgpu::TextureFormat::Rgba8Unorm,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleStrip,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        let shade_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("shade_pipeline"),
            layout: Some(&device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("pl_shade"),
                bind_group_layouts: &[&shade_bgl],
                push_constant_ranges: &[],
            })),
            vertex: wgpu::VertexState {
                module: &shade_sm,
                entry_point: Some("vs_main"),
                buffers: &[],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shade_sm,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: wgpu::TextureFormat::Rgba8Unorm,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleStrip,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        // Create bind groups
        let clear_bg_a = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("bg_clear_a"),
            layout: &clear_bgl,
            entries: &[],
        });
        let clear_bg_b = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("bg_clear_b"),
            layout: &clear_bgl,
            entries: &[],
        });

        let seed_bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("bg_seed_splat"),
            layout: &seed_bgl,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&seed_tex_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: params_common_buf.as_entire_binding(),
                },
            ],
        });

        let jfa_bg_a_to_b = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("bg_jfa_a_to_b"),
            layout: &jfa_bgl,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&seed_tex_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&ids_a_view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(&nearest_sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: params_jfa_buf.as_entire_binding(),
                },
            ],
        });

        let jfa_bg_b_to_a = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("bg_jfa_b_to_a"),
            layout: &jfa_bgl,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&seed_tex_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&ids_b_view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(&nearest_sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: params_jfa_buf.as_entire_binding(),
                },
            ],
        });

        let shade_bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("bg_shade"),
            layout: &shade_bgl,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&ids_a_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&nearest_sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(&seed_tex_view),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::TextureView(&color_lookup_tex_view),
                },
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: params_common_buf.as_entire_binding(),
                },
            ],
        });

        Self {
            device,
            queue,
            size,
            seed_count,
            seed_buf,
            color_buf,
            params_common_buf,
            params_jfa_buf,
            seed_tex,
            seed_tex_view,
            color_lookup_tex,
            color_lookup_tex_view,
            ids_a,
            ids_b,
            ids_a_view,
            ids_b_view,
            color_tex,
            color_view,
            clear_pipeline,
            seed_splat_pipeline,
            jfa_pipeline,
            shade_pipeline,
            clear_bgl,
            seed_bgl,
            jfa_bgl,
            shade_bgl,
            nearest_sampler,
            clear_bg_a,
            clear_bg_b,
            seed_bg,
            jfa_bg_a_to_b,
            jfa_bg_b_to_a,
            shade_bg,
        }
    }

    fn make_ids_texture(device: &wgpu::Device, size: (u32, u32), label: Option<&str>) -> (wgpu::Texture, wgpu::TextureView) {
        let tex = device.create_texture(&wgpu::TextureDescriptor {
            label,
            size: wgpu::Extent3d {
                width: size.0,
                height: size.1,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        let view = tex.create_view(&wgpu::TextureViewDescriptor::default());
        (tex, view)
    }

    fn make_color_texture(device: &wgpu::Device, size: (u32, u32), label: Option<&str>) -> (wgpu::Texture, wgpu::TextureView) {
        let tex = device.create_texture(&wgpu::TextureDescriptor {
            label,
            size: wgpu::Extent3d {
                width: size.0,
                height: size.1,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                | wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::COPY_SRC
                | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let view = tex.create_view(&wgpu::TextureViewDescriptor::default());
        (tex, view)
    }

    fn make_seed_texture(device: &wgpu::Device, queue: &wgpu::Queue, seeds: &[SeedPos], max_seeds: u32) -> (wgpu::Texture, wgpu::TextureView) {
        const TEX_WIDTH: u32 = 1024;
        let tex_height = max_seeds.div_ceil(TEX_WIDTH);

        let tex = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("seed_positions"),
            size: wgpu::Extent3d {
                width: TEX_WIDTH,
                height: tex_height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rg32Float,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });

        let mut data = vec![0.0f32; (TEX_WIDTH * tex_height * 2) as usize];
        for (i, seed) in seeds.iter().enumerate() {
            data[i * 2] = seed.xy[0];
            data[i * 2 + 1] = seed.xy[1];
        }

        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &tex,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            bytemuck::cast_slice(&data),
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(TEX_WIDTH * 8),
                rows_per_image: Some(tex_height),
            },
            wgpu::Extent3d {
                width: TEX_WIDTH,
                height: tex_height,
                depth_or_array_layers: 1,
            },
        );

        let view = tex.create_view(&wgpu::TextureViewDescriptor::default());
        (tex, view)
    }

    fn make_color_lookup_texture(device: &wgpu::Device, queue: &wgpu::Queue, colors: &[SeedColor], max_seeds: u32) -> (wgpu::Texture, wgpu::TextureView) {
        const TEX_WIDTH: u32 = 1024;
        let tex_height = max_seeds.div_ceil(TEX_WIDTH);

        let tex = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("color_lookup"),
            size: wgpu::Extent3d {
                width: TEX_WIDTH,
                height: tex_height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba32Float,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });

        let mut data = vec![0.0f32; (TEX_WIDTH * tex_height * 4) as usize];
        for (i, color) in colors.iter().enumerate() {
            data[i * 4] = color.rgba[0];
            data[i * 4 + 1] = color.rgba[1];
            data[i * 4 + 2] = color.rgba[2];
            data[i * 4 + 3] = color.rgba[3];
        }

        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &tex,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            bytemuck::cast_slice(&data),
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(TEX_WIDTH * 16),
                rows_per_image: Some(tex_height),
            },
            wgpu::Extent3d {
                width: TEX_WIDTH,
                height: tex_height,
                depth_or_array_layers: 1,
            },
        );

        let view = tex.create_view(&wgpu::TextureViewDescriptor::default());
        (tex, view)
    }

    /// 更新 seed 位置
    pub fn update_seeds(&mut self, seeds: &[SeedPos]) {
        const TEX_WIDTH: u32 = 1024;
        let tex_height = self.seed_count.div_ceil(TEX_WIDTH);

        let mut data = vec![0.0f32; (TEX_WIDTH * tex_height * 2) as usize];
        for (i, seed) in seeds.iter().enumerate() {
            data[i * 2] = seed.xy[0];
            data[i * 2 + 1] = seed.xy[1];
        }

        self.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &self.seed_tex,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            bytemuck::cast_slice(&data),
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(TEX_WIDTH * 8),
                rows_per_image: Some(tex_height),
            },
            wgpu::Extent3d {
                width: TEX_WIDTH,
                height: tex_height,
                depth_or_array_layers: 1,
            },
        );
    }

    /// 更新顏色
    pub fn update_colors(&mut self, colors: &[SeedColor]) {
        const TEX_WIDTH: u32 = 1024;
        let tex_height = self.seed_count.div_ceil(TEX_WIDTH);

        let mut data = vec![0.0f32; (TEX_WIDTH * tex_height * 4) as usize];
        for (i, color) in colors.iter().enumerate() {
            data[i * 4] = color.rgba[0];
            data[i * 4 + 1] = color.rgba[1];
            data[i * 4 + 2] = color.rgba[2];
            data[i * 4 + 3] = color.rgba[3];
        }

        self.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &self.color_lookup_tex,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            bytemuck::cast_slice(&data),
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(TEX_WIDTH * 16),
                rows_per_image: Some(tex_height),
            },
            wgpu::Extent3d {
                width: TEX_WIDTH,
                height: tex_height,
                depth_or_array_layers: 1,
            },
        );
    }

    /// 執行 GPU 渲染並回傳 RGBA 像素資料
    pub fn render(&mut self) -> Vec<u8> {
        let mut encoder = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("voronoi_jfa_encoder"),
        });

        // 1) Clear ID texture A
        {
            let mut rpass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("clear_ids_a"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &self.ids_a_view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::WHITE),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            rpass.set_pipeline(&self.clear_pipeline);
            rpass.set_bind_group(0, &self.clear_bg_a, &[]);
            rpass.draw(0..4, 0..1);
        }

        // 2) Seed splat into A
        {
            let mut rpass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("seed_splat"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &self.ids_a_view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            rpass.set_pipeline(&self.seed_splat_pipeline);
            rpass.set_bind_group(0, &self.seed_bg, &[]);
            rpass.draw(0..self.seed_count, 0..1);
        }

        // 3) JFA passes
        let max_dim = self.size.0.max(self.size.1);
        let mut step = 1u32;
        while step < max_dim {
            step <<= 1;
        }
        step >>= 1;

        let mut flip = false;
        let mut is_first_jfa_pass = true;
        while step >= 1 {
            let pj = ParamsJfa {
                width: self.size.0,
                height: self.size.1,
                step,
                _pad: 0,
            };
            let staging = self.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("params_jfa_staging"),
                contents: bytemuck::bytes_of(&pj),
                usage: wgpu::BufferUsages::COPY_SRC,
            });
            encoder.copy_buffer_to_buffer(
                &staging,
                0,
                &self.params_jfa_buf,
                0,
                std::mem::size_of::<ParamsJfa>() as u64,
            );
            {
                let load_op = if is_first_jfa_pass && !flip {
                    wgpu::LoadOp::Clear(wgpu::Color::WHITE)
                } else {
                    wgpu::LoadOp::Load
                };

                let mut rpass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("jfa_step"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: if !flip { &self.ids_b_view } else { &self.ids_a_view },
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: load_op,
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                });
                rpass.set_pipeline(&self.jfa_pipeline);
                rpass.set_bind_group(0, if !flip { &self.jfa_bg_a_to_b } else { &self.jfa_bg_b_to_a }, &[]);
                rpass.draw(0..4, 0..1);
            }
            is_first_jfa_pass = false;
            flip = !flip;
            step >>= 1;
        }

        // 4) Shade to color
        let shade_with_b = flip;
        if shade_with_b {
            let tmp_bg = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("bg_shade_tmp_b"),
                layout: &self.shade_bgl,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(&self.ids_b_view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::Sampler(&self.nearest_sampler),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: wgpu::BindingResource::TextureView(&self.seed_tex_view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 3,
                        resource: wgpu::BindingResource::TextureView(&self.color_lookup_tex_view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 4,
                        resource: self.params_common_buf.as_entire_binding(),
                    },
                ],
            });
            let mut rpass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("shade"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &self.color_view,
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
            rpass.set_pipeline(&self.shade_pipeline);
            rpass.set_bind_group(0, &tmp_bg, &[]);
            rpass.draw(0..4, 0..1);
        } else {
            let mut rpass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("shade"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &self.color_view,
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
            rpass.set_pipeline(&self.shade_pipeline);
            rpass.set_bind_group(0, &self.shade_bg, &[]);
            rpass.draw(0..4, 0..1);
        }

        // 5) Copy texture to buffer for readback
        let width = self.size.0;
        let height = self.size.1;
        let bpp = 4u32;
        let unpadded_bytes_per_row = width * bpp;
        let align = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
        let padded_bytes_per_row = unpadded_bytes_per_row.div_ceil(align) * align;
        let buffer_size = padded_bytes_per_row as u64 * height as u64;

        let readback = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("color readback"),
            size: buffer_size,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: &self.color_tex,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &readback,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(padded_bytes_per_row),
                    rows_per_image: Some(height),
                },
            },
            wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );

        self.queue.submit(Some(encoder.finish()));

        // Map buffer and read pixels
        let slice = readback.slice(..);
        let (tx, rx) = futures_intrusive::channel::shared::oneshot_channel();
        slice.map_async(wgpu::MapMode::Read, move |res| {
            let _ = tx.send(res);
        });

        self.device.poll(wgpu::PollType::Wait).expect("Failed to poll device");
        pollster::block_on(rx.receive()).expect("map_async sender dropped").expect("Failed to map buffer");

        let mapped = slice.get_mapped_range();
        let mut rgba = Vec::with_capacity((width * height * 4) as usize);
        for y in 0..height as usize {
            let start = y * padded_bytes_per_row as usize;
            let end = start + unpadded_bytes_per_row as usize;
            rgba.extend_from_slice(&mapped[start..end]);
        }
        drop(mapped);
        readback.unmap();

        rgba
    }
}
