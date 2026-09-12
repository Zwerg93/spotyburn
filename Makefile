.PHONY: all build build-release test lint fmt clean

all: build

build:
	cargo build --manifest-path src-tauri/Cargo.toml

build-release:
	cargo build --manifest-path src-tauri/Cargo.toml --release

test:
	cargo test --manifest-path src-tauri/Cargo.toml

lint:
	cargo fmt --manifest-path src-tauri/Cargo.toml --check
	cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings

fmt:
	cargo fmt --manifest-path src-tauri/Cargo.toml

clean:
	cargo clean --manifest-path src-tauri/Cargo.toml
	rm -rf target/bundle

bundle-macos:
	bash scripts/build_macos_bundle.sh

dmg: bundle-macos
