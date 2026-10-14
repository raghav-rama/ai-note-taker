#!/usr/bin/env bash
set -euo pipefail

# Build the notetaker-core Rust library for specified targets.
# Usage: build-core.sh [macos|ios|windows|linux|android|host|all] [--release|--debug]
#
# Environment variables:
#   ANDROID_NDK_HOME  — required for Android targets
#   CROSS=1           — use 'cross' instead of 'cargo' for Linux cross-compilation

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
DIST_DIR="$ROOT_DIR/dist/core"
CRATE_NAME="notetaker-core"
LIB_NAME="notetaker"
PROFILE="release"

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

usage() {
    cat <<EOF
Usage: $(basename "$0") [TARGET...] [OPTIONS]

Targets:
  macos      macOS universal binary (aarch64 + x86_64)
  ios        iOS static library (aarch64)
  windows    Windows DLL (x86_64-pc-windows-msvc)
  linux      Linux shared library (x86_64-unknown-linux-gnu)
  android    Android shared libraries (arm64-v8a, armeabi-v7a, x86_64)
  host       Build for the current host platform only
  all        Build all targets possible on this host

Options:
  --release  Release build (default)
  --debug    Debug build
  --help     Show this help

Examples:
  $(basename "$0") macos ios           # Build macOS universal + iOS
  $(basename "$0") all                 # Build everything this host supports
  $(basename "$0") android --debug     # Debug build for Android
EOF
    exit 0
}

log_info()  { echo -e "${GREEN}[INFO]${NC} $*"; }
log_warn()  { echo -e "${YELLOW}[WARN]${NC} $*"; }
log_error() { echo -e "${RED}[ERROR]${NC} $*"; }
log_skip()  { echo -e "${YELLOW}[SKIP]${NC} $*"; }

detect_host() {
    local os
    os="$(uname -s)"
    case "$os" in
        Darwin)  echo "macos" ;;
        Linux)   echo "linux" ;;
        MINGW*|MSYS*|CYGWIN*) echo "windows" ;;
        *)       echo "unknown" ;;
    esac
}

check_prerequisites() {
    local missing=0
    for cmd in rustup cargo; do
        if ! command -v "$cmd" &>/dev/null; then
            log_error "Required command '$cmd' not found. Install Rust via https://rustup.rs"
            missing=1
        fi
    done
    # cmake is needed for whisper.cpp (built by whisper-rs via the cmake crate)
    if ! command -v cmake &>/dev/null; then
        log_error "Required command 'cmake' not found. Install cmake for your platform."
        missing=1
    fi
    if [[ "$missing" -ne 0 ]]; then
        exit 1
    fi
}

ensure_target() {
    local target="$1"
    if ! rustup target list --installed | grep -q "^${target}$"; then
        log_info "Adding Rust target: $target"
        rustup target add "$target"
    fi
}

cargo_cmd() {
    if [[ "${CROSS:-0}" == "1" ]] && command -v cross &>/dev/null; then
        echo "cross"
    else
        echo "cargo"
    fi
}

build_profile_flag() {
    if [[ "$PROFILE" == "release" ]]; then
        echo "--release"
    else
        echo ""
    fi
}

profile_dir() {
    if [[ "$PROFILE" == "release" ]]; then
        echo "release"
    else
        echo "debug"
    fi
}

build_macos() {
    local host
    host="$(detect_host)"
    if [[ "$host" != "macos" ]]; then
        log_skip "macOS build requires macOS host (current: $host)"
        return 0
    fi

    log_info "Building macOS universal binary..."
    local out_dir="$DIST_DIR/macos"
    mkdir -p "$out_dir"

    local targets=("aarch64-apple-darwin" "x86_64-apple-darwin")
    local libs=()

    for target in "${targets[@]}"; do
        ensure_target "$target"
        log_info "  Building for $target..."
        cargo build -p "$CRATE_NAME" --target "$target" $(build_profile_flag) --lib
        local lib_path="$ROOT_DIR/target/$target/$(profile_dir)/lib${LIB_NAME}.dylib"
        if [[ ! -f "$lib_path" ]]; then
            log_error "Expected library not found: $lib_path"
            exit 1
        fi
        libs+=("$lib_path")
    done

    log_info "  Creating universal binary with lipo..."
    lipo -create "${libs[@]}" -output "$out_dir/lib${LIB_NAME}.dylib"

    # Fix install name to use @rpath
    install_name_tool -id "@rpath/lib${LIB_NAME}.dylib" "$out_dir/lib${LIB_NAME}.dylib"

    log_info "macOS universal binary: $out_dir/lib${LIB_NAME}.dylib"
}

build_ios() {
    local host
    host="$(detect_host)"
    if [[ "$host" != "macos" ]]; then
        log_skip "iOS build requires macOS host (current: $host)"
        return 0
    fi

    log_info "Building iOS static library..."
    local out_dir="$DIST_DIR/ios"
    mkdir -p "$out_dir"

    local target="aarch64-apple-ios"
    ensure_target "$target"

    # Set minimum iOS deployment target to avoid version mismatch warnings
    export IPHONEOS_DEPLOYMENT_TARGET="${IPHONEOS_DEPLOYMENT_TARGET:-16.0}"

    # For iOS we need ONLY the static library — dylib linking fails on iOS.
    # Use cargo rustc to pass --crate-type=staticlib directly.
    cargo rustc -p "$CRATE_NAME" --target "$target" $(build_profile_flag) --lib \
        --crate-type staticlib

    local lib_path="$ROOT_DIR/target/$target/$(profile_dir)/lib${LIB_NAME}.a"
    if [[ ! -f "$lib_path" ]]; then
        log_error "Expected static library not found: $lib_path"
        exit 1
    fi

    cp "$lib_path" "$out_dir/lib${LIB_NAME}.a"
    log_info "iOS static library: $out_dir/lib${LIB_NAME}.a"
}

build_windows() {
    local host
    host="$(detect_host)"
    if [[ "$host" != "windows" ]]; then
        log_skip "Windows build requires Windows host (or cargo-xwin on macOS/Linux). Skipping."
        log_skip "  Tip: Install cargo-xwin for cross-compilation: cargo install cargo-xwin"
        return 0
    fi

    log_info "Building Windows DLL..."
    local out_dir="$DIST_DIR/windows"
    mkdir -p "$out_dir"

    local target="x86_64-pc-windows-msvc"
    ensure_target "$target"

    cargo build -p "$CRATE_NAME" --target "$target" $(build_profile_flag) --lib

    local dll_path="$ROOT_DIR/target/$target/$(profile_dir)/${LIB_NAME}.dll"
    local lib_path="$ROOT_DIR/target/$target/$(profile_dir)/${LIB_NAME}.dll.lib"
    if [[ ! -f "$dll_path" ]]; then
        log_error "Expected DLL not found: $dll_path"
        exit 1
    fi

    cp "$dll_path" "$out_dir/${LIB_NAME}.dll"
    [[ -f "$lib_path" ]] && cp "$lib_path" "$out_dir/${LIB_NAME}.dll.lib"
    log_info "Windows DLL: $out_dir/${LIB_NAME}.dll"
}

build_linux() {
    local host
    host="$(detect_host)"
    if [[ "$host" != "linux" ]]; then
        if [[ "${CROSS:-0}" == "1" ]] && command -v cross &>/dev/null; then
            log_info "Cross-compiling Linux target using 'cross'..."
        else
            log_skip "Linux build requires Linux host (current: $host)"
            log_skip "  Tip: Set CROSS=1 and install 'cross' for cross-compilation"
            return 0
        fi
    fi

    log_info "Building Linux shared library..."
    local out_dir="$DIST_DIR/linux"
    mkdir -p "$out_dir"

    local target="x86_64-unknown-linux-gnu"
    ensure_target "$target"

    $(cargo_cmd) build -p "$CRATE_NAME" --target "$target" $(build_profile_flag) --lib

    local lib_path="$ROOT_DIR/target/$target/$(profile_dir)/lib${LIB_NAME}.so"
    if [[ ! -f "$lib_path" ]]; then
        log_error "Expected shared library not found: $lib_path"
        exit 1
    fi

    cp "$lib_path" "$out_dir/lib${LIB_NAME}.so"
    log_info "Linux shared library: $out_dir/lib${LIB_NAME}.so"
}

build_android() {
    log_info "Building Android shared libraries..."

    if [[ -z "${ANDROID_NDK_HOME:-}" ]]; then
        log_skip "Android build requires ANDROID_NDK_HOME to be set. Skipping."
        return 0
    fi

    if ! command -v cargo-ndk &>/dev/null; then
        log_error "cargo-ndk is required for Android builds. Install with: cargo install cargo-ndk"
        exit 1
    fi

    local out_dir="$DIST_DIR/android"
    mkdir -p "$out_dir"

    # Ensure all Android targets are installed
    local targets=("aarch64-linux-android" "armv7-linux-androideabi" "x86_64-linux-android")
    for target in "${targets[@]}"; do
        ensure_target "$target"
    done

    # Export CMake toolchain file for whisper.cpp / cmake crate
    local cmake_toolchain="$ANDROID_NDK_HOME/build/cmake/android.toolchain.cmake"
    if [[ -f "$cmake_toolchain" ]]; then
        export CMAKE_TOOLCHAIN_FILE="$cmake_toolchain"
        log_info "  Using CMake toolchain: $cmake_toolchain"
    else
        log_warn "  CMake toolchain not found at $cmake_toolchain"
        log_warn "  whisper.cpp C build may fail"
    fi

    # Build using cargo-ndk. --lib is required: the crate now also defines a
    # [[bin]] (transcribe-wav), and without --lib cargo-ndk would cross-compile
    # that executable for the Android targets, linking whisper.cpp's C++ into an
    # executable (not the cdylib) and failing libc++ symbol resolution.
    cargo ndk \
        -t arm64-v8a \
        -t armeabi-v7a \
        -t x86_64 \
        -o "$out_dir/jniLibs" \
        build \
        -p "$CRATE_NAME" \
        --lib \
        $(build_profile_flag)

    log_info "Android libraries: $out_dir/jniLibs/"
    if command -v find &>/dev/null; then
        find "$out_dir/jniLibs" -name "*.so" -exec echo "  {}" \;
    fi
}

generate_header() {
    if command -v cbindgen &>/dev/null; then
        log_info "Generating C header with cbindgen..."
        local include_dir="$DIST_DIR/include"
        mkdir -p "$include_dir"
        cbindgen --crate "$CRATE_NAME" --output "$include_dir/${LIB_NAME}.h" 2>/dev/null || {
            log_warn "cbindgen failed (core crate may not exist yet). Skipping header generation."
        }
    else
        log_warn "cbindgen not found. Skipping C header generation."
        log_warn "  Install with: cargo install cbindgen"
    fi
}

# Parse arguments
TARGETS=()
for arg in "$@"; do
    case "$arg" in
        --release) PROFILE="release" ;;
        --debug)   PROFILE="debug" ;;
        --help|-h) usage ;;
        macos|ios|windows|linux|android|host|all)
            TARGETS+=("$arg")
            ;;
        *)
            log_error "Unknown argument: $arg"
            usage
            ;;
    esac
done

# Default to 'host' if no target specified
if [[ ${#TARGETS[@]} -eq 0 ]]; then
    TARGETS=("host")
fi

check_prerequisites

cd "$ROOT_DIR"

FAILURES=0

for target in "${TARGETS[@]}"; do
    case "$target" in
        macos)   build_macos   || ((FAILURES++)) ;;
        ios)     build_ios     || ((FAILURES++)) ;;
        windows) build_windows || ((FAILURES++)) ;;
        linux)   build_linux   || ((FAILURES++)) ;;
        android) build_android || ((FAILURES++)) ;;
        host)
            host="$(detect_host)"
            case "$host" in
                macos)   build_macos   || ((FAILURES++)) ;;
                linux)   build_linux   || ((FAILURES++)) ;;
                windows) build_windows || ((FAILURES++)) ;;
                *) log_error "Unknown host platform: $host"; ((FAILURES++)) ;;
            esac
            ;;
        all)
            build_macos   || true
            build_ios     || true
            build_windows || true
            build_linux   || true
            build_android || true
            ;;
    esac
done

# Generate C header for FFI consumers (Swift, C#, etc.)
generate_header

if [[ "$FAILURES" -gt 0 ]]; then
    log_error "$FAILURES target(s) failed"
    exit 1
fi

log_info "Build complete. Output in: $DIST_DIR/"
