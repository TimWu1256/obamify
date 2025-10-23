PKG := obamify
TARGET := release
BUILD_DIR := target/$(TARGET)

.PHONY: all build release install clean

all: build

build:
	cargo build

release:
	cargo build --release

install: release
	@echo "Installing $(PKG) to $$HOME/.cargo/bin (requires that directory to be in your PATH)"
	mkdir -p $$HOME/.cargo/bin
	cp $(BUILD_DIR)/$(PKG) $$HOME/.cargo/bin/$(PKG)
	@echo "Installed: $$HOME/.cargo/bin/$(PKG)"

clean:
	cargo clean
