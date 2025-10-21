use image::imageops;
use serde::Deserialize;
use serde::Serialize;
use uuid::Uuid;

use std::error::Error;

pub trait ProgressSink {
    fn send(&mut self, msg: crate::calculate::ProgressMsg);
}
// Native-friendly adapter
impl ProgressSink for std::sync::mpsc::SyncSender<crate::calculate::ProgressMsg> {
    fn send(&mut self, msg: crate::calculate::ProgressMsg) {
        let _ = std::sync::mpsc::SyncSender::send(self, msg);
    }
}

// Allow using closures as progress sinks in WASM or test harnesses
impl<T> ProgressSink for T
where
    T: FnMut(crate::calculate::ProgressMsg),
{
    fn send(&mut self, msg: crate::calculate::ProgressMsg) {
        self(msg);
    }
}

#[allow(clippy::type_complexity)]
pub(crate) fn get_images(
    source: SourceImg,
    settings: &GenerationSettings,
) -> Result<(Vec<(u8, u8, u8)>, Vec<(u8, u8, u8)>, Vec<i64>), Box<dyn Error>> {
    let source = settings.source_crop_scale.apply(&source, settings.sidelen);
    let source_pixels = source
        .pixels()
        .map(|p| (p[0], p[1], p[2]))
        .collect::<Vec<_>>();

    let (target, weights) = settings.get_target()?;
    let target_pixels = target
        .pixels()
        .map(|p| (p[0], p[1], p[2]))
        .collect::<Vec<_>>();
    assert_eq!(source_pixels.len(), target_pixels.len());
    Ok((source_pixels, target_pixels, weights))
}

#[derive(Clone, Copy, Serialize, Deserialize, PartialEq)]
pub struct CropScale {
    pub x: f32,     // -1: all left, 0: center, 1: all right
    pub y: f32,     // -1: all top, 0: center, 1: all bottom
    pub scale: f32, // 1: fit within frame, >1: zoom in, <1: not allowed
}

impl CropScale {
    pub fn identity() -> Self {
        Self {
            x: 0.0,
            y: 0.0,
            scale: 1.0,
        }
    }

    pub fn apply(&self, img: &SourceImg, sidelen: u32) -> SourceImg {
        let (w, h) = img.dimensions();

        let s = self.scale.max(1.0);

        let base_side = w.min(h) as f32;
        let mut crop_side = (base_side / s).floor().max(1.0);

        crop_side = crop_side.min(w as f32).min(h as f32);

        let max_x_off = (w as f32 - crop_side).max(0.0);
        let max_y_off = (h as f32 - crop_side).max(0.0);

        let xn = (self.x.clamp(-1.0, 1.0) + 1.0) * 0.5;
        let yn = (self.y.clamp(-1.0, 1.0) + 1.0) * 0.5;

        let x0 = (xn * max_x_off).floor() as u32;
        let y0 = (yn * max_y_off).floor() as u32;
        let cs = crop_side as u32;
        let cropped = imageops::crop_imm(img, x0, y0, cs, cs).to_image();

        if cs == sidelen {
            cropped
        } else {
            imageops::resize(&cropped, sidelen, sidelen, imageops::FilterType::Lanczos3)
        }
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub enum Algorithm {
    Optimal,
    Genetic,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct GenerationSettings {
    pub id: Uuid,
    pub name: String,

    pub proximity_importance: i64,
    pub algorithm: Algorithm,

    pub sidelen: u32,
    custom_target: Option<(u32, u32, Vec<u8>)>,
    pub target_crop_scale: CropScale,
    pub source_crop_scale: CropScale,
}

pub type SourceImg = image::RgbImage;

impl GenerationSettings {
    pub fn default(id: Uuid, name: String) -> Self {
        Self {
            name,
            proximity_importance: 13, // 20
            algorithm: Algorithm::Genetic,
            id,
            sidelen: 128,
            custom_target: None,
            target_crop_scale: CropScale::identity(),
            source_crop_scale: CropScale::identity(),
        }
    }

    pub fn get_target(&self) -> Result<(SourceImg, Vec<i64>), Box<dyn std::error::Error>> {
        let target = self.get_raw_target();
        let target = self.target_crop_scale.apply(&target, self.sidelen);
        let weights = if self.custom_target.is_some() {
            vec![255; (self.sidelen * self.sidelen) as usize] // uniform weights
        } else {
            // If a runtime weights image exists in `assets/weights256.png`, use it; otherwise
            // fall back to uniform weights. Avoid include_bytes for weights because it's not
            // always present in CLI-only builds.
            if std::path::Path::new("assets/weights256.png").exists() {
                let target_weights = image::open("assets/weights256.png")?.to_rgb8();
                let target_weights = self.target_crop_scale.apply(&target_weights, self.sidelen);
                load_weights(target_weights)
            } else {
                vec![255; (self.sidelen * self.sidelen) as usize]
            }
        };

        Ok((target, weights))
    }

    pub(crate) fn get_raw_target(&self) -> SourceImg {
        if let Some((w, h, data)) = &self.custom_target {
            image::ImageBuffer::from_vec(*w, *h, data.clone()).unwrap()
        } else {
            // Refer to bundled assets/target256.png at repo root
            image::load_from_memory(include_bytes!("../../assets/target256.png"))
                .unwrap()
                .to_rgb8()
        }
    }

    #[allow(dead_code)]
    pub(crate) fn set_raw_target(&mut self, img: SourceImg) {
        let (w, h) = img.dimensions();
        let data = img.into_raw();
        self.custom_target = Some((w, h, data));
    }
}

pub fn load_weights(source: SourceImg) -> Vec<i64> {
    let (width, height) = source.dimensions();
    let mut weights = vec![0; (width * height) as usize];
    for (x, y, pixel) in source.enumerate_pixels() {
        let weight = pixel[0] as i64;
        weights[(y * width + x) as usize] = weight;
    }
    weights
}

// Heuristic used by both the CLI and GUI drawing algorithms. Returns a signed integer
// where lower means better match. This mirrors the GUI's heuristic function.
pub fn heuristic(
    src: (u16, u16),
    target_pos: (u16, u16),
    src_col: (u8, u8, u8),
    target_col: (u8, u8, u8),
    weight: i64,
    proximity_importance: i64,
) -> i64 {
    let spatial = (src.0 as i64 - target_pos.0 as i64).pow(2) + (src.1 as i64 - target_pos.1 as i64).pow(2);
    let color = (src_col.0 as i64 - target_col.0 as i64).pow(2)
        + (src_col.1 as i64 - target_col.1 as i64).pow(2)
        + (src_col.2 as i64 - target_col.2 as i64).pow(2);
    color * weight + (spatial * proximity_importance).pow(2)
}
