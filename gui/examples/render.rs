//! Rasterise the console offscreen and write a PNG.
//!
//! `cargo run -p gui --example render -- /tmp/console.png`
//!
//! There is no display on the machine this was built on, and the cost of
//! that was a console that did not look like the design it was drawn from.
//! wgpu renders surfaceless, and Mesa ships lavapipe, so "no display" was
//! never the same thing as "cannot see it".

use eframe::egui;

const WIDTH: u32 = 1600;
const HEIGHT: u32 = 1000;

fn main() {
    let out = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "console.png".to_string());

    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
        backends: wgpu::Backends::VULKAN | wgpu::Backends::GL,
        ..Default::default()
    });
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::LowPower,
        force_fallback_adapter: false,
        compatible_surface: None,
    }))
    .expect("no wgpu adapter -- is a Vulkan driver (lavapipe) installed?");
    eprintln!("adapter: {:?}", adapter.get_info());

    let (device, queue) = pollster::block_on(adapter.request_device(
        &wgpu::DeviceDescriptor {
            label: Some("offscreen"),
            required_features: wgpu::Features::empty(),
            required_limits: wgpu::Limits::downlevel_defaults(),
            memory_hints: Default::default(),
        },
        None,
    ))
    .expect("request_device");

    // Drive the real console for one frame.
    let ctx = egui::Context::default();

    let mut console = gui::app::Console::new("shack.local:4532".to_string());
    // The radio's own palette, exactly as its server publishes it.
    cat_ui_egui::theme::install_with(
        &ctx,
        cat_ui_egui::theme::Palette::from_theme(&radio::console_layout::theme()),
    );
    console.demo_capabilities({
        let mut c = gui::demo::ft991a();
        c.theme = Some(radio::console_layout::theme());
        c.layout = Some(radio::console_layout::layout());
        c
    });
    console.demo_state();
    // A real band, from the same generator the emulator serves, so the
    // still shows the waterfall under something like live conditions.
    let band = cat_signal::synthetic::Band::populated(14_000_000, 14_350_000, 43, -110.0, 7);
    let frames: Vec<_> = (0..220)
        .map(|i| band.frame(14_074_000, 48_000, 1024, f64::from(i) * 0.08, i as u64))
        .collect();
    console.demo_spectrum(&frames);
    // A voice-shaped audio block: a few hundred hertz of energy inside the
    // passband, so the still shows the filter marks against something
    // rather than against an empty grid.
    // `NO_AUDIO=1` renders the other half of the AF panels' story. The
    // empty state is not a lesser version of the live one -- it has to
    // read as an instrument with nothing in it, keep its size so the rail
    // does not jump, and still show where the filter sits. That is worth
    // being able to look at.
    if std::env::var("NO_AUDIO").is_err() {
        console.demo_audio(demo_audio_frame());
    }

    let raw = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::pos2(0.0, 0.0),
            egui::vec2(WIDTH as f32, HEIGHT as f32),
        )),
        ..Default::default()
    };
    // Twice: the first pass lets egui size things it lays out from
    // previous-frame geometry, so a one-pass capture misreports widths.
    // Both frames' texture deltas are kept. The font atlas is created
    // during the FIRST frame and its delta arrives with that frame's
    // output -- and egui's *solid* shapes sample a white texel inside that
    // atlas, so discarding the first delta does not merely lose text, it
    // makes every rectangle sample a texture that was never uploaded and
    // the whole frame renders as nothing at all.
    let first = ctx.run(raw.clone(), |ctx| console.draw(ctx));
    let output = ctx.run(raw, |ctx| console.draw(ctx));
    let deltas: Vec<_> = first
        .textures_delta
        .set
        .into_iter()
        .chain(output.textures_delta.set.clone())
        .collect();

    let pixels_per_point = ctx.pixels_per_point();
    eprintln!("shapes: {}", output.shapes.len());
    let jobs = ctx.tessellate(output.shapes, pixels_per_point);
    let verts: usize = jobs
        .iter()
        .map(|j| match &j.primitive {
            egui::epaint::Primitive::Mesh(m) => m.vertices.len(),
            _ => 0,
        })
        .sum();
    eprintln!(
        "paint jobs: {}  vertices: {}  ppp: {}",
        jobs.len(),
        verts,
        pixels_per_point
    );

    let format = wgpu::TextureFormat::Rgba8UnormSrgb;
    let mut renderer = egui_wgpu::Renderer::new(&device, format, None, 1, false);
    for (id, delta) in &deltas {
        renderer.update_texture(&device, &queue, *id, delta);
    }
    eprintln!("textures uploaded: {}", deltas.len());

    let target = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("target"),
        size: wgpu::Extent3d {
            width: WIDTH,
            height: HEIGHT,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = target.create_view(&Default::default());

    let screen = egui_wgpu::ScreenDescriptor {
        size_in_pixels: [WIDTH, HEIGHT],
        pixels_per_point,
    };
    let mut encoder = device.create_command_encoder(&Default::default());
    renderer.update_buffers(&device, &queue, &mut encoder, &jobs, &screen);
    {
        let pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("egui"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &view,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color {
                        r: 0.023,
                        g: 0.031,
                        b: 0.043,
                        a: 1.0,
                    }),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
        });
        renderer.render(&mut pass.forget_lifetime(), &jobs, &screen);
    }

    // Read back. Rows are padded to 256 bytes, which is easy to forget and
    // produces a sheared image rather than an error.
    let bytes_per_row = (WIDTH * 4).div_ceil(256) * 256;
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("readback"),
        size: u64::from(bytes_per_row) * u64::from(HEIGHT),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    encoder.copy_texture_to_buffer(
        wgpu::ImageCopyTexture {
            texture: &target,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::ImageCopyBuffer {
            buffer: &buffer,
            layout: wgpu::ImageDataLayout {
                offset: 0,
                bytes_per_row: Some(bytes_per_row),
                rows_per_image: Some(HEIGHT),
            },
        },
        wgpu::Extent3d {
            width: WIDTH,
            height: HEIGHT,
            depth_or_array_layers: 1,
        },
    );
    queue.submit([encoder.finish()]);

    let slice = buffer.slice(..);
    slice.map_async(wgpu::MapMode::Read, |_| {});
    device.poll(wgpu::Maintain::Wait);
    let data = slice.get_mapped_range();

    let mut pixels = Vec::with_capacity((WIDTH * HEIGHT * 4) as usize);
    for row in 0..HEIGHT {
        let start = (row * bytes_per_row) as usize;
        pixels.extend_from_slice(&data[start..start + (WIDTH * 4) as usize]);
    }
    image::save_buffer(&out, &pixels, WIDTH, HEIGHT, image::ColorType::Rgba8).expect("write png");
    eprintln!("wrote {out}");
}

/// One block of plausible receive audio.
///
/// Shaped rather than random: a tone at 800 Hz on a sloped noise floor, so
/// the FFT has a peak inside the passband and the scope has a wave in it.
/// Noise alone would look the same whether or not the panels worked.
fn demo_audio_frame() -> cat_signal::AudioFrame {
    let rate = 48_000.0f32;
    let samples: Vec<f32> = (0..512)
        .map(|i| {
            let t = i as f32 / rate;
            0.55 * (std::f32::consts::TAU * 800.0 * t).sin()
                + 0.12 * (std::f32::consts::TAU * 1_900.0 * t).sin()
        })
        .collect();
    let bins: Vec<f32> = (0..96)
        .map(|i| {
            let hz = i as f32 * (4_000.0 / 96.0);
            let floor = -96.0 + hz / 400.0;
            let tone = -40.0 - ((hz - 800.0) / 90.0).powi(2);
            let second = -62.0 - ((hz - 1_900.0) / 120.0).powi(2);
            floor.max(tone).max(second)
        })
        .collect();
    cat_signal::AudioFrame {
        scope: cat_signal::AudioScopeFrame {
            sample_rate_hz: rate as u32,
            samples,
            sequence: 1,
        },
        spectrum: cat_signal::AudioSpectrumFrame {
            start_hz: 0,
            span_hz: 4_000,
            bins,
            sequence: 1,
        },
    }
}
