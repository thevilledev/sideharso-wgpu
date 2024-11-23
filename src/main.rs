use cgmath::*;
use std::iter;
use wgpu::util::DeviceExt;
use winit::{
    event::*,
    event_loop::{ControlFlow, EventLoop},
    window::{Window, WindowBuilder},
};

// Vertex structure for our grid points
#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
struct Vertex {
    position: [f32; 3],
}

#[repr(C)]
#[derive(Debug, Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
struct Uniforms {
    time: f32,
    _padding: [f32; 3], // Padding to satisfy alignment requirements
    view_proj: [[f32; 4]; 4],
}

impl Uniforms {
    fn new() -> Self {
        let perspective = perspective(Deg(45.0), 800.0 / 600.0, 0.1, 100.0);
        let view = Matrix4::look_at_rh(
            Point3::new(0.0, 0.5, -5.0),    // Camera position
            Point3::new(0.0, 0.5, 0.0),  // Looking straight ahead
            Vector3::unit_y(),                       // Up vector
        );

        Self {
            time: 0.0,
            _padding: [0.0; 3],
            view_proj: (perspective * view).into(),
        }
    }

    fn update(&mut self, time: f32) {
        self.time = time;
    }
}

struct State {
    surface: wgpu::Surface,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    size: winit::dpi::PhysicalSize<u32>,
    render_pipeline: wgpu::RenderPipeline,
    vertex_buffer: wgpu::Buffer,
    num_vertices: u32,
    time: f32,
    uniform_buffer: wgpu::Buffer,
    uniform_bind_group: wgpu::BindGroup,
    depth_texture: wgpu::TextureView,
    camera_position: Point3<f32>,
    camera_rotation: f32,
}

impl State {
    async fn new(window: &Window) -> Self {
        let size = window.inner_size();
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::all(),
            dx12_shader_compiler: Default::default(),
        });

        let surface = unsafe { instance.create_surface(&window) }.unwrap();
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::default(),
                compatible_surface: Some(&surface),
                force_fallback_adapter: false,
            })
            .await
            .unwrap();

        let (device, queue) = adapter
            .request_device(
                &wgpu::DeviceDescriptor {
                    label: None,
                    features: wgpu::Features::POLYGON_MODE_LINE,
                    limits: wgpu::Limits::default(),
                },
                None,
            )
            .await
            .unwrap();

        let surface_caps = surface.get_capabilities(&adapter);
        let surface_format = surface_caps
            .formats
            .iter()
            .copied()
            .find(|f| f.is_srgb())
            .unwrap_or(surface_caps.formats[0]);

        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format: surface_format,
            width: size.width,
            height: size.height,
            present_mode: surface_caps.present_modes[0],
            alpha_mode: surface_caps.alpha_modes[0],
            view_formats: vec![],
        };
        surface.configure(&device, &config);

        // Create vertices for the grid
        let (vertices, num_vertices) = create_grid(80, 60);
        let vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Vertex Buffer"),
            contents: bytemuck::cast_slice(&vertices),
            usage: wgpu::BufferUsages::VERTEX,
        });

        // Create the shader module (we'll add the actual GLSL shaders next)
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shader.wgsl").into()),
        });

        // Create uniform buffer and bind group layout
        let uniform_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("Uniform Bind Group Layout"),
                entries: &[wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                }],
            });

        let uniforms = Uniforms::new();
        let uniform_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Uniform Buffer"),
            contents: bytemuck::cast_slice(&[uniforms]),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });

        let uniform_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Uniform Bind Group"),
            layout: &uniform_bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform_buffer.as_entire_binding(),
            }],
        });

        // Create the render pipeline (pass the uniform_bind_group_layout)
        let render_pipeline =
            create_render_pipeline(&device, &shader, &config, &uniform_bind_group_layout);

        // Create depth texture
        let depth_texture = create_depth_texture(&device, &config);

        Self {
            surface,
            device,
            queue,
            config,
            size,
            render_pipeline,
            vertex_buffer,
            num_vertices: num_vertices as u32,
            time: 0.0,
            uniform_buffer,
            uniform_bind_group,
            depth_texture,
            camera_position: Point3::new(0.0, 0.5, -5.0),
            camera_rotation: 0.0,
        }
    }

    fn resize(&mut self, new_size: winit::dpi::PhysicalSize<u32>) {
        if new_size.width > 0 && new_size.height > 0 {
            self.size = new_size;
            self.config.width = new_size.width;
            self.config.height = new_size.height;
            self.surface.configure(&self.device, &self.config);

            // Recreate depth texture on resize
            self.depth_texture = create_depth_texture(&self.device, &self.config);

            // Update the uniform buffer with new aspect ratio
            let mut uniforms = Uniforms::new();
            uniforms.update(self.time);

            self.queue
                .write_buffer(&self.uniform_buffer, 0, bytemuck::cast_slice(&[uniforms]));
        }
    }

    fn input(&mut self, event: &WindowEvent) -> bool {
        match event {
            WindowEvent::KeyboardInput {
                input:
                    KeyboardInput {
                        state: ElementState::Pressed,
                        virtual_keycode: Some(keycode),
                        ..
                    },
                ..
            } => {
                let movement_speed = 0.1;
                let rotation_speed = 0.1;

                match keycode {
                    VirtualKeyCode::W => {
                        self.camera_position.z += movement_speed * self.camera_rotation.cos();
                        self.camera_position.x += movement_speed * self.camera_rotation.sin();
                        true
                    }
                    VirtualKeyCode::S => {
                        self.camera_position.z -= movement_speed * self.camera_rotation.cos();
                        self.camera_position.x -= movement_speed * self.camera_rotation.sin();
                        true
                    }
                    VirtualKeyCode::A => {
                        self.camera_rotation -= rotation_speed;
                        true
                    }
                    VirtualKeyCode::D => {
                        self.camera_rotation += rotation_speed;
                        true
                    }
                    VirtualKeyCode::Q => {
                        self.camera_position.y += movement_speed;
                        true
                    }
                    VirtualKeyCode::E => {
                        self.camera_position.y -= movement_speed;
                        true
                    }
                    _ => false,
                }
            }
            _ => false,
        }
    }

    fn update(&mut self) {
        self.time += 1.0 / 60.0;

        // Update camera view matrix
        let mut uniforms = Uniforms::new();
        uniforms.time = self.time;

        // Create view matrix from camera position and rotation
        let view = Matrix4::look_at_rh(
            self.camera_position,
            Point3::new(
                self.camera_position.x + self.camera_rotation.sin(),
                self.camera_position.y,
                self.camera_position.z + self.camera_rotation.cos(),
            ),
            Vector3::unit_y(),
        );

        let perspective = perspective(
            Deg(45.0),
            self.size.width as f32 / self.size.height as f32,
            0.1,
            100.0,
        );
        uniforms.view_proj = (perspective * view).into();

        self.queue
            .write_buffer(&self.uniform_buffer, 0, bytemuck::cast_slice(&[uniforms]));
    }

    fn render(&mut self) -> Result<(), wgpu::SurfaceError> {
        // Get the current texture view to render to
        let output = self.surface.get_current_texture()?;
        let view = output
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());

        // Create command encoder
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("Render Encoder"),
            });

        // Begin render pass
        {
            let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Render Pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: true,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &self.depth_texture,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.0),
                        store: true,
                    }),
                    stencil_ops: None,
                }),
            });

            // Set pipeline and vertex buffer
            render_pass.set_pipeline(&self.render_pipeline);
            render_pass.set_bind_group(0, &self.uniform_bind_group, &[]);
            render_pass.set_vertex_buffer(0, self.vertex_buffer.slice(..));

            // Draw the grid
            render_pass.draw(0..self.num_vertices, 0..1);
        }

        // Submit command buffer and present
        self.queue.submit(iter::once(encoder.finish()));
        output.present();

        Ok(())
    }
}

fn main() {
    env_logger::init();
    let event_loop = EventLoop::new();
    let window = WindowBuilder::new()
        .with_title("Waveform Visualization")
        .with_inner_size(winit::dpi::PhysicalSize::new(800, 600))
        .build(&event_loop)
        .unwrap();

    let mut state = pollster::block_on(State::new(&window));

    event_loop.run(move |event, _, control_flow| match event {
        Event::WindowEvent {
            ref event,
            window_id,
        } if window_id == window.id() => {
            if !state.input(event) {
                match event {
                    WindowEvent::CloseRequested
                    | WindowEvent::KeyboardInput {
                        input:
                            KeyboardInput {
                                state: ElementState::Pressed,
                                virtual_keycode: Some(VirtualKeyCode::Escape),
                                ..
                            },
                        ..
                    } => *control_flow = ControlFlow::Exit,
                    WindowEvent::Resized(physical_size) => {
                        state.resize(*physical_size);
                    }
                    WindowEvent::ScaleFactorChanged { new_inner_size, .. } => {
                        state.resize(**new_inner_size);
                    }
                    _ => {}
                }
            }
        }
        Event::RedrawRequested(window_id) if window_id == window.id() => {
            state.update();
            match state.render() {
                Ok(_) => {}
                Err(wgpu::SurfaceError::Lost) => state.resize(state.size),
                Err(wgpu::SurfaceError::OutOfMemory) => *control_flow = ControlFlow::Exit,
                Err(e) => eprintln!("{:?}", e),
            }
        }
        Event::MainEventsCleared => {
            window.request_redraw();
        }
        _ => {}
    });
}

fn create_grid(width: u32, depth: u32) -> (Vec<Vertex>, usize) {
    let mut vertices = Vec::new();
    let mut vertex_count = 0;

    // Create horizontal lines
    for z in 0..depth {
        let z_pos = (z as f32 * 2.0 / depth as f32) - 1.0;

        // Add vertices for each horizontal line
        for x in 0..width {
            let x_pos = (x as f32 * 2.0 / width as f32) - 1.0;
            vertices.push(Vertex {
                position: [x_pos, 0.0, z_pos],
            });
            vertex_count += 1;
        }

        // Add degenerate vertices to move to next line
        if z < depth - 1 {
            vertices.push(Vertex {
                position: [1.0, 0.0, z_pos], // Last vertex of current line
            });
            vertices.push(Vertex {
                position: [-1.0, 0.0, (z + 1) as f32 * 2.0 / depth as f32 - 1.0], // First vertex of next line
            });
            vertex_count += 2;
        }
    }

    // Create vertical lines
    for x in 0..width {
        let x_pos = (x as f32 * 2.0 / width as f32) - 1.0;

        // Add vertices for each vertical line
        for z in 0..depth {
            let z_pos = (z as f32 * 2.0 / depth as f32) - 1.0;
            vertices.push(Vertex {
                position: [x_pos, 0.0, z_pos],
            });
            vertex_count += 1;
        }

        // Add degenerate vertices to move to next line
        if x < width - 1 {
            vertices.push(Vertex {
                position: [x_pos, 0.0, 1.0], // Last vertex of current line
            });
            vertices.push(Vertex {
                position: [(x + 1) as f32 * 2.0 / width as f32 - 1.0, 0.0, -1.0], // First vertex of next line
            });
            vertex_count += 2;
        }
    }

    (vertices, vertex_count)
}

// Update the vertex buffer layout in create_render_pipeline
fn create_render_pipeline(
    device: &wgpu::Device,
    shader: &wgpu::ShaderModule,
    config: &wgpu::SurfaceConfiguration,
    uniform_bind_group_layout: &wgpu::BindGroupLayout,
) -> wgpu::RenderPipeline {
    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("Render Pipeline Layout"),
        bind_group_layouts: &[uniform_bind_group_layout],
        push_constant_ranges: &[],
    });

    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("Render Pipeline"),
        layout: Some(&pipeline_layout),
        vertex: wgpu::VertexState {
            module: shader,
            entry_point: "vs_main",
            buffers: &[wgpu::VertexBufferLayout {
                array_stride: std::mem::size_of::<Vertex>() as wgpu::BufferAddress,
                step_mode: wgpu::VertexStepMode::Vertex,
                attributes: &[wgpu::VertexAttribute {
                    offset: 0,
                    shader_location: 0,
                    format: wgpu::VertexFormat::Float32x3,
                }],
            }],
        },
        fragment: Some(wgpu::FragmentState {
            module: shader,
            entry_point: "fs_main",
            targets: &[Some(wgpu::ColorTargetState {
                format: config.format,
                blend: Some(wgpu::BlendState::REPLACE),
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        primitive: wgpu::PrimitiveState {
            topology: wgpu::PrimitiveTopology::LineStrip,
            strip_index_format: None,
            front_face: wgpu::FrontFace::Ccw,
            cull_mode: None,
            polygon_mode: wgpu::PolygonMode::Line,
            unclipped_depth: false,
            conservative: false,
        },
        depth_stencil: Some(wgpu::DepthStencilState {
            format: wgpu::TextureFormat::Depth32Float,
            depth_write_enabled: true,
            depth_compare: wgpu::CompareFunction::Less,
            stencil: wgpu::StencilState::default(),
            bias: wgpu::DepthBiasState::default(),
        }),
        multisample: wgpu::MultisampleState {
            count: 1,
            mask: !0,
            alpha_to_coverage_enabled: false,
        },
        multiview: None,
    })
}

fn create_depth_texture(
    device: &wgpu::Device,
    config: &wgpu::SurfaceConfiguration,
) -> wgpu::TextureView {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("Depth Texture"),
        size: wgpu::Extent3d {
            width: config.width,
            height: config.height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Depth32Float,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    });

    texture.create_view(&wgpu::TextureViewDescriptor::default())
}
