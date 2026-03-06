#!/bin/bash
# OpenLibm Cross-Compilation for FabricOS
# Provides math functions for V8 (no libm dependency)

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(dirname "$SCRIPT_DIR")"
OPENLIBM_DIR="$PROJECT_ROOT/vendor/openlibm"
BUILD_DIR="$PROJECT_ROOT/build/openlibm"

# Target: x86_64 bare metal
target="x86_64-unknown-none"

echo "=== Building OpenLibm for FabricOS ==="
echo "Target: $target"

# Clone openlibm if needed
if [ ! -d "$OPENLIBM_DIR" ]; then
    echo "Cloning openlibm..."
    git clone https://github.com/JuliaMath/openlibm.git "$OPENLIBM_DIR"
    cd "$OPENLIBM"
    # Use stable version
    git checkout v0.8.1
fi

cd "$OPENLIBM_DIR"

# Create build directory
mkdir -p "$BUILD_DIR"

# Cross-compile settings
export CC="${CC:-clang}"
export CFLAGS="-target x86_64-unknown-none -ffreestanding -nostdlib -nostdinc -m64 -march=x86-64 -O3 -fPIC"
export LDFLAGS="-nostdlib"

# Override architecture settings for bare metal
export ARCH="x86_64"
export OS=""

echo "Cleaning previous build..."
make clean 2>/dev/null || true

echo "Building static library..."
make -j$(nproc) libopenlibm.a

echo "Copying artifacts..."
cp libopenlibm.a "$BUILD_DIR/"
cp -r include "$BUILD_DIR/"

echo "=== OpenLibm Build Complete ==="
echo "Output: $BUILD_DIR/libopenlibm.a"
echo "Headers: $BUILD_DIR/include/"
ls -la "$BUILD_DIR/"
