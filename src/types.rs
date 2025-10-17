/// 共享的核心類型定義
use bytemuck::{Pod, Zeroable};

#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct SeedPos {
    pub xy: [f32; 2],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub struct SeedColor {
    pub rgba: [f32; 4],
}
