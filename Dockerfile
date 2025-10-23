FROM rust:1.85 AS base

# 安裝必要的依賴
RUN apt-get update && apt-get install -y \
    libvulkan1 \
    mesa-vulkan-drivers \
    vulkan-tools \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

# ensure output directory exists for container runtime
RUN mkdir -p /app/output

# 設定環境變數
ENV VK_ICD_FILENAMES=/usr/share/vulkan/icd.d/lvp_icd.x86_64.json
ENV RUST_LOG=info

# Development stage - no compilation, just environment
FROM base AS development
COPY docker-entrypoint.sh /usr/local/bin/
RUN chmod +x /usr/local/bin/docker-entrypoint.sh
ENTRYPOINT ["docker-entrypoint.sh"]
CMD ["cargo", "run", "--release", "--bin", "obamify", "--", "wisetree", "output/wisetree.gif"]

# Production stage - copy code and pre-compile
FROM base AS production
COPY Cargo.toml Cargo.lock rust-toolchain ./
COPY src/ ./src/
COPY presets/ ./presets/
COPY assets/ ./assets/

RUN cargo build --release --bin obamify

# Set the binary as entrypoint so args can be passed directly
ENTRYPOINT ["./target/release/obamify"]
CMD ["--preset", "wisetree", "--output", "output/wisetree.gif"]
