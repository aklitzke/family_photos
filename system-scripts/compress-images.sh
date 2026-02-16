#!/usr/bin/env bash
set -euo pipefail

usage() {
    echo "Usage: $0 <source_dir> <lossy_dest> <lossless_dest>"
    echo
    echo "Generate compressed versions of all images (TIF, JPG, PNG) in source_dir."
    echo "  lossy_dest:    JPEG outputs, best quality under 15MB"
    echo "  lossless_dest: PNG outputs, max compression"
    exit 1
}

if [ $# -ne 3 ]; then
    usage
fi

SOURCE_DIR="$1"
LOSSY_DEST="$2"
LOSSLESS_DEST="$3"
MAX_LOSSY_BYTES=$((15 * 1024 * 1024))

if [ ! -d "$SOURCE_DIR" ]; then
    echo "Error: source directory does not exist: $SOURCE_DIR"
    exit 1
fi

mkdir -p "$LOSSY_DEST" "$LOSSLESS_DEST"

# Export for use in subshells
export SOURCE_DIR LOSSY_DEST LOSSLESS_DEST MAX_LOSSY_BYTES

# Counters (use temp files for parallel-safe counting)
TMPDIR_COUNT=$(mktemp -d)
trap 'rm -rf "$TMPDIR_COUNT"' EXIT
export TMPDIR_COUNT

process_image() {
    local src_file="$1"
    local rel_path="${src_file#"$SOURCE_DIR"/}"
    local rel_dir
    rel_dir=$(dirname "$rel_path")
    local basename
    basename=$(basename "$rel_path")
    local name_no_ext="${basename%.*}"
    local src_ext="${basename##*.}"
    local src_ext_lower
    src_ext_lower=$(echo "$src_ext" | tr '[:upper:]' '[:lower:]')

    local lossy_out="$LOSSY_DEST/$rel_dir/${name_no_ext}.jpg"
    local lossless_out="$LOSSLESS_DEST/$rel_dir/${name_no_ext}.png"

    # If source is already under 15MB, just copy it for both outputs
    local src_size
    src_size=$(stat -c%s "$src_file")
    if [ "$src_size" -le "$MAX_LOSSY_BYTES" ]; then
        local did_copy=0

        if [ -f "$LOSSY_DEST/$rel_dir/$basename" ] || [ -f "$lossy_out" ]; then
            touch "$TMPDIR_COUNT/skip_lossy"
        else
            mkdir -p "$LOSSY_DEST/$rel_dir"
            cp "$src_file" "$LOSSY_DEST/$rel_dir/$basename"
            did_copy=1
        fi

        if [ -f "$LOSSLESS_DEST/$rel_dir/$basename" ] || [ -f "$lossless_out" ]; then
            touch "$TMPDIR_COUNT/skip_lossless"
        else
            mkdir -p "$LOSSLESS_DEST/$rel_dir"
            cp "$src_file" "$LOSSLESS_DEST/$rel_dir/$basename"
            did_copy=1
        fi

        if [ $did_copy -eq 1 ]; then
            echo "[copy] $rel_path ($(( src_size / 1024 ))KB)"
            touch "$TMPDIR_COUNT/proc_$(echo "$rel_path" | md5sum | cut -d' ' -f1)"
        fi
        return 0
    fi

    local did_lossy=0
    local did_lossless=0

    # --- Lossy JPEG ---
    if [ -f "$lossy_out" ]; then
        touch "$TMPDIR_COUNT/skip_lossy"
    else
        mkdir -p "$LOSSY_DEST/$rel_dir"
        if generate_lossy "$src_file" "$lossy_out"; then
            did_lossy=1
        else
            echo "ERROR [lossy]: $rel_path" >&2
            touch "$TMPDIR_COUNT/err_lossy_$(echo "$rel_path" | md5sum | cut -d' ' -f1)"
        fi
    fi

    # --- Lossless PNG ---
    if [ -f "$lossless_out" ]; then
        touch "$TMPDIR_COUNT/skip_lossless"
    else
        mkdir -p "$LOSSLESS_DEST/$rel_dir"
        if vips pngsave "$src_file" "$lossless_out" --compression 9 2>/dev/null; then
            did_lossless=1
        else
            echo "ERROR [lossless]: $rel_path" >&2
            touch "$TMPDIR_COUNT/err_lossless_$(echo "$rel_path" | md5sum | cut -d' ' -f1)"
        fi
    fi

    # Report
    if [ $did_lossy -eq 1 ] || [ $did_lossless -eq 1 ]; then
        local parts=""
        [ $did_lossy -eq 1 ] && parts="lossy"
        [ $did_lossless -eq 1 ] && parts="${parts:+$parts+}lossless"
        echo "[${parts}] $rel_path"
        touch "$TMPDIR_COUNT/proc_$(echo "$rel_path" | md5sum | cut -d' ' -f1)"
    fi
}

encode_jpeg() {
    local src="$1" dst="$2" quality="$3" scale="$4"
    if [ "$scale" -lt 100 ]; then
        local tmp_v
        tmp_v=$(mktemp --suffix=.v)
        local factor
        factor=$(echo "$scale / 100" | bc -l)
        vips resize "$src" "$tmp_v" "$factor" 2>/dev/null \
            || { rm -f "$tmp_v"; return 1; }
        vips jpegsave "$tmp_v" "$dst" --Q "$quality" --strip 2>/dev/null \
            || { rm -f "$tmp_v" "$dst"; return 1; }
        rm -f "$tmp_v"
    else
        vips jpegsave "$src" "$dst" --Q "$quality" --strip 2>/dev/null \
            || { rm -f "$dst"; return 1; }
    fi
}

generate_lossy() {
    local src="$1"
    local out="$2"
    local scale=100

    while true; do
        # Try Q95 first
        local tmp
        tmp=$(mktemp --suffix=.jpg)
        if encode_jpeg "$src" "$tmp" 95 "$scale" \
            && [ "$(stat -c%s "$tmp")" -le "$MAX_LOSSY_BYTES" ]; then
            mv "$tmp" "$out"
            return 0
        fi
        rm -f "$tmp"

        # Binary search for highest quality that fits under 15MB
        local lo=1 hi=94 best_q=0
        while [ "$lo" -le "$hi" ]; do
            local mid=$(( (lo + hi) / 2 ))
            tmp=$(mktemp --suffix=.jpg)
            if encode_jpeg "$src" "$tmp" "$mid" "$scale" \
                && [ "$(stat -c%s "$tmp")" -le "$MAX_LOSSY_BYTES" ]; then
                best_q=$mid
                lo=$(( mid + 1 ))
            else
                hi=$(( mid - 1 ))
            fi
            rm -f "$tmp"
        done

        if [ "$best_q" -gt 0 ]; then
            encode_jpeg "$src" "$out" "$best_q" "$scale"
            return 0
        fi

        # Q1 still too big — scale down 75%
        scale=$(( scale * 75 / 100 ))
        if [ "$scale" -lt 1 ]; then
            echo "WARN: cannot fit under 15MB even at minimum quality and scale: $src" >&2
            return 1
        fi
    done
}

export -f process_image encode_jpeg generate_lossy

# Collect all image files
echo "Scanning for images in $SOURCE_DIR..."
IMAGE_LIST=$(mktemp)
find "$SOURCE_DIR" -type f \( \
    -iname '*.tif' -o -iname '*.tiff' \
    -o -iname '*.jpg' -o -iname '*.jpeg' \
    -o -iname '*.png' \
\) | sort > "$IMAGE_LIST"

TOTAL=$(wc -l < "$IMAGE_LIST")
echo "Found $TOTAL images."
echo

if [ "$TOTAL" -eq 0 ]; then
    echo "No images found."
    rm -f "$IMAGE_LIST"
    exit 0
fi

# Process in parallel
cat "$IMAGE_LIST" | xargs -P"$(nproc)" -I{} bash -c 'process_image "$@"' _ {}

rm -f "$IMAGE_LIST"

# Summary
PROCESSED=$(find "$TMPDIR_COUNT" -name 'proc_*' 2>/dev/null | wc -l)
SKIPPED_LOSSY=$(find "$TMPDIR_COUNT" -name 'skip_lossy' 2>/dev/null | wc -l)
SKIPPED_LOSSLESS=$(find "$TMPDIR_COUNT" -name 'skip_lossless' 2>/dev/null | wc -l)
ERRORS_LOSSY=$(find "$TMPDIR_COUNT" -name 'err_lossy_*' 2>/dev/null | wc -l)
ERRORS_LOSSLESS=$(find "$TMPDIR_COUNT" -name 'err_lossless_*' 2>/dev/null | wc -l)

echo
echo "=== Summary ==="
echo "Total images found: $TOTAL"
echo "Processed (new):    $PROCESSED"
echo "Skipped lossy:      $SKIPPED_LOSSY"
echo "Skipped lossless:   $SKIPPED_LOSSLESS"
echo "Errors (lossy):     $ERRORS_LOSSY"
echo "Errors (lossless):  $ERRORS_LOSSLESS"

TOTAL_ERRORS=$(( ERRORS_LOSSY + ERRORS_LOSSLESS ))
if [ "$TOTAL_ERRORS" -gt 0 ]; then
    exit 1
fi
