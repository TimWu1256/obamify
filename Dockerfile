# ── Stage 1: 依賴快取層 ────────────────────────────────────
FROM rust:1-slim AS deps

RUN apt-get update && apt-get install -y \
    pkg-config \
    libvulkan1 \
    mesa-vulkan-drivers \
    vulkan-tools \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

# 先複製 Cargo 相關檔案，利用 Docker layer cache（不複製 rust-toolchain 避免下載不必要的 wasm/linter 組件）
COPY Cargo.toml Cargo.lock ./
COPY crates/core/Cargo.toml    ./crates/core/Cargo.toml
COPY crates/render/Cargo.toml  ./crates/render/Cargo.toml
COPY crates/gui/Cargo.toml     ./crates/gui/Cargo.toml
COPY crates/api/Cargo.toml     ./crates/api/Cargo.toml

# 建立 placeholder source，讓 cargo fetch 先快取依賴
RUN mkdir -p crates/core/src crates/render/src crates/gui/src crates/api/src \
    && echo 'pub fn placeholder() {}' > crates/core/src/lib.rs \
    && echo 'pub fn placeholder() {}' > crates/render/src/lib.rs \
    && echo 'pub fn placeholder() {}' > crates/gui/src/lib.rs \
    && echo 'fn main() {}' > crates/gui/src/main.rs \
    && echo 'fn main() {}' > crates/api/src/main.rs

RUN cargo fetch

# ── Stage 2: 編譯 ────────────────────────────────────────────
FROM deps AS builder

COPY crates/ ./crates/
COPY presets/ ./presets/
COPY assets/ ./assets/
# src/ 也要複製（GUI crate 目前仍引用其中的資源）
COPY src/ ./src/

# 只編譯 API crate
RUN cargo build --release -p obamify-api

# ── Stage 3: 最小執行映像 ──────────────────────────────────
FROM debian:bookworm-slim AS runtime

RUN apt-get update && apt-get install -y \
    libvulkan1 \
    mesa-vulkan-drivers \
    && rm -rf /var/lib/apt/lists/* \
    && ln -sf /usr/share/vulkan/icd.d/lvp_icd.*.json /usr/share/vulkan/icd.d/lvp_icd.json

ENV VK_ICD_FILENAMES=/usr/share/vulkan/icd.d/lvp_icd.json
ENV RUST_LOG=info
ENV LISTEN_ADDR=0.0.0.0:8237

COPY --from=builder /app/target/release/obamify-api /usr/local/bin/

EXPOSE 8237
CMD ["obamify-api"]
