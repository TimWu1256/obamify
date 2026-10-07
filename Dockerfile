# ── Stage 1: 依賴快取層 ────────────────────────────────────
FROM rust:1.85-slim AS deps

RUN apt-get update && apt-get install -y \
    pkg-config \
    libvulkan1 \
    mesa-vulkan-drivers \
    vulkan-tools \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

# 先複製 Cargo 相關檔案，利用 Docker layer cache
COPY Cargo.toml Cargo.lock rust-toolchain ./
COPY crates/core/Cargo.toml    ./crates/core/Cargo.toml
COPY crates/render/Cargo.toml  ./crates/render/Cargo.toml
COPY crates/gui/Cargo.toml     ./crates/gui/Cargo.toml
COPY crates/api/Cargo.toml     ./crates/api/Cargo.toml

# 建立 placeholder source，讓 cargo fetch 先快取依賴
RUN mkdir -p crates/core/src crates/render/src crates/gui/src crates/api/src \
    && echo 'pub fn placeholder() {}' > crates/core/src/lib.rs \
    && echo 'pub fn placeholder() {}' > crates/render/src/lib.rs \
    && echo 'pub fn placeholder() {}' > crates/gui/src/lib.rs \
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
    && rm -rf /var/lib/apt/lists/*

ENV VK_ICD_FILENAMES=/usr/share/vulkan/icd.d/lvp_icd.x86_64.json
ENV RUST_LOG=info
ENV LISTEN_ADDR=0.0.0.0:3000

COPY --from=builder /app/target/release/obamify-api /usr/local/bin/

EXPOSE 3000
CMD ["obamify-api"]
