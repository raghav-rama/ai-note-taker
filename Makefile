# AI Note Taker — Top-level Build Orchestration
#
# Assumes core/Cargo.toml declares:
#   [lib]
#   name = "notetaker"
#   crate-type = ["cdylib", "staticlib", "rlib"]
#
#   [features]
#   metal   = ["whisper-rs/metal"]
#   coreml  = ["whisper-rs/coreml"]
#   cuda    = ["whisper-rs/cuda"]
#   vulkan  = ["whisper-rs/vulkan"]
#
# Feature flags forwarded to whisper-rs must match these names.

CARGO        ?= cargo
PROFILE      ?= release
MODEL        ?= tiny
MODEL_URL    := https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-$(MODEL).bin

UNAME_S := $(shell uname -s 2>/dev/null || echo Unknown)

CARGO_FLAGS :=
ifeq ($(PROFILE),release)
  CARGO_FLAGS += --release
endif

# ─── Core library ────────────────────────────────────────────────────────────

.PHONY: build-core
build-core:
	$(CARGO) build --manifest-path core/Cargo.toml $(CARGO_FLAGS)

# ─── macOS ───────────────────────────────────────────────────────────────────

.PHONY: build-macos
build-macos:
ifeq ($(UNAME_S),Darwin)
	# Build core for both Apple Silicon and Intel
	$(CARGO) build --manifest-path core/Cargo.toml $(CARGO_FLAGS) \
		--target aarch64-apple-darwin --features metal
	$(CARGO) build --manifest-path core/Cargo.toml $(CARGO_FLAGS) \
		--target x86_64-apple-darwin
	# Create universal binary
	mkdir -p core/target/universal/$(PROFILE)
	lipo -create \
		core/target/aarch64-apple-darwin/$(PROFILE)/libnotetaker.dylib \
		core/target/x86_64-apple-darwin/$(PROFILE)/libnotetaker.dylib \
		-output core/target/universal/$(PROFILE)/libnotetaker.dylib
	# Build macOS app
	# NOTE: apple/NoteHelper.xcodeproj does not exist yet; it is generated via
	# XcodeGen (apple/project.yml) in a later phase.
	xcodebuild -project apple/NoteHelper.xcodeproj \
		-scheme NoteHelper-macOS \
		-configuration Release \
		-derivedDataPath apple/build
else
	@echo "SKIP: build-macos requires macOS (current: $(UNAME_S))"
endif

# ─── iOS ─────────────────────────────────────────────────────────────────────

.PHONY: build-ios
build-ios:
ifeq ($(UNAME_S),Darwin)
	# Build core for iOS device and simulator
	$(CARGO) build --manifest-path core/Cargo.toml $(CARGO_FLAGS) \
		--target aarch64-apple-ios --features metal
	$(CARGO) build --manifest-path core/Cargo.toml $(CARGO_FLAGS) \
		--target aarch64-apple-ios-sim --features metal
	# Create XCFramework
	xcodebuild -create-xcframework \
		-library core/target/aarch64-apple-ios/$(PROFILE)/libnotetaker.a \
		-library core/target/aarch64-apple-ios-sim/$(PROFILE)/libnotetaker.a \
		-output core/target/NoteCore.xcframework
	# Build iOS app
	# NOTE: apple/NoteHelper.xcodeproj does not exist yet; it is generated via
	# XcodeGen (apple/project.yml) in a later phase.
	xcodebuild -project apple/NoteHelper.xcodeproj \
		-scheme NoteHelper-iOS \
		-configuration Release \
		-sdk iphoneos \
		-derivedDataPath apple/build
else
	@echo "SKIP: build-ios requires macOS (current: $(UNAME_S))"
endif

# ─── Windows ─────────────────────────────────────────────────────────────────

.PHONY: build-windows
build-windows:
	$(CARGO) build --manifest-path core/Cargo.toml $(CARGO_FLAGS) \
		--target x86_64-pc-windows-msvc
	dotnet build windows/NoteHelper/NoteHelper.sln -c Release

# ─── Linux ───────────────────────────────────────────────────────────────────

.PHONY: build-linux
build-linux:
	# Linux app is a direct Rust dependency on core — no FFI needed
	$(CARGO) build --manifest-path linux/Cargo.toml $(CARGO_FLAGS)

# ─── Android ─────────────────────────────────────────────────────────────────

ANDROID_TARGETS := aarch64-linux-android armv7-linux-androideabi \
                   x86_64-linux-android i686-linux-android

.PHONY: build-android
build-android:
	# Build core for all Android ABIs via cargo-ndk
	cargo ndk --manifest-path core/Cargo.toml \
		--target aarch64-linux-android \
		--target armv7-linux-androideabi \
		--target x86_64-linux-android \
		--target i686-linux-android \
		--platform 26 \
		-- build $(CARGO_FLAGS)
	# Build Android app
	cd android && ./gradlew assembleRelease

# ─── Aggregate ───────────────────────────────────────────────────────────────

.PHONY: build-all
build-all: build-core build-macos build-ios build-windows build-linux build-android

# ─── Tests ───────────────────────────────────────────────────────────────────

.PHONY: test
test:
	$(CARGO) test --manifest-path core/Cargo.toml
	$(CARGO) clippy --manifest-path core/Cargo.toml -- -D warnings

# ─── Models ──────────────────────────────────────────────────────────────────

.PHONY: download-models
download-models:
	mkdir -p models
	@if [ ! -f models/ggml-$(MODEL).bin ]; then \
		echo "Downloading whisper $(MODEL) model..."; \
		curl -L -o models/ggml-$(MODEL).bin "$(MODEL_URL)"; \
	else \
		echo "Model models/ggml-$(MODEL).bin already exists."; \
	fi

# ─── Clean ───────────────────────────────────────────────────────────────────

.PHONY: clean
clean:
	$(CARGO) clean --manifest-path core/Cargo.toml 2>/dev/null || true
	$(CARGO) clean --manifest-path linux/Cargo.toml 2>/dev/null || true
	rm -rf apple/build
	rm -rf android/app/build
	cd windows && dotnet clean NoteHelper/NoteHelper.sln 2>/dev/null || true

.PHONY: clean-models
clean-models:
	rm -rf models/*.bin
