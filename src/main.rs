use obamify::headless_render::HeadlessRenderer;
// SeedColor/SeedPos are re-exported via `morph_sim::init_image` return types; no direct import needed here
use obamify::morph_sim;
use obamify::preset::{Preset, UnprocessedPreset};
use std::path::{Path, PathBuf};
use image::imageops::FilterType;
use color_quant::NeuQuant;
// simple uniform 3-3-2 quantization will be used for a deterministic global palette

// Defaults chosen to match main branch GUI behavior
const DEFAULT_GIF_DELAY: u16 = 8; // centiseconds (1/100s) => 12.5 FPS (matches GIF_FRAMERATE=8)
const DEFAULT_RENDER_RESOLUTION: u32 = 2048; // rendering resolution (matches main branch DEFAULT_RESOLUTION)
const DEFAULT_OUTPUT_RESOLUTION: u32 = 400; // final GIF output size (matches main branch GIF_RESOLUTION)
const DEFAULT_GIF_MAX_FRAMES: u32 = 140; // matches main branch GIF_MAX_FRAMES
const DEFAULT_SIM_SPEED: f32 = 1.5; // simulation speed multiplier (matches main branch GIF_SPEED)
const DEFAULT_DST_FORCE: f32 = 0.14; // proximity importance: how much the algorithm changes the original to match target (higher = more transformation)

fn get_env_u32(key: &str, default: u32) -> u32 {
    std::env::var(key)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

#[allow(dead_code)]
fn get_env_u16(key: &str, default: u16) -> u16 {
    std::env::var(key)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

fn get_env_f32(key: &str, default: f32) -> f32 {
    std::env::var(key)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::init();

    // Ensure XDG_RUNTIME_DIR exists in headless/container environments.
    // Some containers don't set this; wgpu/Vulkan may require it for socket/runtime paths.
    match std::env::var("XDG_RUNTIME_DIR") {
        Ok(dir) => {
            if !std::path::Path::new(&dir).exists() {
                // Try to create it as a fallback
                let _ = std::fs::create_dir_all(&dir);
            }
        }
        Err(_) => {
            // Default fallback: try to create /tmp/xdg_runtime but do not set env here.
            // Prefer the container/runtime to inject XDG_RUNTIME_DIR (docker-compose sets it).
            let fallback = "/tmp/xdg_runtime";
            let _ = std::fs::create_dir_all(fallback);
        }
    }

    let args: Vec<String> = std::env::args().collect();

    // parameter sources: GIF_DELAY (centiseconds) overrides GIF_FPS if present
    let gif_delay_env = std::env::var("GIF_DELAY").ok();
    let gif_fps_env = std::env::var("GIF_FPS").ok();
    let render_resolution = get_env_u32("RENDER_RESOLUTION", DEFAULT_RENDER_RESOLUTION);
    let output_resolution = get_env_u32("OUTPUT_RESOLUTION", DEFAULT_OUTPUT_RESOLUTION);
    let gif_max_frames = get_env_u32("GIF_MAX_FRAMES", DEFAULT_GIF_MAX_FRAMES);
    let sim_speed = get_env_f32("SIM_SPEED", DEFAULT_SIM_SPEED);
    // Default to 0 so generated GIFs match example.gif frame counts/duration unless user requests holds
    let hold_frames = get_env_u32("HOLD_FRAMES", 0) as u32;
    let dst_force = get_env_f32("DST_FORCE", DEFAULT_DST_FORCE);

    // determine gif_delay (centiseconds) and effective fps
    let gif_delay: u16 = if let Some(dstr) = gif_delay_env {
        dstr.parse().ok().unwrap_or(DEFAULT_GIF_DELAY)
    } else if let Some(fstr) = gif_fps_env {
        let fps_val: f32 = fstr.parse().ok().unwrap_or(100.0 / DEFAULT_GIF_DELAY as f32);
        (100.0 / fps_val).round().max(1.0) as u16
    } else {
        DEFAULT_GIF_DELAY
    };
    let gif_fps_effective: f32 = 100.0 / gif_delay as f32;

    // CLI args and simple flag parsing for --source <path>
    let mut source_path: Option<PathBuf> = None;
    let mut positional: Vec<String> = Vec::new();
    let mut i = 1;
    while i < args.len() {
        let a = &args[i];
        if a == "--source" {
            if i + 1 < args.len() {
                source_path = Some(PathBuf::from(&args[i + 1]));
                i += 2;
                continue;
            }
        } else if a.starts_with("--source=") {
            let val = a.splitn(2, '=').nth(1).unwrap().to_string();
            source_path = Some(PathBuf::from(val));
            i += 1;
            continue;
        } else if a.starts_with("-") {
            // unknown flag - ignore
        } else {
            positional.push(a.clone());
        }
        i += 1;
    }

    // Determine output and preset. If a source image is provided, create a runtime Preset.
    let (preset_name, output_path, runtime_preset): (String, PathBuf, Option<Preset>) = if let Some(src) = source_path {
        // If positional[0] matches a built-in preset name, use it as the target mapping.
        // Otherwise use Obama target (auto mode)
        let mut target_name = "auto_obama".to_string();
        let mut out_path = PathBuf::from("custom.gif");
        let mut use_preset_target = false;

        if positional.len() >= 1 {
            // try load as preset name
            match load_preset(&positional[0]) {
                Ok(_) => {
                    target_name = positional[0].clone();
                    use_preset_target = true;
                    if positional.len() >= 2 {
                        out_path = PathBuf::from(&positional[1]);
                    } else {
                        out_path = PathBuf::from(format!("{}_from_custom.gif", target_name));
                    }
                }
                Err(_) => {
                    // treat positional[0] as output (auto mode with Obama target)
                    out_path = PathBuf::from(&positional[0]);
                }
            }
        }

        // create runtime preset (resize to 128x128 by default)
        let source_preset = create_preset_from_image(&src, 128)?;

        // Get target image: either from preset or use Obama target
    let _target_img_raw = if use_preset_target {
            let target_preset = load_preset(&target_name)?;
            target_preset.inner.source_img
        } else {
            // Auto mode: use built-in Obama target (matches GUI behavior)
            println!("Auto mode: using built-in Obama target");
            let obama_target = image::load_from_memory(include_bytes!("../assets/target256.png"))?.to_rgb8();
            // Crop to square and resize to 128x128 (matches GUI's target processing)
            let (w, h) = (obama_target.width(), obama_target.height());
            let side = w.min(h);
            let x0 = (w - side) / 2;
            let y0 = (h - side) / 2;
            let cropped = image::imageops::crop_imm(&obama_target, x0, y0, side, side).to_image();
            image::imageops::resize(&cropped, 128, 128, FilterType::Lanczos3).into_raw()
        };

        // Calculate assignments using genetic algorithm (align with GUI)
        println!("Calculating optimal pixel assignments (this may take a moment)...");
        // Build per-target weights from bundled weights256.png (matches GUI) when available
    let _weights_vec: Vec<i64> = if std::path::Path::new("assets/weights256.png").exists() {
            let weights_img = image::open("assets/weights256.png")?.to_rgb8();
            let (ww, hh) = (weights_img.width(), weights_img.height());
            let side_w = ww.min(hh);
            let xw0 = (ww - side_w) / 2;
            let yw0 = (hh - side_w) / 2;
            let cropped_w = image::imageops::crop_imm(&weights_img, xw0, yw0, side_w, side_w).to_image();
            let weights_resized = image::imageops::resize(&cropped_w, 128, 128, FilterType::Lanczos3);
            weights_resized.pixels().map(|p| p[0] as i64).collect()
        } else {
            // Fallback: uniform weights (255) as GUI does when custom target provided
            vec![255i64; (128 * 128) as usize]
        };

        let assignments = calculate_assignments_genetic(
            &source_preset.inner.source_img,
            &_target_img_raw,
            128,
            13, // proximity_importance: higher = preserve more spatial structure (GUI default is 13, range 0-50)
            &_weights_vec,
        );

        let mut preset = source_preset;
        preset.assignments = assignments;

        (target_name, out_path, Some(preset))
    } else {
        let p = if positional.len() >= 1 { positional[0].clone() } else { "wisetree".to_string() };
        let out = if positional.len() >= 2 { PathBuf::from(&positional[1]) } else { PathBuf::from(format!("{}.gif", p)) };
        (p, out, None)
    };

    println!("Loading preset: {}", preset_name);
    println!("Output: {}", output_path.display());
    println!("Parameters: render={}x{}, output={}x{}, {} frames @ {:.3} FPS (delay={}cs), sim_speed={:.2}x",
        render_resolution, render_resolution, output_resolution, output_resolution,
        gif_max_frames, gif_fps_effective, gif_delay, sim_speed);
    println!("Proximity importance (dst_force): {:.3}", dst_force);

    // load preset (either runtime or embedded) and initialize sim
    let preset = if let Some(p) = runtime_preset { p } else { load_preset(&preset_name)? };
    let (seed_count, mut seeds, colors, mut sim) = morph_sim::init_image(render_resolution, preset);

    // Apply dst_force to all cells (proximity importance parameter)
    for cell in &mut sim.cells {
        cell.set_dst_force(dst_force);
    }

    println!("Initializing headless renderer...");
    let mut renderer = HeadlessRenderer::new((render_resolution, render_resolution), seed_count, seeds.clone(), colors.clone()).await;

    // CRITICAL: Initialize simulation state before recording (matches GUI behavior)
    // GUI does 20 pre-updates in gui.rs:353 after reset_sim() before starting GIF recording
    // This allows particles to settle from their initial grid positions toward assigned destinations
    println!("Initializing simulation (20 pre-updates before rendering)...");
    for _ in 0..20 {
        sim.update(&mut seeds, render_resolution);
    }
    renderer.update_seeds(&seeds);

    println!("Rendering frames into memory (first pass)...");

    // First pass: render all frames into memory (RGBA) so we can build a global palette
    let mut frames_rgba: Vec<Vec<u8>> = Vec::with_capacity((gif_max_frames + hold_frames) as usize);

    // Simulation stepping strategy
    let mut frame_count: u32 = 0;
    let mut sim_acc: f32 = 0.0;
    let sim_steps_per_frame = (60.0 / gif_fps_effective) * sim_speed;

    while frame_count < gif_max_frames {
        // GUI rendering order (gui.rs:196): run_gpu() FIRST, then update() AFTER writing frame
        // This ensures frame N shows the state AFTER N-1 updates, matching GUI behavior
        let rgba = renderer.render();
        frames_rgba.push(rgba);

        frame_count += 1;
        if frame_count % 10 == 0 { println!("  Rendered {}/{} frames", frame_count, gif_max_frames); }

        // Update simulation AFTER rendering (matches GUI: gui.rs:211)
        sim_acc += sim_steps_per_frame;
        let mut steps = sim_acc.floor() as usize;
        if steps == 0 { steps = 1; }
        sim_acc -= steps as f32;

        for _ in 0..steps {
            sim.update(&mut seeds, render_resolution);
        }
        renderer.update_seeds(&seeds);
    }

    // optional hold frames: repeat last frame(s) but keep their delays configurable
    if hold_frames > 0 {
        if let Some(last) = frames_rgba.last().cloned() {
            for _ in 0..hold_frames { frames_rgba.push(last.clone()); }
        }
    }

    println!("Building GIF palette with NeuQuant (match GUI) and encoding GIF...");

    // ensure output directory exists
    if let Some(parent) = output_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    // Build NeuQuant palette from active seed colors (match GUI behavior)
    let mut colors_bytes: Vec<u8> = Vec::with_capacity(colors.len() * 4);
    for c in &colors {
        // GUI maps floats to bytes with 1.0 -> 255, otherwise f*256
        for f in &c.rgba {
            let b = if *f == 1.0 { 255u8 } else { (*f * 256.0) as u8 };
            colors_bytes.push(b);
        }
    }

    // Create NeuQuant palette (samplefac=1 for best quality like GUI)
    let nq = NeuQuant::new(1, 256, &colors_bytes);
    let palette_map = nq.color_map_rgb();

    let mut encoder = gif::Encoder::new(std::fs::File::create(&output_path)?, output_resolution as u16, output_resolution as u16, &palette_map)?;
    encoder.set_repeat(gif::Repeat::Infinite)?;

    // Check if we need to resize frames
    let needs_resize = render_resolution != output_resolution;

    for (i, rgba) in frames_rgba.into_iter().enumerate() {
        let final_rgba = if needs_resize {
            // Use image crate for high-quality downsampling
            let img = image::RgbaImage::from_raw(render_resolution, render_resolution, rgba)
                .ok_or("Failed to create image from RGBA data")?;
            let resized = image::imageops::resize(&img, output_resolution, output_resolution, image::imageops::FilterType::Lanczos3);
            resized.into_raw()
        } else {
            rgba
        };

        // Map pixels to NeuQuant palette indices
        let pixels: Vec<u8> = final_rgba
            .chunks_exact(4)
            .map(|pix| nq.index_of(pix) as u8)
            .collect();

        let mut frame = gif::Frame::from_indexed_pixels(output_resolution as u16, output_resolution as u16, pixels, None);
        frame.delay = gif_delay;
        encoder.write_frame(&frame)?;
        if (i + 1) % 10 == 0 { println!("  Encoded {}/{} frames", i + 1, frame_count + hold_frames); }
    }

    drop(encoder);
    println!("✓ GIF saved to: {}", output_path.display());

    Ok(())
}

fn load_preset(name: &str) -> Result<Preset, Box<dyn std::error::Error>> {
    let presets = get_presets();
    presets.into_iter().find(|p| p.inner.name == name).ok_or_else(|| format!("Preset '{}' not found", name).into())
}

fn get_presets() -> Vec<Preset> {
    macro_rules! include_presets {
        ($($name:literal),*) => {
            vec![
                $( {
                    let img = image::load_from_memory(include_bytes!(concat!("../presets/", $name, "/source.png"))).unwrap().to_rgb8();
                    Preset {
                        inner: obamify::preset::UnprocessedPreset {
                            name: $name.to_owned(),
                            width: img.width(),
                            height: img.height(),
                            source_img: img.into_raw(),
                        },
                        assignments: include_str!(concat!("../presets/", $name, "/assignments.json")).to_string().strip_prefix('[').unwrap().strip_suffix(']').unwrap().split(',').map(|s| s.parse().unwrap()).collect::<Vec<usize>>(),
                    }
                } ),*
            ]
        };
    }
    include_presets! { "wisetree", "blackhole", "cat", "cat2", "colorful" }
}

// Create a runtime Preset from an image file path.
// Mimics GUI's ensure_reasonable_size + crop/scale logic
fn create_preset_from_image(path: &Path, target_size: u32) -> Result<Preset, Box<dyn std::error::Error>> {
    let img = image::open(path)?.to_rgb8();
    let (w, h) = (img.width(), img.height());

    // Step 1: Ensure reasonable size (max 512px like GUI)
    let max_side = 512;
    let img = if w > max_side || h > max_side {
        let scale = (max_side as f32 / w as f32).min(max_side as f32 / h as f32);
        let new_w = (w as f32 * scale).round() as u32;
        let new_h = (h as f32 * scale).round() as u32;
        image::imageops::resize(&img, new_w, new_h, FilterType::Lanczos3)
    } else {
        img
    };

    // Step 2: Center-crop to square (matches GUI's CropScale::identity().apply())
    let (w, h) = (img.width(), img.height());
    let side = w.min(h);
    let x0 = (w - side) / 2;
    let y0 = (h - side) / 2;
    let cropped = image::imageops::crop_imm(&img, x0, y0, side, side).to_image();

    // Step 3: Resize to target_size x target_size (default 128, matches GUI's sidelen)
    let resized = image::imageops::resize(&cropped, target_size, target_size, FilterType::Lanczos3);

    let raw = resized.into_raw();
    let mut assignments: Vec<usize> = Vec::new();
    let n = (target_size * target_size) as usize;
    assignments.reserve(n);
    for i in 0..n { assignments.push(i); }

    Ok(Preset {
        inner: UnprocessedPreset {
            name: "custom".to_string(),
            width: target_size,
            height: target_size,
            source_img: raw,
        },
        assignments,
    })
}

// Simplified genetic algorithm for calculating pixel assignments
// Mimics GUI's process_genetic but optimized for CLI speed
#[allow(dead_code)]
fn calculate_assignments_genetic(
    source_rgb: &[u8],
    target_rgb: &[u8],
    sidelen: u32,
    proximity_importance: i64,
    weights: &[i64],
) -> Vec<usize> {
    let n = (sidelen * sidelen) as usize;
    assert_eq!(source_rgb.len(), n * 3);
    assert_eq!(target_rgb.len(), n * 3);

    // Convert to pixel structures with initial identity assignment
    let mut pixels: Vec<(u16, u16, u8, u8, u8, i64)> = Vec::with_capacity(n);
    for i in 0..n {
        let x = (i % sidelen as usize) as u16;
        let y = (i / sidelen as usize) as u16;
        let sr = source_rgb[i * 3];
        let sg = source_rgb[i * 3 + 1];
        let sb = source_rgb[i * 3 + 2];
        let tr = target_rgb[i * 3];
        let tg = target_rgb[i * 3 + 1];
        let tb = target_rgb[i * 3 + 2];

        // Calculate initial heuristic (lower is better)
        // Formula matches GUI: color * color_weight + (spatial * proximity_importance).pow(2)
        let spatial = 0i64; // initially at same position
        let color = (sr as i64 - tr as i64).pow(2)
                  + (sg as i64 - tg as i64).pow(2)
                  + (sb as i64 - tb as i64).pow(2);
        // Use per-target weight like GUI
        let color_weight = weights[i];
        let h = color * color_weight + (spatial * proximity_importance).pow(2);

        pixels.push((x, y, sr, sg, sb, h));
    }

    // Note: GUI's process_genetic (used for preset generation) does NOT use STROKE_REWARD.
    // STROKE_REWARD is only used in drawing_process_genetic (interactive drawing mode).
    // For static preset calculation, we rely purely on color + spatial heuristic.

    // Use same RNG as GUI for determinism
    let mut rng = frand::Rand::with_seed(12345);

    const SWAPS_PER_GENERATION_PER_PIXEL: usize = 128;
    let swaps_per_generation = SWAPS_PER_GENERATION_PER_PIXEL * n;

    let mut max_dist = sidelen;
    loop {
        let mut swaps_made = 0usize;
        for _ in 0..swaps_per_generation {
            let apos = rng.gen_range(0..n as u32) as usize;
            let ax = apos as u16 % sidelen as u16;
            let ay = apos as u16 / sidelen as u16;
            let bx = (ax as i16 + rng.gen_range(-(max_dist as i16)..(max_dist as i16 + 1))).clamp(0, sidelen as i16 - 1) as u16;
            let by = (ay as i16 + rng.gen_range(-(max_dist as i16)..(max_dist as i16 + 1))).clamp(0, sidelen as i16 - 1) as u16;
            let bpos = by as usize * sidelen as usize + bx as usize;

            if apos == bpos { continue; }

            let tr_a = target_rgb[apos * 3];
            let tg_a = target_rgb[apos * 3 + 1];
            let tb_a = target_rgb[apos * 3 + 2];
            let tr_b = target_rgb[bpos * 3];
            let tg_b = target_rgb[bpos * 3 + 1];
            let tb_b = target_rgb[bpos * 3 + 2];

            let (_, _, sr_a, sg_a, sb_a, h_a) = pixels[apos];
            let (_, _, sr_b, sg_b, sb_b, h_b) = pixels[bpos];

            let spatial_ab = (bx as i64 - ax as i64).pow(2) + (by as i64 - ay as i64).pow(2);
            let color_ab = (sr_a as i64 - tr_b as i64).pow(2)
                         + (sg_a as i64 - tg_b as i64).pow(2)
                         + (sb_a as i64 - tb_b as i64).pow(2);
            let color_weight_b = weights[bpos];
            let h_a_at_b = color_ab * color_weight_b + (spatial_ab * proximity_importance).pow(2);

            let spatial_ba = spatial_ab;
            let color_ba = (sr_b as i64 - tr_a as i64).pow(2)
                         + (sg_b as i64 - tg_a as i64).pow(2)
                         + (sb_b as i64 - tb_a as i64).pow(2);
            let color_weight_a = weights[apos];
            let h_b_at_a = color_ba * color_weight_a + (spatial_ba * proximity_importance).pow(2);

            let improvement_a = h_a - h_a_at_b;
            let improvement_b = h_b - h_b_at_a;
            if improvement_a + improvement_b > 0 {
                pixels.swap(apos, bpos);
                pixels[apos].5 = h_b_at_a;
                pixels[bpos].5 = h_a_at_b;
                swaps_made += 1;
            }
        }

        // shrink max_dist multiplicatively like GUI
        max_dist = (max_dist as f32 * 0.99).max(2.0) as u32;

        if max_dist < 4 && swaps_made < 10 {
            break;
        }
    }

    // Build assignments: for each target position, which source pixel is assigned
    let mut assignments = vec![0usize; n];
    for (target_pos, pixel) in pixels.iter().enumerate() {
        let source_pos = pixel.1 as usize * sidelen as usize + pixel.0 as usize;
        assignments[target_pos] = source_pos;
    }

    assignments
}
