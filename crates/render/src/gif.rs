use color_quant::NeuQuant;

pub const GIF_FRAMERATE: u32 = 8;
pub const GIF_RESOLUTION: u32 = 400;
pub const GIF_MAX_FRAMES: u32 = 140;
pub const GIF_MIN_FRAMES: u32 = 100;
pub const GIF_MAX_SIZE: usize = 10 * 1024 * 1024; // 10 MB
pub const GIF_SPEED: f32 = 1.5;
pub const GIF_PALETTE_SAMPLEFAC: i32 = 1;

pub struct GifEncoder {
    resolution: u32,
    delay: u16,  // centiseconds (100/fps)
    palette: Option<NeuQuant>,
    encoder: Option<gif::Encoder<Vec<u8>>>,
    frame_count: u32,
}

impl GifEncoder {
    pub fn new(resolution: u32, fps: u32) -> Self {
        Self {
            resolution,
            delay: ((100.0 / fps as f32) / GIF_SPEED) as u16,
            palette: None,
            encoder: None,
            frame_count: 0,
        }
    }

    /// 第一幀時建立 palette，後續幀重用
    pub fn push_frame(&mut self, rgba_data: &[u8]) {
        if self.encoder.is_none() {
            let gif_palette = NeuQuant::new(GIF_PALETTE_SAMPLEFAC, 256, rgba_data);
            let mut encoder = gif::Encoder::new(
                vec![],
                self.resolution as u16,
                self.resolution as u16,
                &gif_palette.color_map_rgb(),
            ).expect("Failed to create GIF encoder");
            
            encoder.set_repeat(gif::Repeat::Infinite).expect("Failed to set GIF repeat");
            self.palette = Some(gif_palette);
            self.encoder = Some(encoder);
        }

        if let Some(encoder) = &mut self.encoder {
            if let Some(nq) = &self.palette {
                let pixels: Vec<u8> = rgba_data
                    .chunks_exact(4)
                    .map(|pix| nq.index_of(pix) as u8)
                    .collect();
                
                let mut frame = gif::Frame::from_indexed_pixels(
                    self.resolution as u16,
                    self.resolution as u16,
                    pixels,
                    None,
                );
                
                frame.delay = self.delay;
                
                if let Err(e) = encoder.write_frame(&frame) {
                    log::error!("Failed to write GIF frame: {}", e);
                }
                self.frame_count += 1;
            }
        }
    }

    /// 完成編碼，回傳 GIF bytes
    pub fn finish(mut self) -> Vec<u8> {
        if let Some(encoder) = self.encoder.take() {
            match encoder.into_inner() {
                Ok(data) => data,
                Err(e) => {
                    log::error!("Failed to finish GIF encoding: {}", e);
                    vec![]
                }
            }
        } else {
            vec![]
        }
    }
}
