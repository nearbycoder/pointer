//! The compositor: Rune draws the HUD, and at the two effect slots in the view this draws
//! the scene the stage built (the glass under the HUD, the pointer over it) with its own
//! pipelines. One upload per frame; each slot is a handful of draws.

use std::time::Instant;

use rune::compose::{Compositor, DeviceKey, Frame};

use crate::draw::{Op, Prim, RVert, Scene};
use crate::stage::{MARK_OVER, MARK_UNDER, Stage};

const SHADER: &str = include_str!("shader.wgsl");

struct Pipes {
    format: wgpu::TextureFormat,
    device: DeviceKey,
    prim: wgpu::RenderPipeline,
    ribbon: wgpu::RenderPipeline,
    uniforms: wgpu::Buffer,
    bind: wgpu::BindGroup,
    prims: wgpu::Buffer,
    prims_cap: u64,
    verts: wgpu::Buffer,
    verts_cap: u64,
}

pub struct Gfx {
    pipes: Option<Pipes>,
    scene: Scene,
    animating: bool,
    last_frame: Option<Instant>,
}

impl Gfx {
    pub fn new() -> Gfx {
        Gfx { pipes: None, scene: Scene::default(), animating: true, last_frame: None }
    }
}

fn buffer(device: &wgpu::Device, label: &str, size: u64, usage: wgpu::BufferUsages) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor { label: Some(label), size, usage: usage | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false })
}

fn pipes(device: &wgpu::Device, key: DeviceKey, format: wgpu::TextureFormat) -> Pipes {
    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("pointer"), source: wgpu::ShaderSource::Wgsl(SHADER.into()) });
    let bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("pointer"),
        entries: &[wgpu::BindGroupLayoutEntry {
            binding: 0,
            visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
            ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None },
            count: None,
        }],
    });
    let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: Some("pointer"), bind_group_layouts: &[Some(&bgl)], immediate_size: 0 });
    let target = [Some(wgpu::ColorTargetState { format, blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING), write_mask: wgpu::ColorWrites::ALL })];
    let prim_attrs = wgpu::vertex_attr_array![0 => Float32x4, 1 => Float32x4, 2 => Float32x4, 3 => Float32x4, 4 => Float32x4];
    let prim = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("pointer prims"),
        layout: Some(&layout),
        vertex: wgpu::VertexState {
            module: &module,
            entry_point: Some("vs_prim"),
            compilation_options: Default::default(),
            buffers: &[Some(wgpu::VertexBufferLayout { array_stride: std::mem::size_of::<Prim>() as u64, step_mode: wgpu::VertexStepMode::Instance, attributes: &prim_attrs })],
        },
        fragment: Some(wgpu::FragmentState { module: &module, entry_point: Some("fs_prim"), compilation_options: Default::default(), targets: &target }),
        primitive: wgpu::PrimitiveState { topology: wgpu::PrimitiveTopology::TriangleStrip, ..Default::default() },
        depth_stencil: None,
        multisample: Default::default(),
        multiview_mask: None,
        cache: None,
    });
    let ribbon_attrs = wgpu::vertex_attr_array![0 => Float32x2, 1 => Float32x2, 2 => Float32x4, 3 => Float32x4, 4 => Float32x4];
    let ribbon = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("pointer ribbons"),
        layout: Some(&layout),
        vertex: wgpu::VertexState {
            module: &module,
            entry_point: Some("vs_ribbon"),
            compilation_options: Default::default(),
            buffers: &[Some(wgpu::VertexBufferLayout { array_stride: std::mem::size_of::<RVert>() as u64, step_mode: wgpu::VertexStepMode::Vertex, attributes: &ribbon_attrs })],
        },
        fragment: Some(wgpu::FragmentState { module: &module, entry_point: Some("fs_ribbon"), compilation_options: Default::default(), targets: &target }),
        primitive: wgpu::PrimitiveState { topology: wgpu::PrimitiveTopology::TriangleList, ..Default::default() },
        depth_stencil: None,
        multisample: Default::default(),
        multiview_mask: None,
        cache: None,
    });
    let uniforms = buffer(device, "pointer uniforms", 48, wgpu::BufferUsages::UNIFORM);
    let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("pointer"),
        layout: &bgl,
        entries: &[wgpu::BindGroupEntry { binding: 0, resource: uniforms.as_entire_binding() }],
    });
    let prims_cap = 1024 * std::mem::size_of::<Prim>() as u64;
    let verts_cap = 8192 * std::mem::size_of::<RVert>() as u64;
    Pipes {
        format,
        device: key,
        prim,
        ribbon,
        uniforms,
        bind,
        prims: buffer(device, "pointer prims", prims_cap, wgpu::BufferUsages::VERTEX),
        prims_cap,
        verts: buffer(device, "pointer ribbons", verts_cap, wgpu::BufferUsages::VERTEX),
        verts_cap,
    }
}

fn draw_ops(pass: &mut wgpu::RenderPass<'_>, p: &Pipes, ops: &[Op]) {
    if ops.is_empty() { return; }
    pass.set_bind_group(0, &p.bind, &[]);
    for op in ops {
        match *op {
            Op::Prims(a, b) => {
                pass.set_pipeline(&p.prim);
                pass.set_vertex_buffer(0, p.prims.slice(..));
                pass.draw(0..4, a..b);
            }
            Op::Ribbon(a, b) => {
                pass.set_pipeline(&p.ribbon);
                pass.set_vertex_buffer(0, p.verts.slice(..));
                pass.draw(a..b, 0..1);
            }
        }
    }
}

impl Compositor<Stage> for Gfx {
    fn render(&mut self, f: &mut Frame<'_, Stage>) {
        let started = Instant::now();
        let gap = self.last_frame.map(|t| started.duration_since(t).as_secs_f64());
        self.last_frame = Some(started);

        let (vw, vh) = f.rt.window_viewport();
        let scale = if vh > 0.0 { f.size.1 as f32 / vh } else { f.rt.window_scale() };
        let stage = &mut f.rt.state;
        stage.set_viewport(vw, vh);
        self.scene.clear(scale);
        stage.paint(&mut self.scene);
        self.animating = stage.wants_frames();

        let key = f.device_key();
        if self.pipes.as_ref().is_none_or(|p| p.format != f.format || p.device != key) {
            self.pipes = Some(pipes(f.device, key, f.format));
        }
        let p = self.pipes.as_mut().expect("made above");
        let prim_bytes = (self.scene.prims.len() * std::mem::size_of::<Prim>()) as u64;
        if prim_bytes > p.prims_cap {
            p.prims_cap = prim_bytes.next_power_of_two();
            p.prims = buffer(f.device, "pointer prims", p.prims_cap, wgpu::BufferUsages::VERTEX);
        }
        let vert_bytes = (self.scene.verts.len() * std::mem::size_of::<RVert>()) as u64;
        if vert_bytes > p.verts_cap {
            p.verts_cap = vert_bytes.next_power_of_two();
            p.verts = buffer(f.device, "pointer ribbons", p.verts_cap, wgpu::BufferUsages::VERTEX);
        }
        let srgb = if f.srgb_blend() { 1.0 } else { 0.0 };
        let (w, h) = (f.size.0 as f32, f.size.1 as f32);
        let t = f.rt.state.clock() as f32;
        let uniforms: [f32; 12] = [w / scale, h / scale, w, h, 0.0, 0.0, scale, srgb, t, 0.0, 0.0, 0.0];
        let (ubuf, pbuf, vbuf) = (p.uniforms.clone(), p.prims.clone(), p.verts.clone());
        f.write_buffer(&ubuf, 0, bytemuck::cast_slice(&uniforms));
        if prim_bytes > 0 { f.write_buffer(&pbuf, 0, bytemuck::cast_slice(&self.scene.prims)); }
        if vert_bytes > 0 { f.write_buffer(&vbuf, 0, bytemuck::cast_slice(&self.scene.verts)); }

        let clear = f.rt.clear_color();
        f.ui.prepare(f.device, f.queue, &mut f.rt.text.atlas, &f.rt.instances, f.size);
        let p = self.pipes.as_ref().expect("made above");
        let scene = &self.scene;
        f.ui.draw_with(f.encoder, f.target, clear, &mut |pass, site| {
            if site.format != p.format || site.in_layer { return; }
            match site.mark {
                MARK_UNDER => draw_ops(pass, p, &scene.under.ops),
                MARK_OVER => draw_ops(pass, p, &scene.over.ops),
                _ => {}
            }
        });
        let cpu_ms = started.elapsed().as_secs_f64() * 1000.0 + f.update_ms;
        f.rt.state.frame_drawn(gap, cpu_ms, self.scene.prims.len(), self.scene.verts.len());
    }

    fn animating(&self) -> bool {
        self.animating
    }
}
