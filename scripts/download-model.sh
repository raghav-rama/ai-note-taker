#!/usr/bin/env bash
set -euo pipefail

# Download Whisper GGML models from Hugging Face.
# Usage: download-model.sh [tiny|base|small|medium] [OPTIONS]

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
MODELS_DIR="$ROOT_DIR/models"

BASE_URL="https://huggingface.co/ggerganov/whisper.cpp/resolve/main"

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

declare -A MODEL_SIZES
MODEL_SIZES=(
    [tiny]="~75 MB"
    [base]="~142 MB"
    [small]="~466 MB"
    [medium]="~1.5 GB"
)

usage() {
    cat <<EOF
Usage: $(basename "$0") MODEL [OPTIONS]

Download Whisper GGML models for local transcription.

Models:
  tiny       Fastest, lowest quality      (~75 MB)
  base       Good balance for quick notes  (~142 MB)
  small      Better accuracy               (~466 MB)
  medium     Best quality, slower          (~1.5 GB)

Options:
  --force    Re-download even if model exists
  --list     List available models and exit
  --dir DIR  Download to DIR instead of models/
  --help     Show this help

Examples:
  $(basename "$0") tiny                  # Download tiny model
  $(basename "$0") small --force         # Re-download small model
  $(basename "$0") base --dir /opt/models
EOF
    exit 0
}

log_info()  { echo -e "${GREEN}[INFO]${NC} $*"; }
log_warn()  { echo -e "${YELLOW}[WARN]${NC} $*"; }
log_error() { echo -e "${RED}[ERROR]${NC} $*"; }

list_models() {
    echo "Available Whisper GGML models:"
    echo ""
    printf "  %-10s %-12s %s\n" "Model" "Size" "Status"
    printf "  %-10s %-12s %s\n" "-----" "----" "------"
    for model in tiny base small medium; do
        local file="$MODELS_DIR/ggml-${model}.bin"
        local status="not downloaded"
        if [[ -f "$file" ]] && [[ -s "$file" ]]; then
            local size_bytes
            size_bytes=$(wc -c < "$file" | tr -d ' ')
            local size_mb=$((size_bytes / 1024 / 1024))
            status="downloaded (${size_mb} MB)"
        fi
        printf "  %-10s %-12s %s\n" "$model" "${MODEL_SIZES[$model]}" "$status"
    done
    exit 0
}

download_file() {
    local url="$1"
    local output="$2"
    local part_file="${output}.part"

    # Use curl if available, otherwise wget
    if command -v curl &>/dev/null; then
        curl -fL --progress-bar -C - -o "$part_file" "$url"
    elif command -v wget &>/dev/null; then
        wget -c --show-progress -O "$part_file" "$url"
    else
        log_error "Neither curl nor wget found. Install one to download models."
        exit 1
    fi

    # Move completed download to final location
    mv "$part_file" "$output"
}

# Parse arguments
MODEL=""
FORCE=0

while [[ $# -gt 0 ]]; do
    case "$1" in
        --force)  FORCE=1; shift ;;
        --list)   list_models ;;
        --dir)    MODELS_DIR="$2"; shift 2 ;;
        --help|-h) usage ;;
        tiny|base|small|medium)
            MODEL="$1"; shift
            ;;
        *)
            log_error "Unknown argument or invalid model: $1"
            echo ""
            echo "Valid models: tiny, base, small, medium"
            echo "Run '$(basename "$0") --help' for usage."
            exit 1
            ;;
    esac
done

if [[ -z "$MODEL" ]]; then
    log_error "No model specified."
    echo ""
    echo "Valid models: tiny, base, small, medium"
    echo "Run '$(basename "$0") --help' for usage."
    exit 1
fi

# Prepare output
mkdir -p "$MODELS_DIR"
OUTPUT_FILE="$MODELS_DIR/ggml-${MODEL}.bin"
DOWNLOAD_URL="${BASE_URL}/ggml-${MODEL}.bin"

# Check if already downloaded
if [[ -f "$OUTPUT_FILE" ]] && [[ -s "$OUTPUT_FILE" ]] && [[ "$FORCE" -eq 0 ]]; then
    local_size=$(wc -c < "$OUTPUT_FILE" | tr -d ' ')
    local_mb=$((local_size / 1024 / 1024))
    log_info "Model '$MODEL' already exists (${local_mb} MB): $OUTPUT_FILE"
    log_info "Use --force to re-download."
    exit 0
fi

log_info "Downloading Whisper model: $MODEL (${MODEL_SIZES[$MODEL]})"
log_info "URL: $DOWNLOAD_URL"
log_info "Destination: $OUTPUT_FILE"
echo ""

# Clean up partial download on failure
cleanup() {
    if [[ -f "${OUTPUT_FILE}.part" ]]; then
        log_warn "Download interrupted. Partial file preserved for resume: ${OUTPUT_FILE}.part"
    fi
}
trap cleanup EXIT

download_file "$DOWNLOAD_URL" "$OUTPUT_FILE"

# Verify download
if [[ ! -f "$OUTPUT_FILE" ]] || [[ ! -s "$OUTPUT_FILE" ]]; then
    log_error "Download failed or file is empty."
    rm -f "$OUTPUT_FILE"
    exit 1
fi

final_size=$(wc -c < "$OUTPUT_FILE" | tr -d ' ')
final_mb=$((final_size / 1024 / 1024))

echo ""
log_info "Download complete: $OUTPUT_FILE (${final_mb} MB)"
