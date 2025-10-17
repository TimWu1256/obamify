#![warn(clippy::all, rust_2018_idioms)]

// 核心類型
pub mod types;
pub use types::{SeedPos, SeedColor};

// Headless 渲染模組（CLI 使用）
pub mod headless_render;

// 模擬和 preset 模塊
pub mod morph_sim;
pub mod preset;
